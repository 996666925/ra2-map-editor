//! Terrain authoring commands. Work from cached geometry and write only through
//! MapDocument, leaving commit/cancel ownership to the caller's stroke transaction.
use crate::map::MapScene;
use anyhow::{Context, Result, ensure};
use ra2_map_assets::catalog::{TerrainBrush, TerrainTemplate};
use ra2_map_document::document::{Cell, MapDocument, Target, TerrainValue, Value};
use std::collections::{BTreeSet, HashMap, VecDeque};
use vera20k_core::map::{
    lat::{LatConfig, SlopeFixupConfig, apply_load_recalc_sweeps},
    map_file::MapCell,
    rmg::{RmgGrid, RmgScratch, phases::hills_corners::CornerGrid},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainMode {
    #[default]
    Subtile,
    Template,
    Fill,
    Raise,
    Lower,
    Flatten,
    Plateau,
    CliffOutline,
    CliffFront,
    CliffBack,
    PlateauCut,
    RampEntrance,
    Bridge,
    Water,
    WaterOutline,
    LandFill,
    ShoreRepair,
    Road,
    RoadCut,
    CoastalCliff,
}
impl TerrainMode {
    pub const ALL: [Self; 20] = [
        Self::Subtile,
        Self::Template,
        Self::Fill,
        Self::Raise,
        Self::Lower,
        Self::Flatten,
        Self::Plateau,
        Self::CliffOutline,
        Self::CliffFront,
        Self::CliffBack,
        Self::PlateauCut,
        Self::RampEntrance,
        Self::Bridge,
        Self::Water,
        Self::WaterOutline,
        Self::LandFill,
        Self::ShoreRepair,
        Self::Road,
        Self::RoadCut,
        Self::CoastalCliff,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Subtile => "子块画笔",
            Self::Template => "完整模板",
            Self::Fill => "区域填充",
            Self::Raise => "升高",
            Self::Lower => "降低",
            Self::Flatten => "平整",
            Self::Plateau => "矩形高台（自动悬崖）",
            Self::CliffOutline => "围合高台（自动拐角）",
            Self::CliffFront => "悬崖线段（正面）",
            Self::CliffBack => "悬崖线段（背面）",
            Self::PlateauCut => "切除高台（矩形）",
            Self::RampEntrance => "插入高台坡道",
            Self::Bridge => "桥梁（拖动连接两端）",
            Self::Water => "矩形水域（自动岸线）",
            Self::WaterOutline => "围合水域（自动岸线）",
            Self::LandFill => "回填陆地（修补岸线）",
            Self::ShoreRepair => "修复岸线（矩形）",
            Self::Road => "道路（自动转角 / 路口）",
            Self::RoadCut => "切除道路（修补端点）",
            Self::CoastalCliff => "匹配临水悬崖（矩形）",
        }
    }
    pub fn edits_height(self) -> bool {
        matches!(self, Self::Raise | Self::Lower | Self::Flatten)
    }
    pub fn planned(self) -> bool {
        self.joining()
            || matches!(
                self,
                Self::Plateau
                    | Self::CliffOutline
                    | Self::CliffFront
                    | Self::CliffBack
                    | Self::PlateauCut
                    | Self::RampEntrance
                    | Self::Bridge
            )
    }
    pub fn joining(self) -> bool {
        matches!(
            self,
            Self::Water
                | Self::WaterOutline
                | Self::LandFill
                | Self::ShoreRepair
                | Self::Road
                | Self::RoadCut
                | Self::CoastalCliff
        )
    }
}

pub fn value(doc: &MapDocument, scene: &MapScene, at: Cell) -> TerrainValue {
    doc.terrain(at).unwrap_or(TerrainValue {
        tile: scene
            .palette
            .tile_ids
            .map_or(0, |ids| raw(scene, ids.clear).unwrap_or(0)),
        sub: 0,
        level: scene.source.header.level as u8,
        ice: 0,
    })
}

/// Exact floor at an authored object's cell centre, including preview terrain.
pub fn object_world_z(scene: &MapScene, at: Cell, value: TerrainValue, on_bridge: bool) -> i32 {
    let tile = scene
        .catalog
        .raw_tiles
        .iter()
        .find_map(|(&tile, &raw)| (raw == value.tile).then_some(tile))
        .or_else(|| u16::try_from(value.tile).ok());
    let slope = tile
        .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
        .and_then(|t| t.cells.get(usize::from(value.sub)))
        .and_then(|s| *s)
        .map_or(0, |s| s.slope);
    vera20k_core::util::lepton::ground_height_leptons(
        value.level,
        slope,
        i32::from(at[0]) * 256 + 128,
        i32::from(at[1]) * 256 + 128,
    )
    .unwrap_or(i32::from(value.level) * 104)
        + if on_bridge { 416 } else { 0 }
}

fn raw(scene: &MapScene, tile: i32) -> Option<i32> {
    scene
        .catalog
        .raw_tiles
        .get(&u16::try_from(tile).ok()?)
        .copied()
}

fn translated(scene: &MapScene) -> HashMap<i32, i32> {
    scene
        .catalog
        .raw_tiles
        .iter()
        .map(|(&tile, &raw)| (raw, i32::from(tile)))
        .collect()
}

fn tile_id(ids: &HashMap<i32, i32>, v: TerrainValue) -> i32 {
    ids.get(&v.tile).copied().unwrap_or(v.tile)
}

fn template<'a>(scene: &'a MapScene, brush: &TerrainBrush) -> Result<&'a TerrainTemplate> {
    scene
        .palette
        .templates
        .get(&(brush.key.tile_id, brush.key.variant))
        .context("模板几何不可用")
}

pub fn footprint(scene: &MapScene, brush: &TerrainBrush, at: Cell) -> Result<Vec<(Cell, u8, u8)>> {
    let template = template(scene, brush)?;
    ensure!(template.width > 0, "模板宽度无效");
    ensure!(
        template
            .cells
            .get(usize::from(brush.key.sub_tile))
            .is_some_and(Option::is_some),
        "所选子块为空"
    );
    let anchor = [
        i32::from(at[0]) - i32::from(brush.key.sub_tile) % i32::from(template.width),
        i32::from(at[1]) - i32::from(brush.key.sub_tile) / i32::from(template.width),
    ];
    template
        .cells
        .iter()
        .enumerate()
        .filter_map(|(sub, metadata)| metadata.map(|metadata| (sub, metadata)))
        .map(|(sub, metadata)| {
            let x = anchor[0] + sub as i32 % i32::from(template.width);
            let y = anchor[1] + sub as i32 / i32::from(template.width);
            ensure!(
                (0..512).contains(&x) && (0..512).contains(&y),
                "完整模板越过地图边界"
            );
            Ok(([x as u16, y as u16], sub as u8, metadata.height))
        })
        .collect()
}

/// Keep the clicked subtile under the cursor; TMP height bytes are relative to
/// the shared template base level. Validate the whole placement before writing.
pub fn stamp(
    doc: &mut MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    whole: bool,
) -> Result<Vec<Cell>> {
    stamp_at_height(doc, scene, brush, at, whole, None)
}

/// Shared preflight for both the canvas ghost and the committed template.
/// An explicit level refers to the template base, not the selected anchor.
pub fn template_placement(
    doc: &MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    base_level: Option<u8>,
) -> Result<Vec<(Cell, TerrainValue)>> {
    doc.check_cell(at)?;
    let t = template(scene, brush)?;
    let selected_height = t
        .cells
        .get(usize::from(brush.key.sub_tile))
        .and_then(|cell| *cell)
        .context("所选子块为空")?
        .height;
    let base = base_level.map_or_else(
        || i16::from(value(doc, scene, at).level) - i16::from(selected_height),
        i16::from,
    );
    // Map format records no visual variant. Geometry must match the
    // pristine template which the engine uses to interpret saved subtiles.
    let pristine_template = scene.palette.templates.get(&(brush.key.tile_id, 0));
    ensure!(
        pristine_template.is_some_and(|p| p.width == t.width
            && p.height == t.height
            && p.cells.len() == t.cells.len()
            && p.cells
                .iter()
                .zip(&t.cells)
                .all(|(a, b)| a.is_some() == b.is_some())),
        "此变体的几何与基础模板不同，不能保存为完整地形"
    );
    let mut edits = Vec::new();
    for (cell, sub, height) in footprint(scene, brush, at)? {
        doc.check_cell(cell)?;
        let key = vera20k_core::map::theater::TileKey {
            sub_tile: sub,
            ..brush.key
        };
        ensure!(
            scene.palette.images.contains_key(&key),
            "模板子块资源缺失，未修改地图"
        );
        let pristine = pristine_template
            .and_then(|t| t.cells.get(usize::from(sub)))
            .and_then(|s| *s);
        ensure!(
            pristine.is_some_and(
                |s| s.height == height && s.slope == t.cells[usize::from(sub)].unwrap().slope
            ),
            "此变体的几何与基础模板不同，不能保存为完整地形"
        );
        let level = base + i16::from(height);
        ensure!(
            (0..=14).contains(&level),
            "模板高度越过 0～14，请调整基准高度或放置位置"
        );
        edits.push((
            cell,
            TerrainValue {
                tile: brush.raw,
                sub,
                level: level as u8,
                ..value(doc, scene, cell)
            },
        ));
    }
    Ok(edits)
}

pub fn stamp_at_height(
    doc: &mut MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    at: Cell,
    whole: bool,
    base_level: Option<u8>,
) -> Result<Vec<Cell>> {
    let mut edits = Vec::new();
    if whole {
        edits = template_placement(doc, scene, brush, at, base_level)?;
    } else {
        edits.push((
            at,
            TerrainValue {
                tile: brush.raw,
                sub: brush.key.sub_tile,
                ..value(doc, scene, at)
            },
        ));
    }
    apply(doc, edits)
}

fn apply(doc: &mut MapDocument, edits: Vec<(Cell, TerrainValue)>) -> Result<Vec<Cell>> {
    for (at, _) in &edits {
        doc.check_cell(*at)?;
    }
    let mut changed = Vec::new();
    for (at, terrain) in edits {
        if doc.terrain(at) != Some(terrain) {
            doc.set(Target::Terrain(at), Some(Value::Terrain(terrain)))?;
            changed.push(at);
        }
    }
    Ok(changed)
}

fn ground_family(tile: i32, scene: &MapScene) -> i32 {
    if let Some(ids) = scene.palette.tile_ids {
        for (base, lat) in [
            (ids.rough, ids.rough_lat),
            (ids.sand, ids.sand_lat),
            (ids.green, ids.green_lat),
            (ids.pave, ids.pave_lat),
        ] {
            if base >= 0 && (tile == base || lat >= 0 && (lat..lat + 16).contains(&tile)) {
                return base;
            }
        }
    }
    tile
}

/// Iterative four-neighbour bucket fill, bounded by Size and original height.
/// LAT edge variants belong to the same logical ground as their base tile.
pub fn fill(
    doc: &mut MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    start: Cell,
) -> Result<Vec<Cell>> {
    doc.check_cell(start)?;
    let t = template(scene, brush)?;
    ensure!(t.width == 1 && t.height == 1, "区域填充请选择 1×1 地形模板");
    let ids = translated(scene);
    let initial = value(doc, scene, start);
    let family = ground_family(tile_id(&ids, initial), scene);
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    let mut edits = Vec::new();
    while let Some(at) = queue.pop_front() {
        if !visited.insert(at) || doc.check_cell(at).is_err() {
            continue;
        }
        let old = value(doc, scene, at);
        if old.level != initial.level
            || old.sub != initial.sub
            || ground_family(tile_id(&ids, old), scene) != family
        {
            continue;
        }
        edits.push((
            at,
            TerrainValue {
                tile: brush.raw,
                sub: brush.key.sub_tile,
                ..old
            },
        ));
        queue.extend(neighbors(at));
    }
    apply(doc, edits)
}

pub fn neighbors(at: Cell) -> impl Iterator<Item = Cell> {
    [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
            ((0..512).contains(&x) && (0..512).contains(&y)).then_some([x as u16, y as u16])
        })
}

/// Reuse the engine's two LAT/slope sweeps with a padded read context. Only
/// the stroke and its two-cell halo are persisted; distant map art stays intact.
pub fn recalculate(
    doc: &mut MapDocument,
    scene: &MapScene,
    touched: &BTreeSet<Cell>,
) -> Result<Vec<Cell>> {
    let ids = scene.palette.tile_ids.context("当前环境缺少地形连接配置")?;
    let mut write = touched.clone();
    for _ in 0..2 {
        write.extend(write.clone().into_iter().flat_map(neighbors));
    }
    let mut read = write.clone();
    for _ in 0..3 {
        read.extend(read.clone().into_iter().flat_map(neighbors));
    }
    let translated = translated(scene);
    let mut cells: Vec<_> = read
        .into_iter()
        .filter(|at| doc.check_cell(*at).is_ok())
        .map(|at| {
            let v = value(doc, scene, at);
            MapCell {
                rx: at[0],
                ry: at[1],
                tile_index: tile_id(&translated, v),
                sub_tile: v.sub,
                z: v.level,
            }
        })
        .collect();
    // Match the native cell iterator's stable sum/x order.
    cells.sort_by_key(|c| (u32::from(c.rx) + u32::from(c.ry), c.rx));
    apply_load_recalc_sweeps(
        &mut cells,
        &LatConfig::from_tile_ids(&ids),
        SlopeFixupConfig::from_tile_ids(&ids),
        &mut |tile, sub| {
            u16::try_from(tile)
                .ok()
                .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
                .and_then(|t| t.cells.get(usize::from(sub)))
                .and_then(|s| *s)
                .map_or(0, |s| s.slope)
        },
    );
    let edits = cells
        .into_iter()
        .filter(|c| write.contains(&[c.rx, c.ry]))
        .filter_map(|c| {
            let key = vera20k_core::map::theater::TileKey {
                tile_id: u16::try_from(c.tile_index).ok()?,
                sub_tile: c.sub_tile,
                variant: 0,
            };
            scene.palette.images.contains_key(&key).then_some(())?;
            let at = [c.rx, c.ry];
            Some((
                at,
                TerrainValue {
                    tile: raw(scene, c.tile_index)?,
                    ..value(doc, scene, at)
                },
            ))
        })
        .collect();
    apply(doc, edits)
}

/// Height editing uses the engine's corner grid and slope propagation. The
/// native morph engine supports 0..12; explicit raw-level mode supports 0..14.
pub fn height(
    doc: &mut MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    mode: TerrainMode,
    level: u8,
    smooth: bool,
) -> Result<Vec<Cell>> {
    ensure!(mode.edits_height(), "不是高度工具");
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let target = |old: u8| match mode {
        TerrainMode::Raise => old.saturating_add(1),
        TerrainMode::Lower => old.saturating_sub(1),
        _ => level,
    };
    if !smooth {
        let edits = selected
            .iter()
            .map(|&at| {
                let old = value(doc, scene, at);
                (
                    at,
                    TerrainValue {
                        level: target(old.level).min(14),
                        ..old
                    },
                )
            })
            .collect();
        return apply(doc, edits);
    }
    let ids = scene.palette.tile_ids.context("当前环境没有坡面配置")?;
    ensure!(
        ids.clear >= 0 && ids.ramp_base >= 0,
        "当前环境没有可用的坡面模板；可关闭自动坡面直接编辑高度"
    );
    let span = (doc.base.header.width + doc.base.header.height) as usize;
    let min = doc.base.header.width as i32;
    let max = min + 2 * doc.base.header.height as i32;
    let mut grid = RmgGrid::new(span, min, max);
    let scratch = RmgScratch::new(span, min, max);
    let translated = translated(scene);
    for (x, y) in grid.native_cells().collect::<Vec<_>>() {
        let old = value(doc, scene, [x as u16, y as u16]);
        let tile = tile_id(&translated, old);
        let c = grid.get_mut(x, y).context("坡面格网无效")?;
        c.tile = tile;
        c.sub_tile = old.sub;
        c.level = old.level;
        c.slope = u16::try_from(tile)
            .ok()
            .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
            .and_then(|t| t.cells.get(usize::from(old.sub)))
            .and_then(|s| *s)
            .map_or(0, |s| s.slope);
        let overlay = doc.overlay([x as u16, y as u16]);
        c.overlay = if overlay.0 == 255 {
            -1
        } else {
            i32::from(overlay.0)
        };
        c.occupied = old.level > 12;
    }
    for record in doc
        .records()
        .values()
        .filter(|r| r.section != "Waypoints" && r.section != "Smudge")
    {
        if let Some(at) = record.cell() {
            let type_id = record.type_id();
            let image = scene
                .catalog
                .rules
                .section_or_empty(&type_id)
                .read_name("Image", 256)
                .unwrap_or(&type_id);
            let foundation = vera20k_core::rules::foundation::Foundation::Stock(
                scene
                    .catalog
                    .art
                    .resolve_metadata_entry(&type_id, image)
                    .and_then(|art| art.foundation)
                    .unwrap_or(0),
            );
            let offsets = if record.section == "Structures" {
                foundation.cell_offsets()
            } else {
                vec![(0, 0)]
            };
            for (dx, dy) in offsets {
                if let Some(c) = grid.get_mut(
                    i32::from(at[0]) + i32::from(dx),
                    i32::from(at[1]) + i32::from(dy),
                ) {
                    c.occupied = true;
                }
            }
        }
    }
    let morphable = |tile: i32| {
        u16::try_from(tile)
            .ok()
            .is_some_and(|tile| scene.palette.morphable.contains(&tile))
            || tile == ids.clear
            || (ids.ramp_base..ids.ramp_base + 20).contains(&tile)
            || ids.ramp_smooth >= 0 && (ids.ramp_smooth..ids.ramp_smooth + 12).contains(&tile)
    };
    let mut corners = CornerGrid::build(&grid, &scratch, span as i32 - 1, &ids, &morphable);
    for &at in selected {
        doc.check_cell(at)?;
        let (x, y) = (i32::from(at[0]), i32::from(at[1]));
        let c = grid.get(x, y).context("格子在 Size 之外")?;
        let goal = target(c.level);
        ensure!(
            goal <= 12,
            "自动坡面支持 0～12 层；可关闭自动坡面编辑到 14 层"
        );
        ensure!(
            morphable(c.tile) && c.overlay == -1 && !c.occupied,
            "自动坡面不能修改水面、道路、建筑或覆盖物占用的格子"
        );
        for _ in 0..28 {
            let current = corners.current_level(&grid, &scratch, x, y, &ids, &morphable);
            // Equal base height alone does not flatten an existing slope.
            let flat = corners.pick_mask(x, y, 1) == corners.pick_mask(x, y, -1);
            if current == i32::from(goal) && (mode != TerrainMode::Flatten || flat) {
                break;
            }
            let direction = if current < i32::from(goal) { 1 } else { -1 };
            let mask = corners.pick_mask(x, y, direction);
            ensure!(
                mask != 0 && corners.apply(direction, x, y, mask),
                "坡面扩展被边界或不可变地形阻挡"
            );
        }
        ensure!(
            corners.current_level(&grid, &scratch, x, y, &ids, &morphable) == i32::from(goal),
            "无法构造连续坡面"
        );
    }
    corners.finalize(&mut grid, &scratch, &ids, &morphable);
    let mut edits = Vec::new();
    for (x, y) in grid.native_cells() {
        let c = grid.get(x, y).context("坡面格网无效")?;
        let at = [x as u16, y as u16];
        let old = value(doc, scene, at);
        if c.level == old.level && c.tile == tile_id(&translated, old) {
            continue;
        }
        let sub = if c.tile != tile_id(&translated, old) {
            0
        } else {
            c.sub_tile
        };
        let key = vera20k_core::map::theater::TileKey {
            tile_id: u16::try_from(c.tile)?,
            sub_tile: sub,
            variant: 0,
        };
        ensure!(
            scene.palette.images.contains_key(&key),
            "坡面资源缺失，未修改地图"
        );
        edits.push((
            at,
            TerrainValue {
                tile: raw(scene, c.tile).context("坡面无法转换为地图编号")?,
                sub,
                level: c.level,
                ..old
            },
        ));
    }
    apply(doc, edits)
}

/// Wall masks use the engine's N/E/S/W bit order and connect same types only.
/// Recalculate both placed and deleted wall neighbours in the same transaction.
pub fn connect_walls(
    doc: &mut MapDocument,
    scene: &MapScene,
    touched: &BTreeSet<Cell>,
) -> Result<Vec<Cell>> {
    let walls: BTreeSet<_> = scene
        .catalog
        .overlays
        .iter()
        .enumerate()
        .filter(|(_, id)| {
            scene
                .catalog
                .rules
                .section_or_empty(id)
                .read_bool("Wall", false)
        })
        .filter_map(|(i, _)| u8::try_from(i).ok())
        .collect();
    let mut area = touched.clone();
    area.extend(touched.iter().copied().flat_map(neighbors));
    let mut changed = Vec::new();
    for at in area {
        if doc.check_cell(at).is_err() {
            continue;
        }
        let (id, frame) = doc.overlay(at);
        if !walls.contains(&id) {
            continue;
        }
        let mut mask = 0;
        for (bit, (dx, dy)) in [(0, -1), (1, 0), (0, 1), (-1, 0)].into_iter().enumerate() {
            let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
            if (0..512).contains(&x)
                && (0..512).contains(&y)
                && doc.overlay([x as u16, y as u16]).0 == id
            {
                mask |= 1 << bit;
            }
        }
        let connected = (frame & 0xf0) | mask;
        if connected != frame {
            doc.set(Target::Overlay(at), Some(Value::Overlay(id, connected)))?;
            changed.push(at);
        }
    }
    Ok(changed)
}

#[cfg(test)]
#[path = "terrain_tools_tests.rs"]
mod tests;
