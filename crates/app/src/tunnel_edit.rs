//! Explicit `[Tubes]` route authoring and canvas markers.
use crate::{
    camera::MapCamera,
    document::{Cell, MapDocument, Target, TerrainValue, Value, cell_line},
    editing::{Editor, Tool},
    i18n::tr,
    map::MapScene,
};
use anyhow::{Context, Result, ensure};
use eframe::egui::{self, Color32, Stroke};
use ra2_map_assets::{catalog::TerrainBrush, terrain_library::TerrainCategory};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PATH_STEPS: usize = 99;
const PORTAL_BACKUP_SECTION: &str = "TunnelPortalBackup";
const PORTAL_FACING_SECTION: &str = "TunnelPortalFacing";
const DIRECTIONS: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];
const DIRECTION_NAMES: [&str; 8] = ["北", "东北", "东", "东南", "南", "西南", "西", "西北"];

#[derive(Clone, Debug)]
struct TunnelRecord {
    target: Target,
    key: String,
    entry: Cell,
    exit: Cell,
    direction: i32,
    exit_facing: Option<u8>,
    steps: Vec<i32>,
    error: Option<String>,
}

pub(super) struct TunnelEdit {
    selected: Option<Target>,
    draft: Option<TunnelDraft>,
    auto_art: bool,
    art_family: Option<u16>,
}

#[derive(Clone)]
struct TunnelDraft {
    replace: Option<Target>,
    points: Vec<Cell>,
    direction: u8,
}

#[derive(Clone, Debug)]
struct PortalPiece {
    tile_id: u16,
    anchor_subtile: u8,
}

#[derive(Clone, Debug)]
struct TunnelArtFamily {
    group_id: u16,
    name: String,
    x_axis: PortalPiece,
    y_axis: PortalPiece,
}

#[derive(Clone)]
struct PortalTerrainBackup {
    target: Target,
    cell: Cell,
    terrain: Option<TerrainValue>,
    placed: Option<TerrainValue>,
}

impl Default for TunnelEdit {
    fn default() -> Self {
        Self {
            selected: None,
            draft: None,
            auto_art: true,
            art_family: None,
        }
    }
}

fn portal_anchor(scene: &MapScene, tile_id: u16, width: u16, height: u16) -> Option<u8> {
    let template = scene.palette.templates.get(&(tile_id, 0))?;
    let tunnel_cells: Vec<_> = template
        .cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| cell.filter(|cell| cell.terrain == 5).map(|_| index as u8))
        .collect();
    if !tunnel_cells.is_empty() {
        return tunnel_cells.get(tunnel_cells.len() / 2).copied();
    }
    // Stock temperate/dirt arches put Railroad/Rock (TMP 6/7) on the
    // wall, and Road (11/12) just in front of it. Rock cannot be a tube
    // endpoint: the ordinary drive into the entry and out of the exit
    // still go through the land's speed/passability checks.
    let wall_cells: Vec<_> = template
        .cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            cell.filter(|cell| matches!(cell.terrain, 6 | 7))
                .map(|_| index as u8)
        })
        .collect();
    if let Some(&wall) = wall_cells.get(wall_cells.len() / 2) {
        let (wx, wy) = (u16::from(wall) % width, u16::from(wall) / width);
        if let Some(road) = template
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.is_some_and(|cell| matches!(cell.terrain, 11 | 12)))
            .min_by_key(|(index, _)| {
                let index = *index as u16;
                (index % width).abs_diff(wx) + (index / width).abs_diff(wy)
            })
            .and_then(|(index, _)| u8::try_from(index).ok())
        {
            return Some(road);
        }
    }
    // Keep a sensible fallback for custom theaters that only encode the
    // mouth visually without one of the stock land types.
    let center_x = (width - 1) / 2;
    let center_y = (height - 1) / 2;
    (0..template.cells.len())
        .filter(|&index| {
            template.cells[index].is_some_and(|cell| {
                use vera20k_core::rules::terrain_rules::{LandType, tmp_terrain_to_land_type};
                !matches!(
                    tmp_terrain_to_land_type(cell.terrain),
                    LandType::Water | LandType::Rock | LandType::Wall
                )
            })
        })
        .min_by_key(|&index| {
            let x = index as u16 % width;
            let y = index as u16 / width;
            x.abs_diff(center_x) + y.abs_diff(center_y)
        })
        .and_then(|index| u8::try_from(index).ok())
}

fn tunnel_art_families(scene: &MapScene) -> Vec<TunnelArtFamily> {
    let mut groups = BTreeMap::<u16, (String, Vec<u16>)>::new();
    for (&tile_id, group) in &scene.palette.groups {
        if group.category == TerrainCategory::Tunnel {
            let entry = groups
                .entry(group.id)
                .or_insert_with(|| (group.name.clone(), Vec::new()));
            entry.1.push(tile_id);
        }
    }
    let mut families = Vec::new();
    for (group_id, (name, mut tiles)) in groups {
        tiles.sort_unstable();
        tiles.dedup();
        let mut x_axis = None;
        let mut y_axis = None;
        for tile_id in tiles {
            let Some(template) = scene.palette.templates.get(&(tile_id, 0)) else {
                continue;
            };
            let Some(anchor_subtile) =
                portal_anchor(scene, tile_id, template.width, template.height)
            else {
                continue;
            };
            let piece = PortalPiece {
                tile_id,
                anchor_subtile,
            };
            match (template.width, template.height) {
                (4, 3) => x_axis = Some(piece),
                (3, 4) => y_axis = Some(piece),
                _ => {}
            }
        }
        if let (Some(x_axis), Some(y_axis)) = (x_axis, y_axis) {
            families.push(TunnelArtFamily {
                group_id,
                name,
                x_axis,
                y_axis,
            });
        }
    }
    families.sort_by_key(|family| {
        (
            family.name.to_ascii_lowercase().contains("dirt"),
            family.group_id,
        )
    });
    families
}

fn tunnel_cardinal(direction: u8) -> u8 {
    let direction = direction & 7;
    if direction.is_multiple_of(2) {
        direction
    } else {
        direction.wrapping_add(1) & 7
    }
}

fn exit_facing(tunnel: &TunnelRecord) -> Option<u8> {
    tunnel
        .exit_facing
        .or_else(|| tunnel.steps.last().map(|step| (step & 7) as u8))
        .or_else(|| {
            u8::try_from(tunnel.direction)
                .ok()
                .filter(|&direction| direction < 8)
        })
}

fn portal_piece_for_facing(family: &TunnelArtFamily, direction: u8) -> &PortalPiece {
    match tunnel_cardinal(direction) {
        // The 3x4 TMP is the north/east-facing artwork; the 4x3 TMP faces
        // south/west. These are opposite directional pieces, not axis labels.
        0 | 2 => &family.y_axis,
        4 | 6 => &family.x_axis,
        _ => unreachable!("tunnel_cardinal returns a cardinal direction"),
    }
}

fn portal_brush<'a>(scene: &'a MapScene, piece: &PortalPiece) -> Result<&'a TerrainBrush> {
    scene
        .brushes
        .iter()
        .find(|brush| {
            brush.key.tile_id == piece.tile_id
                && brush.key.sub_tile == piece.anchor_subtile
                && brush.key.variant == 0
        })
        .context("隧道口地形素材未完整加载")
}

fn tunnel_portal_edits(
    doc: &MapDocument,
    scene: &MapScene,
    family: &TunnelArtFamily,
    points: &[Cell],
    exit_direction: u8,
) -> Result<Vec<(Cell, TerrainValue)>> {
    let entry = *points.first().context(tr!("请先绘制入口"))?;
    let exit = *points.last().context(tr!("请先绘制出口"))?;
    let steps = draft_steps(points)?;
    let entry_facing = (steps[0] as u8).wrapping_add(4) & 7;
    let endpoints = [
        (entry, portal_piece_for_facing(family, entry_facing)),
        (exit, portal_piece_for_facing(family, exit_direction)),
    ];
    let mut edits = BTreeMap::new();
    let mut occupied = BTreeSet::new();
    let other_portals: BTreeSet<_> = doc
        .ini_entries(PORTAL_BACKUP_SECTION)
        .into_iter()
        .filter_map(|(target, _, bytes)| parse_portal_backup(target, &bytes).ok())
        .map(|backup| backup.cell)
        .collect();
    let object_cells: BTreeSet<_> = doc
        .record_iter()
        .filter(|(_, record)| !matches!(record.section.as_str(), "Waypoints" | "Smudge"))
        .flat_map(|(_, record)| {
            ra2_map_scene::placement::footprint(scene, record)
                .unwrap_or_else(|_| record.cell().into_iter().collect())
        })
        .collect();
    for (cell, piece) in endpoints {
        let brush = portal_brush(scene, piece)?;
        for (at, value) in
            ra2_map_scene::terrain_tools::template_placement(doc, scene, brush, cell, None)?
        {
            ensure!(occupied.insert(at), "隧道两端距离太近，入口素材会重叠");
            ensure!(
                !other_portals.contains(&at),
                "{}",
                tr!("洞口与另一条隧道的洞口重叠")
            );
            ensure!(
                doc.overlay(at).0 == 255 && !object_cells.contains(&at),
                "隧道口素材覆盖到现有对象或覆盖物"
            );
            edits.insert(at, value);
        }
    }
    Ok(edits.into_iter().collect())
}

fn portal_backup_key(tunnel_key: &str, cell: Cell) -> String {
    format!("T{tunnel_key}_{}_{}", cell[0], cell[1])
}

fn portal_backup_owner(key: &str) -> Option<&str> {
    let (key, _) = key.rsplit_once('_')?;
    let (key, _) = key.rsplit_once('_')?;
    key.strip_prefix('T')
}

fn parse_portal_backup(target: Target, bytes: &[u8]) -> Result<PortalTerrainBackup> {
    let fields: Vec<_> = std::str::from_utf8(bytes)?.split(',').collect();
    ensure!(
        matches!(fields.len(), 3 | 6 | 10),
        "invalid tunnel portal backup"
    );
    let cell = [fields[0].parse::<u16>()?, fields[1].parse::<u16>()?];
    let terrain = if fields.len() == 3 && fields[2] == "-1" {
        ensure!(fields.len() == 3, "invalid absent-terrain backup");
        None
    } else {
        ensure!(matches!(fields.len(), 6 | 10), "incomplete terrain backup");
        Some(TerrainValue {
            tile: fields[2].parse()?,
            sub: fields[3].parse()?,
            level: fields[4].parse()?,
            ice: fields[5].parse()?,
        })
    };
    Ok(PortalTerrainBackup {
        target,
        cell,
        terrain,
        placed: if fields.len() == 10 {
            Some(TerrainValue {
                tile: fields[6].parse()?,
                sub: fields[7].parse()?,
                level: fields[8].parse()?,
                ice: fields[9].parse()?,
            })
        } else {
            None
        },
    })
}

fn serialize_portal_backup(cell: Cell, terrain: TerrainValue, placed: TerrainValue) -> Vec<u8> {
    let mut fields = vec![cell[0].to_string(), cell[1].to_string()];
    for value in [terrain, placed] {
        fields.extend([
            value.tile.to_string(),
            value.sub.to_string(),
            value.level.to_string(),
            value.ice.to_string(),
        ]);
    }
    fields.join(",").into_bytes()
}

fn restore_portal_backups(
    doc: &mut MapDocument,
    scene: &MapScene,
    tunnel: &TunnelRecord,
) -> Result<()> {
    let backups: Vec<_> = doc
        .ini_entries(PORTAL_BACKUP_SECTION)
        .into_iter()
        .filter(|(_, key, _)| portal_backup_owner(key) == Some(tunnel.key.as_str()))
        .map(|(target, _, bytes)| parse_portal_backup(target, &bytes))
        .collect::<Result<_>>()?;
    if !backups.is_empty() {
        for backup in backups {
            // Restore only cells that still contain our stamp. A manual edit
            // made after authoring the portal belongs to the user.
            if backup.placed.map_or_else(
                || is_portal_cell(doc, scene, backup.cell),
                |placed| doc.terrain(backup.cell) == Some(placed),
            ) {
                let terrain = backup.terrain.unwrap_or_else(|| clear_terrain(scene));
                doc.set(Target::Terrain(backup.cell), Some(Value::Terrain(terrain)))?;
            }
            doc.set(backup.target, None)?;
        }
        return Ok(());
    }

    // Maps saved before portal backups existed can still remove editor-stamped
    // tunnel TMP cells. Their former terrain is unknowable, so restore clear
    // ground while preserving height and ice.
    let Some(clear_tile) = scene
        .palette
        .tile_ids
        .and_then(|ids| u16::try_from(ids.clear).ok())
        .and_then(|clear| scene.catalog.raw_tiles.get(&clear).copied())
    else {
        return Ok(());
    };
    if tunnel.error.is_some() || tunnel.steps.is_empty() {
        return Ok(());
    }
    let endpoints = [tunnel.entry, tunnel.exit];
    let families = tunnel_art_families(scene);
    for cell in endpoints {
        let mut stamped_cells = BTreeSet::new();
        for family in &families {
            for piece in [&family.x_axis, &family.y_axis] {
                let Ok(default_brush) = portal_brush(scene, piece) else {
                    continue;
                };
                // Pre-fix versions anchored on the wall instead of its road.
                // Use the actual stamped subtile when removing that artwork.
                let brush = doc
                    .terrain(cell)
                    .filter(|terrain| terrain.tile == default_brush.raw)
                    .and_then(|terrain| {
                        scene.brushes.iter().find(|brush| {
                            brush.key.tile_id == piece.tile_id
                                && brush.key.sub_tile == terrain.sub
                                && brush.key.variant == 0
                        })
                    })
                    .unwrap_or(default_brush);
                let Ok(placement) =
                    ra2_map_scene::terrain_tools::template_placement(doc, scene, brush, cell, None)
                else {
                    continue;
                };
                // Check the entire template, including subtiles, before clearing
                // legacy art. Direction may have changed since it was stamped.
                if !placement
                    .iter()
                    .all(|(at, value)| doc.terrain(*at) == Some(*value))
                {
                    continue;
                }
                for (at, _) in placement {
                    if let Some(current) = doc.terrain(at)
                        && current.tile == brush.raw
                    {
                        stamped_cells.insert(at);
                    }
                }
            }
        }
        for at in stamped_cells {
            if let Some(current) = doc.terrain(at) {
                doc.set(
                    Target::Terrain(at),
                    Some(Value::Terrain(TerrainValue {
                        tile: clear_tile,
                        sub: 0,
                        ..current
                    })),
                )?;
            }
        }
    }
    Ok(())
}

fn clear_terrain(scene: &MapScene) -> TerrainValue {
    TerrainValue {
        tile: scene
            .palette
            .tile_ids
            .and_then(|ids| u16::try_from(ids.clear).ok())
            .and_then(|id| scene.catalog.raw_tiles.get(&id).copied())
            .unwrap_or(0),
        sub: 0,
        level: scene.source.header.level as u8,
        ice: 0,
    }
}

fn is_portal_cell(doc: &MapDocument, scene: &MapScene, at: Cell) -> bool {
    doc.terrain(at).is_some_and(|value| {
        tunnel_art_families(scene).iter().any(|family| {
            [&family.x_axis, &family.y_axis]
                .iter()
                .any(|piece| portal_brush(scene, piece).is_ok_and(|brush| brush.raw == value.tile))
        })
    })
}

fn parse_record(target: Target, key: String, bytes: &[u8]) -> TunnelRecord {
    let mut row = TunnelRecord {
        target,
        key,
        entry: [0, 0],
        exit: [0, 0],
        direction: 0,
        exit_facing: None,
        steps: Vec::new(),
        error: None,
    };
    let parsed = (|| -> Result<()> {
        let text = std::str::from_utf8(bytes).context(tr!("隧道记录不是 UTF-8"))?;
        let fields: Vec<_> = text.split(',').map(str::trim).collect();
        ensure!(fields.len() >= 6, "{}", tr!("隧道记录缺少路径结束标记"));
        let number = |index: usize| -> Result<i32> {
            fields
                .get(index)
                .context(tr!("隧道记录字段不足"))?
                .parse::<i32>()
                .context(tr!("隧道记录字段必须是整数"))
        };
        row.entry = [
            u16::try_from(number(0)?).context(tr!("入口 X 坐标超出范围"))?,
            u16::try_from(number(1)?).context(tr!("入口 Y 坐标超出范围"))?,
        ];
        row.direction = number(2)?;
        row.exit = [
            u16::try_from(number(3)?).context(tr!("出口 X 坐标超出范围"))?,
            u16::try_from(number(4)?).context(tr!("出口 Y 坐标超出范围"))?,
        ];
        let mut terminated = false;
        for (slot, field) in fields.iter().skip(5).take(100).enumerate() {
            let step = field.parse::<i32>().context(tr!("路径步骤必须是整数"))?;
            if step == -1 {
                terminated = true;
                break;
            }
            // The engine's 100-iteration reader retains the first 99 steps.
            if slot == 99 {
                terminated = true;
                break;
            }
            row.steps.push(step);
        }
        ensure!(terminated, "{}", tr!("隧道路径缺少 -1 结束标记"));
        Ok(())
    })();
    if parsed.is_err() {
        row.error = Some(tr!("隧道记录格式无效，入口、出口或路径步骤无法读取。").into());
    }
    row
}

fn rows(doc: &MapDocument) -> Vec<TunnelRecord> {
    doc.ini_entries("Tubes")
        .into_iter()
        .map(|(target, key, value)| {
            let facing = doc
                .ini_value(PORTAL_FACING_SECTION, &key)
                .and_then(|bytes| std::str::from_utf8(&bytes).ok()?.parse::<u8>().ok())
                .filter(|&facing| facing < 8);
            let mut tunnel = parse_record(target, key, &value);
            tunnel.exit_facing = facing;
            tunnel
        })
        .collect()
}

fn route(entry: Cell, steps: &[i32]) -> Vec<Cell> {
    let mut result = vec![entry];
    let mut at = entry;
    for &step in steps {
        let &(dx, dy) = &DIRECTIONS[(step as u32 & 7) as usize];
        let Some(x) = u16::try_from(i32::from(at[0]) + dx).ok() else {
            break;
        };
        let Some(y) = u16::try_from(i32::from(at[1]) + dy).ok() else {
            break;
        };
        at = [x, y];
        result.push(at);
    }
    result
}

fn draft_steps(points: &[Cell]) -> Result<Vec<i32>> {
    ensure!(points.len() >= 2, "{}", tr!("至少绘制入口和一个后续格"));
    let steps: Vec<_> = points
        .windows(2)
        .map(|pair| {
            let dx = i32::from(pair[1][0]) - i32::from(pair[0][0]);
            let dy = i32::from(pair[1][1]) - i32::from(pair[0][1]);
            DIRECTIONS
                .iter()
                .position(|&delta| delta == (dx, dy))
                .context(tr!("相邻路径格必须相隔一个八方向步长"))
                .map(|direction| direction as i32)
        })
        .collect::<Result<_>>()?;
    ensure!(
        steps.len() <= MAX_PATH_STEPS,
        "{}",
        tr!("隧道最多支持 {arg0} 个路径步骤", arg0 = MAX_PATH_STEPS)
    );
    Ok(steps)
}

fn serialize(entry: Cell, exit: Cell, direction: u8, steps: &[i32]) -> Vec<u8> {
    let mut fields = vec![
        entry[0].to_string(),
        entry[1].to_string(),
        direction.to_string(),
        exit[0].to_string(),
        exit[1].to_string(),
    ];
    fields.extend(steps.iter().map(ToString::to_string));
    fields.push("-1".into());
    fields.join(",").into_bytes()
}

fn next_key(doc: &MapDocument) -> Result<String> {
    let used: std::collections::BTreeSet<_> = doc
        .ini_entries("Tubes")
        .into_iter()
        .map(|(_, key, _)| key)
        .collect();
    (0u32..=u32::MAX)
        .find(|n| !used.contains(&n.to_string()))
        .map(|n| n.to_string())
        .context(tr!("没有可用的隧道编号"))
}

impl TunnelEdit {
    pub(super) fn has_draft(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn begin_new(&mut self) {
        self.selected = None;
        self.draft = Some(TunnelDraft {
            replace: None,
            points: Vec::new(),
            direction: 2,
        });
    }

    pub(super) fn cancel_draft(&mut self) {
        self.draft = None;
    }

    fn begin_redraw(&mut self, entry: &TunnelRecord) {
        self.selected = Some(entry.target.clone());
        self.draft = Some(TunnelDraft {
            replace: Some(entry.target.clone()),
            points: Vec::new(),
            direction: exit_facing(entry).unwrap_or(2),
        });
    }

    pub(super) fn append_pointer(&mut self, doc: &MapDocument, at: Cell) -> Result<bool> {
        let Some(draft) = &mut self.draft else {
            return Ok(false);
        };
        doc.check_cell(at)
            .map_err(|_| anyhow::anyhow!(tr!("路径格子超出可用地图范围")))?;
        if draft.points.last() == Some(&at) {
            return Ok(false);
        }
        let segment = draft.points.last().map_or_else(
            || vec![at],
            |&last| cell_line(last, at).into_iter().skip(1).collect(),
        );
        for &cell in &segment {
            doc.check_cell(cell)
                .map_err(|_| anyhow::anyhow!(tr!("路径格子超出可用地图范围")))?;
        }
        ensure!(
            draft.points.len() + segment.len() <= MAX_PATH_STEPS + 1,
            "{}",
            tr!("隧道最多支持 {arg0} 个路径步骤", arg0 = MAX_PATH_STEPS)
        );
        draft.points.extend(segment);
        Ok(true)
    }

    pub(super) fn options(&mut self, ui: &mut egui::Ui, scene: &MapScene, editor: &mut Editor) {
        let entries = editor.doc.as_ref().map(rows).unwrap_or_default();
        ui.small(tr!(
            "编辑显式 [Tubes] 隧道路径。点击或拖动逐格绘制；入口是路径起点，出口由最后一步确定。"
        ));
        ui.small(tr!(
            "最多 99 步；八方向路径，支持点击后续格或拖动连续绘制。保存后可撤销。"
        ));
        let art_families = tunnel_art_families(scene);
        if art_families.is_empty() {
            ui.small(tr!(
                "当前环境没有完整的隧道入口地形组；保存时只写入路径数据。"
            ));
        } else {
            if self
                .art_family
                .is_none_or(|id| !art_families.iter().any(|family| family.group_id == id))
            {
                self.art_family = art_families.first().map(|family| family.group_id);
            }
            ui.checkbox(&mut self.auto_art, tr!("保存时自动放置两端隧道口素材"));
            ui.add_enabled_ui(self.auto_art, |ui| {
                egui::ComboBox::from_id_salt("tunnel-art-family")
                    .selected_text(
                        self.art_family
                            .and_then(|id| art_families.iter().find(|family| family.group_id == id))
                            .map_or_else(|| tr!("隧道口素材"), |family| family.name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for family in &art_families {
                            ui.selectable_value(
                                &mut self.art_family,
                                Some(family.group_id),
                                crate::labels::terrain(&family.name),
                            );
                        }
                    })
                    .response
                    .accessible_name(tr!("隧道口素材"));
            });
            ui.small(tr!(
                "重绘或删除会恢复编辑器备份的洞口底图；旧地图无底图备份时会恢复为清地形。"
            ));
        }

        let mut begin_new = false;
        let mut begin_redraw = false;
        let mut delete = false;
        let mut save = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr!("新建隧道")).clicked() {
                begin_new = true;
            }
            let selected = self
                .selected
                .as_ref()
                .and_then(|target| entries.iter().find(|entry| &entry.target == target));
            if ui
                .add_enabled(
                    selected.is_some_and(|entry| entry.error.is_none()),
                    egui::Button::new(tr!("重绘所选")),
                )
                .clicked()
            {
                begin_redraw = true;
            }
            if ui
                .add_enabled(selected.is_some(), egui::Button::new(tr!("删除所选")))
                .clicked()
            {
                delete = true;
            }
        });

        if entries.is_empty() {
            ui.label(tr!("此地图还没有显式隧道。"));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("tunnel-list")
                .max_height(170.)
                .show(ui, |ui| {
                    for entry in &entries {
                        let label = if let Some(error) = &entry.error {
                            tr!(
                                "隧道 {arg0} · 无法编辑：{arg1}",
                                arg0 = entry.key,
                                arg1 = error
                            )
                        } else {
                            tr!(
                                "隧道 {arg0} · ({arg1},{arg2}) → ({arg3},{arg4}) · {arg5} 步",
                                arg0 = entry.key,
                                arg1 = entry.entry[0],
                                arg2 = entry.entry[1],
                                arg3 = entry.exit[0],
                                arg4 = entry.exit[1],
                                arg5 = entry.steps.len()
                            )
                        };
                        if ui
                            .selectable_label(self.selected.as_ref() == Some(&entry.target), label)
                            .clicked()
                        {
                            self.selected = Some(entry.target.clone());
                        }
                    }
                });
        }

        let mut direction_update = None;
        if self.draft.is_none()
            && let Some(entry) = self
                .selected
                .as_ref()
                .and_then(|target| entries.iter().find(|entry| &entry.target == target))
            && exit_facing(entry).is_some()
        {
            let original_direction = exit_facing(entry).unwrap();
            let mut direction = original_direction;
            egui::ComboBox::from_id_salt("selected-tunnel-direction")
                .selected_text(tr!(
                    "所选隧道出口朝向：{arg0}",
                    arg0 = crate::i18n::text(DIRECTION_NAMES[usize::from(direction)])
                ))
                .show_ui(ui, |ui| {
                    for (index, name) in DIRECTION_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut direction, index as u8, crate::i18n::text(name));
                    }
                })
                .response
                .accessible_name(tr!(
                    "所选隧道出口朝向：{arg0}",
                    arg0 = crate::i18n::text(DIRECTION_NAMES[usize::from(direction)])
                ));
            if direction != original_direction {
                direction_update = Some((entry.target.clone(), direction));
            }
        }

        if begin_new {
            self.begin_new();
        }
        if begin_redraw
            && let Some(entry) = self
                .selected
                .as_ref()
                .and_then(|target| entries.iter().find(|entry| &entry.target == target))
        {
            self.begin_redraw(entry);
        }
        if delete {
            let result = (|| -> Result<()> {
                let target = self.selected.clone().context(tr!("请选择一条隧道"))?;
                let doc = editor.doc.as_mut().context(tr!("没有可编辑地图"))?;
                let tunnel = rows(doc)
                    .into_iter()
                    .find(|entry| entry.target == target)
                    .context(tr!("请选择一条隧道"))?;
                doc.atomic(|doc| {
                    restore_portal_backups(doc, scene, &tunnel)?;
                    doc.set_ini(PORTAL_FACING_SECTION, &tunnel.key, None)?;
                    doc.set(target, None)
                })?;
                editor.commit(scene, "删除隧道");
                Ok(())
            })();
            if let Err(error) = result {
                editor.status = format!("{error:#}");
            } else {
                self.selected = None;
                self.draft = None;
                editor.status = tr!("已删除隧道；可撤销。").into();
            }
        }

        if let Some((target, direction)) = direction_update {
            let result = self.change_direction(editor, scene, target, direction);
            if let Err(error) = result {
                editor.status = format!("{error:#}");
            }
        }
        if let Some(draft) = &mut self.draft {
            ui.separator();
            ui.label(if draft.replace.is_some() {
                tr!("重绘路径：重新绘制选中隧道；保存后替换原路径。")
            } else {
                tr!("新建路径：先点入口，再绘制路线。")
            });
            egui::ComboBox::from_id_salt("tunnel-direction")
                .selected_text(tr!(
                    "出口朝向：{arg0}",
                    arg0 = crate::i18n::text(DIRECTION_NAMES[usize::from(draft.direction)])
                ))
                .show_ui(ui, |ui| {
                    for (index, name) in DIRECTION_NAMES.iter().enumerate() {
                        ui.selectable_value(
                            &mut draft.direction,
                            index as u8,
                            crate::i18n::text(name),
                        );
                    }
                })
                .response
                .accessible_name(tr!(
                    "出口朝向：{arg0}",
                    arg0 = crate::i18n::text(DIRECTION_NAMES[usize::from(draft.direction)])
                ));
            ui.small(tr!(
                "出口朝向选择门洞素材和箭头方向；入口通行方向由路径起步自动确定。"
            ));
            ui.label(tr!("当前路径格数：{arg0} / 100", arg0 = draft.points.len()));
            if let Ok(steps) = draft_steps(&draft.points) {
                save = ui.button(tr!("保存隧道路径")).clicked();
                if save {
                    let _ = steps;
                }
            } else {
                ui.add_enabled(false, egui::Button::new(tr!("保存隧道路径")));
            }
            if ui.button(tr!("放弃当前路径")).clicked() {
                self.draft = None;
            }
        }

        if save {
            let result = self.save_draft(editor, scene);
            if let Err(error) = result {
                editor.status = format!("{error:#}");
            }
        }
    }

    fn save_draft(&mut self, editor: &mut Editor, scene: &MapScene) -> Result<()> {
        let draft = self
            .draft
            .as_ref()
            .context(tr!("没有待保存的隧道路径"))?
            .clone();
        self.apply_draft(editor, scene, draft, None)
    }

    fn change_direction(
        &mut self,
        editor: &mut Editor,
        scene: &MapScene,
        target: Target,
        direction: u8,
    ) -> Result<()> {
        let doc = editor.doc.as_ref().context(tr!("没有可编辑地图"))?;
        let value = doc.value(&target).context(tr!("请选择一条隧道"))?;
        let Value::Ini(bytes) = value else {
            anyhow::bail!("invalid tube target")
        };
        let tunnel = rows(doc)
            .into_iter()
            .find(|row| row.target == target)
            .context(tr!("请选择一条隧道"))?;
        // The record is attached to the ENTRY cell. Its direction gates
        // ordinary entry, so changing exit artwork must not rotate it.
        let encoded = bytes;
        let points = route(tunnel.entry, &tunnel.steps);
        // Imported records can have a path end different from their exit or a
        // zero-length automatic shell. Preserve those raw records verbatim.
        if self.auto_art
            && !tunnel_art_families(scene).is_empty()
            && tunnel.error.is_none()
            && points.len() >= 2
            && points.last() == Some(&tunnel.exit)
        {
            self.apply_draft(
                editor,
                scene,
                TunnelDraft {
                    replace: Some(target),
                    points,
                    direction,
                },
                Some(encoded),
            )
        } else {
            editor.doc.as_mut().unwrap().atomic(|doc| {
                doc.set(target, Some(Value::Ini(encoded)))?;
                doc.set_ini(
                    PORTAL_FACING_SECTION,
                    &tunnel.key,
                    Some(direction.to_string().into_bytes()),
                )
            })?;
            editor.commit(scene, "修改隧道出口朝向");
            editor.status = tr!("已更新隧道出口朝向；可撤销。").into();
            Ok(())
        }
    }

    fn apply_draft(
        &mut self,
        editor: &mut Editor,
        scene: &MapScene,
        draft: TunnelDraft,
        encoded: Option<Vec<u8>>,
    ) -> Result<()> {
        let direction_only = encoded.is_some();
        let replacing = draft.replace.is_some();
        let steps = draft_steps(&draft.points)?;
        let entry = *draft.points.first().context(tr!("请先绘制入口"))?;
        let exit = *draft.points.last().context(tr!("请先绘制出口"))?;
        ensure!(exit != [0, 0], "{}", tr!("隧道出口不能是地图坐标 0,0"));
        let source_doc = editor.doc.as_ref().context(tr!("没有可编辑地图"))?;
        let target = match draft.replace.clone() {
            Some(target) => target,
            None => Target::IniNew("Tubes".into(), next_key(source_doc)?),
        };
        let replaced_tunnel = rows(source_doc)
            .into_iter()
            .find(|row| row.target == target);
        ensure!(
            !rows(source_doc)
                .iter()
                .any(|row| row.target != target && row.entry == entry),
            "{}",
            tr!("入口格已有另一条隧道")
        );
        ensure!(
            !replacing || replaced_tunnel.is_some(),
            "{}",
            tr!("所选隧道已不存在，请重新选择")
        );
        let mut placement_doc = source_doc.fork();
        if let Some(tunnel) = &replaced_tunnel {
            restore_portal_backups(&mut placement_doc, scene, tunnel)?;
        }
        let art_families = tunnel_art_families(scene);
        let art_result = if self.auto_art {
            // Changing facing should retain the portal's existing road/dirt
            // family even if the panel currently defaults to another family.
            let existing_family = if direction_only {
                source_doc.terrain(entry).and_then(|terrain| {
                    art_families.iter().find(|family| {
                        [&family.x_axis, &family.y_axis].iter().any(|piece| {
                            portal_brush(scene, piece).is_ok_and(|brush| brush.raw == terrain.tile)
                        })
                    })
                })
            } else {
                None
            };
            existing_family
                .or_else(|| {
                    self.art_family.and_then(|group_id| {
                        art_families
                            .iter()
                            .find(|family| family.group_id == group_id)
                    })
                })
                .or_else(|| art_families.first())
                .map(|family| {
                    tunnel_portal_edits(
                        &placement_doc,
                        scene,
                        family,
                        &draft.points,
                        draft.direction,
                    )
                    .map_err(|error| error.to_string())
                })
        } else {
            None
        };
        let (art_edits, art_warning) = match art_result {
            Some(Ok(edits)) => (edits, None),
            Some(Err(error)) => (Vec::new(), Some(error)),
            None if self.auto_art => (Vec::new(), Some(tr!("当前环境没有隧道口素材").to_owned())),
            None => (Vec::new(), None),
        };
        if direction_only && let Some(warning) = &art_warning {
            anyhow::bail!("{warning}");
        }
        let art_backups: Vec<_> = art_edits
            .iter()
            .map(|(cell, placed)| {
                (
                    *cell,
                    ra2_map_scene::terrain_tools::value(&placement_doc, scene, *cell),
                    *placed,
                )
            })
            .collect();
        let encoded = encoded.unwrap_or_else(|| serialize(entry, exit, steps[0] as u8, &steps));
        let tunnel_key = match &target {
            Target::IniLine(_) => replaced_tunnel.as_ref().unwrap().key.clone(),
            Target::IniNew(_, key) => key.clone(),
            _ => unreachable!("tunnel targets are INI entries"),
        };
        let doc = editor.doc.as_mut().context(tr!("没有可编辑地图"))?;
        doc.atomic(|doc| {
            if let Some(tunnel) = &replaced_tunnel {
                restore_portal_backups(doc, scene, tunnel)?;
            }
            for &cell in &draft.points {
                doc.check_cell(cell)
                    .map_err(|_| anyhow::anyhow!(tr!("路径格子超出可用地图范围")))?;
            }
            doc.set(target.clone(), Some(Value::Ini(encoded)))?;
            doc.set_ini(
                PORTAL_FACING_SECTION,
                &tunnel_key,
                Some(draft.direction.to_string().into_bytes()),
            )?;
            for (cell, terrain) in &art_edits {
                doc.set(Target::Terrain(*cell), Some(Value::Terrain(*terrain)))?;
            }
            for (cell, terrain, placed) in &art_backups {
                let key = portal_backup_key(
                    match &target {
                        Target::IniLine(_) => replaced_tunnel
                            .as_ref()
                            .map_or("0", |tunnel| tunnel.key.as_str()),
                        Target::IniNew(_, key) => key,
                        _ => unreachable!("tunnel targets are INI entries"),
                    },
                    *cell,
                );
                doc.set_ini(
                    PORTAL_BACKUP_SECTION,
                    &key,
                    Some(serialize_portal_backup(*cell, *terrain, *placed)),
                )?;
            }
            Ok(())
        })?;
        editor.commit(
            scene,
            if direction_only {
                "修改隧道出口朝向"
            } else {
                "绘制隧道"
            },
        );
        self.selected = Some(target);
        self.draft = None;
        editor.status = if let Some(error) = art_warning {
            if replacing {
                tr!("已替换路径；未自动放置洞口素材：{arg0}", arg0 = error)
            } else {
                tr!("已保存路径；未自动放置洞口素材：{arg0}", arg0 = error)
            }
        } else if art_edits.is_empty() {
            if replacing {
                tr!("已替换所选隧道路径；可撤销。").into()
            } else {
                tr!("已保存隧道；可撤销。").into()
            }
        } else {
            if replacing {
                tr!("已替换隧道路径和两端入口素材；可整笔撤销。").into()
            } else {
                tr!("已保存隧道路径和两端入口素材；可整笔撤销。").into()
            }
        };
        Ok(())
    }

    pub(super) fn draw(
        &self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &MapCamera,
        editor: &Editor,
    ) {
        if !editor.tunnels_visible && editor.tool != Tool::Tunnel {
            return;
        }
        let Some(doc) = &editor.doc else {
            return;
        };
        let project = |at: Cell| {
            if editor.flat_view {
                let world = crate::flat_view::position(at);
                rect.min + (world - camera.world_rect(rect).min) * camera.zoom
            } else {
                let level = ra2_map_scene::terrain_tools::value(doc, scene, at).level;
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
                let world = egui::pos2(x + 30., y + 15.);
                rect.min + (world - camera.world_rect(rect).min) * camera.zoom
            }
        };
        let scale = camera.zoom.clamp(0.55, 1.6);
        let draw_facing = |at: Cell, direction: i32, color: Color32| {
            let direction = (direction as u32 & 7) as usize;
            let (dx, dy) = DIRECTIONS[direction];
            let Some(x) = u16::try_from(i32::from(at[0]) + dx).ok() else {
                return;
            };
            let Some(y) = u16::try_from(i32::from(at[1]) + dy).ok() else {
                return;
            };
            let start = project(at);
            let end = project([x, y]);
            let delta = end - start;
            let length = delta.length();
            if length < 0.01 {
                return;
            }
            let unit = delta / length;
            let side = egui::vec2(-unit.y, unit.x);
            let head = end - unit * (11. * scale);
            painter.line_segment([start, end], Stroke::new(2.5 * scale, color));
            painter.line_segment(
                [end, head + side * (6. * scale)],
                Stroke::new(2.5 * scale, color),
            );
            painter.line_segment(
                [end, head - side * (6. * scale)],
                Stroke::new(2.5 * scale, color),
            );
        };
        let replaced_target = self.draft.as_ref().and_then(|draft| draft.replace.as_ref());
        for entry in rows(doc) {
            if replaced_target == Some(&entry.target) {
                continue;
            }
            if entry.error.is_some() {
                continue;
            }
            let points = route(entry.entry, &entry.steps);
            let selected = self.selected.as_ref() == Some(&entry.target);
            let color = if selected {
                Color32::from_rgb(255, 242, 96)
            } else {
                Color32::from_rgb(80, 230, 230)
            };
            for pair in points.windows(2) {
                painter.line_segment(
                    [project(pair[0]), project(pair[1])],
                    Stroke::new(if selected { 3.0 } else { 2.0 } * scale, color),
                );
            }
            let entry_pos = project(entry.entry);
            let exit_pos = project(entry.exit);
            painter.circle_filled(entry_pos, 5.0 * scale, Color32::from_rgb(70, 245, 120));
            painter.circle_filled(exit_pos, 5.0 * scale, Color32::from_rgb(255, 135, 70));
            let exit_direction = exit_facing(&entry);
            if let Some(direction) = exit_direction {
                let facing_color = Color32::from_rgb(255, 125, 70);
                draw_facing(entry.exit, i32::from(direction), facing_color);
            }
            if camera.zoom >= 0.32 || selected {
                painter.text(
                    entry_pos + egui::vec2(6., -7.),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{} 入", entry.key),
                    egui::FontId::proportional(10. * scale),
                    color,
                );
                painter.text(
                    exit_pos + egui::vec2(6., 7.),
                    egui::Align2::LEFT_TOP,
                    exit_direction.map_or_else(
                        || format!("{} 出", entry.key),
                        |direction| {
                            format!(
                                "{} 出 · {}",
                                entry.key,
                                crate::i18n::text(DIRECTION_NAMES[usize::from(direction)])
                            )
                        },
                    ),
                    egui::FontId::proportional(10. * scale),
                    color,
                );
            }
        }
        if let Some(draft) = &self.draft {
            let color = Color32::YELLOW;
            for pair in draft.points.windows(2) {
                painter.line_segment(
                    [project(pair[0]), project(pair[1])],
                    Stroke::new(3.0 * scale, color),
                );
            }
            if let Some(&entry) = draft.points.first() {
                painter.circle_filled(project(entry), 6.0 * scale, Color32::LIGHT_GREEN);
            }
            if let Some(&exit) = draft.points.last() {
                painter.circle_filled(project(exit), 6.0 * scale, color);
                draw_facing(
                    exit,
                    i32::from(draft.direction),
                    Color32::from_rgb(255, 125, 70),
                );
            }
        }
    }
}

impl Editor {
    pub(super) fn tunnel_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        let mut state = std::mem::take(&mut self.tunnel_edit);
        state.options(ui, scene, self);
        self.tunnel_edit = state;
    }

    pub(crate) fn tunnel_preview(
        &self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &MapCamera,
    ) {
        self.tunnel_edit.draw(painter, scene, rect, camera, self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::tests::{editor, terrain_scene};
    use ra2_map_assets::{
        catalog::{TerrainBrush, TerrainSubtile, TerrainTemplate},
        terrain_library::TerrainGroup,
    };

    fn reopen(doc: &MapDocument) -> MapDocument {
        let (source, _) = ra2_map_document::source::SourceMap::parse(
            "reopened.map".into(),
            "test".into(),
            doc.snapshot().bytes().unwrap(),
        )
        .unwrap();
        MapDocument::new(source.edit_base.unwrap())
    }

    fn draw_test_tunnel(editor: &mut Editor, scene: &MapScene, points: Vec<Cell>) {
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.draft = Some(TunnelDraft {
            replace: None,
            points,
            direction: 2,
        });
        state.save_draft(editor, scene).unwrap();
        editor.tunnel_edit = state;
    }

    #[test]
    fn reopened_portal_backups_survive_repeated_overlapping_redraws() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        // Each subtile has a different original tile/height/ice, so merely
        // clearing the footprint cannot satisfy this regression.
        let doc = editor.doc.as_mut().unwrap();
        for x in 17..32 {
            for y in 17..30 {
                let original = TerrainValue {
                    tile: 10 + i32::from(x + y),
                    sub: 0,
                    level: 4,
                    ice: (x % 8) as u8,
                };
                doc.set(Target::Terrain([x, y]), Some(Value::Terrain(original)))
                    .unwrap();
            }
        }
        doc.commit("original terrain");
        let original = doc.fork();
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        for offset in [1, 0, 2] {
            editor.doc = Some(reopen(editor.doc.as_ref().unwrap()));
            let tunnel = rows(editor.doc.as_ref().unwrap()).remove(0);
            assert!(matches!(tunnel.target, Target::IniLine(_)));
            let mut state = std::mem::take(&mut editor.tunnel_edit);
            state.begin_redraw(&tunnel);
            state.draft.as_mut().unwrap().points = (20..=28).map(|x| [x, 20 + offset]).collect();
            state.save_draft(&mut editor, &scene).unwrap();
            editor.tunnel_edit = state;
            let doc = editor.doc.as_ref().unwrap();
            assert_eq!(rows(doc).len(), 1);
            let entries = doc.ini_entries(PORTAL_BACKUP_SECTION);
            assert_eq!(
                entries.len(),
                entries
                    .iter()
                    .map(|(_, key, _)| key)
                    .collect::<BTreeSet<_>>()
                    .len()
            );
        }
        let mut doc = reopen(editor.doc.as_ref().unwrap());
        let tunnel = rows(&doc).remove(0);
        restore_portal_backups(&mut doc, &scene, &tunnel).unwrap();
        for x in 17..32 {
            for y in 17..30 {
                assert_eq!(doc.terrain([x, y]), original.terrain([x, y]), "{x},{y}");
            }
        }
        assert!(doc.ini_entries(PORTAL_BACKUP_SECTION).is_empty());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sparse_and_legacy_absent_backups_restore_visible_ground_without_losing_manual_edits() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        // Expand the document fixture, which has only one terrain coordinate.
        let sparse = String::from_utf8_lossy(&ra2_map_document::test_support::fixture())
            .replace("0,0,3,3", "0,0,20,20")
            .into_bytes();
        let (source, _) =
            ra2_map_document::source::SourceMap::parse("sparse.map".into(), "test".into(), sparse)
                .unwrap();
        editor.doc = Some(MapDocument::new(source.edit_base.unwrap()));
        assert!(editor.doc.as_ref().unwrap().terrain([20, 20]).is_none());
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let mut doc = reopen(editor.doc.as_ref().unwrap());
        let tunnel = rows(&doc).remove(0);
        let manually_edited = TerrainValue {
            tile: 97,
            sub: 0,
            level: 8,
            ice: 5,
        };
        doc.set(
            Target::Terrain([20, 20]),
            Some(Value::Terrain(manually_edited)),
        )
        .unwrap();
        // Also exercise the previous three-field format persisted by older builds.
        doc.set_ini(
            PORTAL_BACKUP_SECTION,
            &portal_backup_key(&tunnel.key, [28, 20]),
            Some(b"28,20,-1".to_vec()),
        )
        .unwrap();
        restore_portal_backups(&mut doc, &scene, &tunnel).unwrap();
        assert_eq!(doc.terrain([20, 20]), Some(manually_edited));
        assert_eq!(doc.terrain([28, 20]), Some(clear_terrain(&scene)));
        assert!(doc.snapshot().bytes().is_ok());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_or_malformed_tubes_do_not_prevent_deletion() {
        let scene = terrain_scene();
        for bytes in [&b"20,20,2,20,20,-1"[..], &b"broken"[..]] {
            let (mut editor, _ctx, root) = editor();
            editor.attach(&scene);
            let doc = editor.doc.as_mut().unwrap();
            doc.set_ini("Tubes", "0", Some(bytes.to_vec())).unwrap();
            let tunnel = rows(doc).remove(0);
            doc.atomic(|doc| {
                restore_portal_backups(doc, &scene, &tunnel)?;
                doc.set(tunnel.target, None)
            })
            .unwrap();
            assert!(rows(doc).is_empty());
            drop(editor);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn redraw_button_and_canvas_replace_the_selected_physical_record() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _ctx, root) = editor();
        let scene = terrain_scene();
        editor.attach(&scene);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        editor.doc = Some(reopen(editor.doc.as_ref().unwrap()));
        editor.activate_tool(Tool::Tunnel);
        {
            let mut harness = crate::test_support::harness()
                .with_context_setup(|ctx| {
                    crate::ui_theme::install(ctx);
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                })
                .with_size(egui::vec2(650., 1300.))
                .build_ui(|ui| editor.tunnel_options(ui, &scene));
            harness
                .get_by_label("隧道 0 · (20,20) → (28,20) · 8 步")
                .click();
            harness.run();
            harness.get_by_label("重绘所选").click();
            harness.run();
        }
        assert!(editor.tunnel_edit.draft.as_ref().unwrap().replace.is_some());
        for at in [[20, 24], [28, 24]] {
            editor.pointer(&scene, Some(at), true, true, false, None);
            editor.pointer(&scene, Some(at), false, false, true, None);
        }
        {
            let mut harness = crate::test_support::harness()
                .with_context_setup(|ctx| {
                    crate::ui_theme::install(ctx);
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                })
                .with_size(egui::vec2(650., 1300.))
                .build_ui(|ui| editor.tunnel_options(ui, &scene));
            harness.get_by_label("保存隧道路径").click();
            harness.run();
        }
        let doc = reopen(editor.doc.as_ref().unwrap());
        let tunnels = rows(&doc);
        assert_eq!(tunnels.len(), 1);
        assert_eq!(tunnels[0].entry, [20, 24]);
        assert_eq!(tunnels[0].exit, [28, 24]);
        assert_eq!(tunnels[0].key, "0");
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direction_change_updates_art_preserves_raw_tail_and_undoes_together() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        let original_ground = editor.doc.as_ref().unwrap().fork();
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let doc = editor.doc.as_mut().unwrap();
        let before_tunnel = rows(doc).remove(0);
        let mut bytes = doc.ini_value("Tubes", "0").unwrap();
        bytes.extend_from_slice(b",opaque");
        doc.set(
            before_tunnel.target.clone(),
            Some(Value::Ini(bytes.clone())),
        )
        .unwrap();
        doc.commit("tail");
        let before = doc.snapshot().bytes().unwrap();
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state
            .change_direction(&mut editor, &scene, before_tunnel.target, 6)
            .unwrap();
        editor.tunnel_edit = state;
        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(doc.ini_value("Tubes", "0").unwrap(), bytes);
        assert_eq!(doc.terrain([28, 20]).unwrap().tile, 2200);
        assert_eq!(doc.terrain([28, 18]), original_ground.terrain([28, 18]));
        let after = doc.snapshot().bytes().unwrap();
        editor.history(false, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        editor.history(true, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exit_art_facing_never_changes_entry_admission_and_survives_reopen() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let original = editor
            .doc
            .as_ref()
            .unwrap()
            .ini_value("Tubes", "0")
            .unwrap();
        for direction in 0..8 {
            editor.doc = Some(reopen(editor.doc.as_ref().unwrap()));
            let tunnel = rows(editor.doc.as_ref().unwrap()).remove(0);
            let mut state = TunnelEdit::default();
            state
                .change_direction(&mut editor, &scene, tunnel.target, direction)
                .unwrap();
            let doc = editor.doc.as_ref().unwrap();
            assert_eq!(doc.ini_value("Tubes", "0").unwrap(), original);
            assert_eq!(rows(&reopen(doc))[0].exit_facing, Some(direction));
            for endpoint in [[20, 20], [28, 20]] {
                let terrain = doc.terrain(endpoint).unwrap();
                let tile_id = scene
                    .brushes
                    .iter()
                    .find(|brush| brush.raw == terrain.tile)
                    .unwrap()
                    .key
                    .tile_id;
                let cell =
                    scene.palette.templates[&(tile_id, 0)].cells[usize::from(terrain.sub)].unwrap();
                assert_eq!(
                    cell.terrain, 11,
                    "endpoint must be on the road, not the wall"
                );
            }
        }
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlapping_portals_do_not_steal_another_tunnels_ground_backups() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let doc = editor.doc.as_ref().unwrap();
        let old_backup = doc.ini_entries(PORTAL_BACKUP_SECTION);
        let portal = doc.terrain([20, 20]);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 21]).collect());
        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(rows(doc).len(), 2);
        assert_eq!(doc.ini_entries(PORTAL_BACKUP_SECTION), old_backup);
        assert_eq!(doc.terrain([20, 20]), portal);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn portal_collision_checks_building_foundations_and_direction_failures_are_atomic() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let doc = editor.doc.as_mut().unwrap();
        // This cell is in the new 4x3 exit, outside its old 3x4 footprint.
        doc.set(Target::Overlay([26, 21]), Some(Value::Overlay(1, 0)))
            .unwrap();
        doc.commit("obstacle");
        let tunnel = rows(doc).remove(0);
        let before = doc.snapshot().bytes().unwrap();
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        assert!(
            state
                .change_direction(&mut editor, &scene, tunnel.target.clone(), 6)
                .is_err()
        );
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        state.auto_art = false;
        state
            .change_direction(&mut editor, &scene, tunnel.target, 6)
            .unwrap();
        assert_eq!(
            editor.doc.as_ref().unwrap().terrain([28, 20]).unwrap().tile,
            2201
        );
        editor.tunnel_edit = state;

        std::sync::Arc::make_mut(&mut scene.catalog).art =
            std::sync::Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(
                &vera20k_core::rules::ini_parser::IniFile::from_str("[LARGE]\nFoundation=3x3\n"),
            ));
        editor
            .doc
            .as_mut()
            .unwrap()
            .add_record("Structures", [16, 25], "Neutral", "LARGE")
            .unwrap();
        let family = tunnel_art_families(&scene).remove(0);
        let points: Vec<_> = (20..=28).map(|x| [x, 26]).collect();
        assert!(
            tunnel_portal_edits(editor.doc.as_ref().unwrap(), &scene, &family, &points, 2).is_err()
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn release_includes_final_cell_and_history_cancels_stale_redraws() {
        let (mut editor, _ctx, root) = editor();
        let scene = terrain_scene();
        editor.attach(&scene);
        editor.activate_tool(Tool::Tunnel);
        editor.pointer(&scene, Some([20, 20]), true, true, false, None);
        editor.pointer(&scene, Some([28, 20]), false, false, true, None);
        assert_eq!(editor.tunnel_edit.draft.as_ref().unwrap().points.len(), 9);
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.save_draft(&mut editor, &scene).unwrap();
        let existing = rows(editor.doc.as_ref().unwrap()).remove(0);
        state.begin_redraw(&existing);
        editor.tunnel_edit = state;
        editor.history(false, &scene);
        assert!(!editor.tunnel_edit.has_draft());
        assert!(rows(editor.doc.as_ref().unwrap()).is_empty());
        // A retained stale command must not silently resurrect an undone row.
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.begin_redraw(&existing);
        state.draft.as_mut().unwrap().points = (20..=28).map(|x| [x, 24]).collect();
        assert!(state.save_draft(&mut editor, &scene).is_err());
        assert!(rows(editor.doc.as_ref().unwrap()).is_empty());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_cleanup_matches_subtiles_and_both_portal_orientations() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        editor.attach(&scene);
        draw_test_tunnel(&mut editor, &scene, (20..=28).map(|x| [x, 20]).collect());
        let doc = editor.doc.as_mut().unwrap();
        for (target, _, _) in doc.ini_entries(PORTAL_BACKUP_SECTION) {
            doc.set(target, None).unwrap();
        }
        // Old UI changed only the raw direction, leaving the other TMP behind.
        doc.set_ini(
            "Tubes",
            "0",
            Some(b"20,20,2,28,20,2,2,2,2,2,2,2,2,-1".to_vec()),
        )
        .unwrap();
        let tunnel = rows(doc).remove(0);
        restore_portal_backups(doc, &scene, &tunnel).unwrap();
        assert_eq!(
            doc.terrain([20, 20]).unwrap().tile,
            clear_terrain(&scene).tile
        );
        assert_eq!(
            doc.terrain([28, 20]).unwrap().tile,
            clear_terrain(&scene).tile
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn add_tunnel_art(scene: &mut MapScene) {
        let image = scene.palette.images.values().next().unwrap().clone();
        let palette = std::sync::Arc::get_mut(&mut scene.palette).unwrap();
        let catalog = std::sync::Arc::get_mut(&mut scene.catalog).unwrap();
        for (tile_id, width, height, horizontal) in [(600, 4, 3, true), (601, 3, 4, false)] {
            let cells = (0..width * height)
                .map(|sub| {
                    let col = sub % width;
                    let row = sub / width;
                    let tunnel_mouth = if horizontal {
                        row == 0 && col > 0
                    } else {
                        col == 0 && row > 0
                    };
                    Some(TerrainSubtile {
                        height: 0,
                        slope: 0,
                        terrain: if tunnel_mouth {
                            if horizontal { 7 } else { 6 }
                        } else {
                            11
                        },
                    })
                })
                .collect();
            palette.templates.insert(
                (tile_id, 0),
                TerrainTemplate {
                    width,
                    height,
                    cells,
                },
            );
            palette.groups.insert(
                tile_id,
                TerrainGroup {
                    id: 42,
                    name: "Tunnels".into(),
                    category: TerrainCategory::Tunnel,
                },
            );
            let raw = 1600 + i32::from(tile_id);
            catalog.raw_tiles.insert(tile_id, raw);
            for sub_tile in 0..width * height {
                let key = vera20k_core::map::theater::TileKey {
                    tile_id,
                    sub_tile: sub_tile as u8,
                    variant: 0,
                };
                palette.images.insert(key, image.clone());
                scene.brushes.push(TerrainBrush {
                    key,
                    raw,
                    label: format!("Tunnels · {tile_id}:{sub_tile}"),
                });
            }
        }
    }

    #[test]
    fn route_steps_use_native_eight_way_order_and_round_trip_ini_shape() {
        let points = [[20, 20], [21, 19], [22, 19], [22, 20]];
        let steps = draft_steps(&points).unwrap();
        assert_eq!(steps, [1, 2, 4]);
        let bytes = serialize(points[0], points[3], 3, &steps);
        let parsed = parse_record(
            Target::IniNew("Tubes".into(), "0".into()),
            "0".into(),
            &bytes,
        );
        assert_eq!(parsed.error, None);
        assert_eq!(parsed.entry, points[0]);
        assert_eq!(parsed.exit, points[3]);
        assert_eq!(route(parsed.entry, &parsed.steps), points);
        for direction in 0..8 {
            let raw_direction = direction;
            let parsed = parse_record(
                Target::IniNew("Tubes".into(), direction.to_string()),
                direction.to_string(),
                &serialize(points[0], points[3], raw_direction, &steps),
            );
            assert_eq!(parsed.error, None);
            assert_eq!(parsed.direction, i32::from(raw_direction));
        }
    }

    #[test]
    fn portal_backup_keys_identify_their_tunnel_owner() {
        assert_eq!(portal_backup_owner("T0_20_20"), Some("0"));
        assert_eq!(portal_backup_owner("T0_other_20_20"), Some("0_other"));
    }

    #[test]
    fn authored_tunnel_is_undoable_redrawable_and_deleted_as_an_ini_record() {
        let (mut editor, _ctx, root) = editor();
        let scene = terrain_scene();
        editor.attach(&scene);
        editor.activate_tool(Tool::Tunnel);
        editor.pointer(&scene, Some([20, 20]), true, true, false, None);
        editor.pointer(&scene, Some([22, 20]), false, true, false, None);
        editor.pointer(&scene, Some([22, 20]), false, false, true, None);
        editor.tunnel_edit.draft.as_mut().unwrap().direction = 2;
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.save_draft(&mut editor, &scene).unwrap();
        editor.tunnel_edit = state;
        let entries = rows(editor.doc.as_ref().unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry, [20, 20]);
        assert_eq!(entries[0].exit, [22, 20]);
        assert_eq!(entries[0].steps, [2, 2]);
        let serialized = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let raw = ra2_map_document::raw_ini::RawIni::parse(serialized.into()).unwrap();
        let (line, _) = raw.entries("Tubes").next().unwrap();
        assert_eq!(raw.value(line), b"20,20,2,22,20,2,2,-1");

        editor.history(false, &scene);
        assert!(rows(editor.doc.as_ref().unwrap()).is_empty());
        editor.history(true, &scene);
        assert_eq!(rows(editor.doc.as_ref().unwrap()).len(), 1);
        let target = rows(editor.doc.as_ref().unwrap())[0].target.clone();
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        let existing = rows(editor.doc.as_ref().unwrap()).remove(0);
        state.begin_redraw(&existing);
        assert_eq!(state.draft.as_ref().unwrap().replace, Some(target.clone()));
        for at in [[22, 20], [22, 21]] {
            state
                .append_pointer(editor.doc.as_ref().unwrap(), at)
                .unwrap();
        }
        state.save_draft(&mut editor, &scene).unwrap();
        editor.tunnel_edit = state;
        let redrawn = rows(editor.doc.as_ref().unwrap());
        assert_eq!(redrawn.len(), 1);
        assert_eq!(redrawn[0].target, target);
        assert_eq!(redrawn[0].entry, [22, 20]);
        assert_eq!(redrawn[0].exit, [22, 21]);
        assert_eq!(redrawn[0].direction, 4);
        assert_eq!(redrawn[0].steps, [4]);
        editor.history(false, &scene);
        assert_eq!(rows(editor.doc.as_ref().unwrap())[0].entry, [20, 20]);
        editor.history(true, &scene);
        assert_eq!(rows(editor.doc.as_ref().unwrap())[0].entry, [22, 20]);
        editor.doc.as_mut().unwrap().set(target, None).unwrap();
        editor.commit(&scene, "删除隧道");
        assert!(rows(editor.doc.as_ref().unwrap()).is_empty());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tunnel_assets_are_added_for_a_declared_tunnels_group_and_undo_with_the_route() {
        let (mut editor, _ctx, root) = editor();
        let mut scene = terrain_scene();
        add_tunnel_art(&mut scene);
        let families = tunnel_art_families(&scene);
        assert_eq!(families.len(), 1);
        assert_eq!(families[0].name, "Tunnels");
        assert_eq!(families[0].x_axis.anchor_subtile, 6);
        assert_eq!(families[0].y_axis.anchor_subtile, 7);
        assert_eq!(portal_piece_for_facing(&families[0], 0).tile_id, 601);
        assert_eq!(portal_piece_for_facing(&families[0], 2).tile_id, 601);
        assert_eq!(portal_piece_for_facing(&families[0], 4).tile_id, 600);
        assert_eq!(portal_piece_for_facing(&families[0], 6).tile_id, 600);
        editor.attach(&scene);
        let before_entry = editor.doc.as_ref().unwrap().terrain([20, 20]);
        let before_exit = editor.doc.as_ref().unwrap().terrain([28, 20]);
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.art_family = Some(42);
        state.draft = Some(TunnelDraft {
            replace: None,
            points: (20..=28).map(|x| [x, 20]).collect(),
            direction: 2,
        });
        state.save_draft(&mut editor, &scene).unwrap();
        editor.tunnel_edit = state;

        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(rows(doc).len(), 1);
        assert_eq!(doc.terrain([20, 20]).unwrap().tile, 2200);
        assert_eq!(doc.terrain([28, 20]).unwrap().tile, 2201);
        assert!(doc.terrain([18, 20]).is_some());
        assert!(!doc.ini_entries(PORTAL_BACKUP_SECTION).is_empty());

        let old_tunnel = rows(doc).remove(0);
        let before_redraw_entry = doc.terrain([20, 26]);
        let before_redraw_exit = doc.terrain([28, 26]);
        let mut state = std::mem::take(&mut editor.tunnel_edit);
        state.begin_redraw(&old_tunnel);
        for at in [[20, 26], [28, 26]] {
            state
                .append_pointer(editor.doc.as_ref().unwrap(), at)
                .unwrap();
        }
        state.draft.as_mut().unwrap().direction = 2;
        state.save_draft(&mut editor, &scene).unwrap();
        editor.tunnel_edit = state;

        let doc = editor.doc.as_ref().unwrap();
        let redrawn = rows(doc);
        assert_eq!(redrawn.len(), 1);
        assert_eq!(redrawn[0].target, old_tunnel.target);
        assert_eq!(redrawn[0].entry, [20, 26]);
        assert_eq!(redrawn[0].exit, [28, 26]);
        assert_eq!(doc.terrain([20, 20]), before_entry);
        assert_eq!(doc.terrain([28, 20]), before_exit);
        assert_eq!(doc.terrain([20, 26]).unwrap().tile, 2200);
        assert_eq!(doc.terrain([28, 26]).unwrap().tile, 2201);

        editor.history(false, &scene);
        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(rows(doc)[0].entry, [20, 20]);
        assert_eq!(rows(doc)[0].exit, [28, 20]);
        assert_eq!(doc.terrain([20, 20]).unwrap().tile, 2200);
        assert_eq!(doc.terrain([28, 20]).unwrap().tile, 2201);
        assert_eq!(doc.terrain([20, 26]), before_redraw_entry);
        assert_eq!(doc.terrain([28, 26]), before_redraw_exit);
        let tunnel = rows(doc).remove(0);
        let doc = editor.doc.as_mut().unwrap();
        doc.atomic(|doc| {
            restore_portal_backups(doc, &scene, &tunnel)?;
            doc.set(tunnel.target.clone(), None)
        })
        .unwrap();
        editor.commit(&scene, "删除隧道");
        let doc = editor.doc.as_ref().unwrap();
        assert!(rows(doc).is_empty());
        assert!(doc.ini_entries(PORTAL_BACKUP_SECTION).is_empty());
        assert_eq!(doc.terrain([20, 20]), before_entry);
        assert_eq!(doc.terrain([28, 20]), before_exit);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "tunnel_edit_gameplay_tests.rs"]
mod gameplay_tests;
