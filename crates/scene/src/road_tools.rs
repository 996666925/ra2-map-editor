//! Three-cell paved-road networks reconstructed from complete stock templates.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    shore_tools::{ground, id, offset, protect},
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_assets::catalog::TerrainTemplate;
use ra2_map_document::document::{Cell, MapDocument, TerrainValue, cell_line};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const DIRS: [(i32, i32, u8); 4] = [(0, -1, 1), (1, 0, 2), (0, 1, 4), (-1, 0, 8)];
/// (ports N/E/S/W, centre offset). Curves have an extra approach cell.
fn profile(piece: u16) -> Option<(u8, [i32; 2], [u16; 2])> {
    Some(match piece {
        0 => (10, [0, 1], [1, 3]),
        1 => (5, [1, 0], [3, 1]),
        2 => (15, [1, 1], [3, 3]),
        3 => (13, [1, 1], [3, 3]),
        4 => (14, [1, 1], [3, 3]),
        5 => (14, [1, 1], [3, 3]),
        6 => (13, [1, 1], [3, 3]),
        7 => (11, [1, 1], [3, 3]),
        8 => (7, [1, 1], [3, 3]),
        9 => (3, [1, 1], [4, 3]),
        10 => (12, [2, 1], [4, 3]),
        12 => (9, [1, 2], [3, 4]),
        13 => (6, [1, 1], [3, 4]),
        _ => return None,
    })
}
fn road(scene: &MapScene, tile: u16) -> bool {
    scene.palette.tile_ids.is_some_and(|i| {
        i.paved_roads >= 0 && (i.paved_roads..i.paved_roads + 15).contains(&i32::from(tile))
            || i.paved_road_ends >= 0
                && (i.paved_road_ends..i.paved_road_ends + 4).contains(&i32::from(tile))
    })
}
fn stamp_origin(at: Cell, v: TerrainValue, template: &TerrainTemplate) -> Result<Cell> {
    ensure!(
        template.width > 0
            && template
                .cells
                .get(usize::from(v.sub))
                .is_some_and(Option::is_some),
        "道路子块编号无效"
    );
    offset(
        at,
        -(i32::from(v.sub) % i32::from(template.width)),
        -(i32::from(v.sub) / i32::from(template.width)),
    )
    .context("道路模板越界")
}

fn existing_stamp(
    doc: &MapDocument,
    scene: &MapScene,
    tile: u16,
    origin: Cell,
    base: u8,
) -> Result<BTreeMap<Cell, TerrainValue>> {
    let cells = cliff_authoring::stamp(doc, scene, tile, origin, base)?;
    for (&at, &expected) in &cells {
        ensure!(
            value(doc, scene, at) == expected,
            "{},{} 的道路模板不完整，请先修复该完整模板",
            at[0],
            at[1]
        );
    }
    Ok(cells)
}

fn paved_slope(template: &TerrainTemplate) -> Option<u8> {
    let slope = template.cells.first().copied().flatten()?.slope;
    let size = match slope {
        1 | 3 => (1, 3),
        2 | 4 => (3, 1),
        _ => return None,
    };
    ((template.width, template.height) == size
        && template.cells.len() == 3
        && template.cells.iter().all(|cell| {
            cell.is_some_and(|c| c.height == 0 && c.slope == slope && matches!(c.terrain, 11 | 12))
        }))
    .then_some(slope)
}

/// Stock paved slopes have three equally sloped road cells across the road.
/// Index by decoded slope, rejecting templates whose footprint faces the wrong axis.
#[derive(Default)]
pub(crate) struct HeightRoads {
    slopes: [Option<u16>; 4],
    slope_tiles: BTreeSet<u16>,
}

impl HeightRoads {
    pub(crate) fn new(scene: &MapScene) -> Self {
        let mut result = Self::default();
        for (&tile, group) in &scene.palette.groups {
            // Snow has the group and TMPs but lacks the PavedRoadSlopes INI key.
            if !group.name.eq_ignore_ascii_case("Paved Road Slopes") {
                continue;
            }
            let Some(t) = scene.palette.templates.get(&(tile, 0)) else {
                continue;
            };
            result.slope_tiles.insert(tile);
            if let Some(slope) = paved_slope(t) {
                let slot = &mut result.slopes[usize::from(slope - 1)];
                *slot = Some(slot.map_or(tile, |current| current.min(tile)));
            }
        }
        result
    }

    pub(crate) fn contains(&self, scene: &MapScene, tile: u16) -> bool {
        self.slope_tiles.contains(&tile)
            || road(scene, tile)
                && scene.palette.groups.get(&tile).is_some_and(|group| {
                    // Empty paved sets (Lunar) share the next set's cumulative ID.
                    group.category == ra2_map_assets::terrain_library::TerrainCategory::Road
                })
    }

    // A slope is a fixed connection across two elevations. It contributes a
    // port to the flat network without being included in its rebuild.
    fn connects_to_slope(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        at: Cell,
        direction: u8,
        level: u8,
    ) -> Result<bool> {
        let v = value(doc, scene, at);
        let Some(tile) = id(scene, v).filter(|tile| self.slope_tiles.contains(tile)) else {
            return Ok(false);
        };
        let template = &scene.palette.templates[&(tile, 0)];
        let Some(slope) = paved_slope(template) else {
            return Ok(false);
        };
        let bank = match (slope, direction) {
            (1, 2) | (2, 4) | (3, 8) | (4, 1) => 0,
            (1, 8) | (2, 1) | (3, 2) | (4, 4) => 1,
            _ => return Ok(false),
        };
        if v.sub != 1 || v.level.checked_add(bank) != Some(level) {
            return Ok(false);
        }
        existing_stamp(doc, scene, tile, stamp_origin(at, v, template)?, v.level)?;
        Ok(true)
    }

    pub(crate) fn validate_raw_levels(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        edits: &BTreeMap<Cell, TerrainValue>,
    ) -> Result<()> {
        use crate::flatten_tools::RAMPS;

        let mut checked = BTreeSet::new();
        for (&at, &next) in edits {
            let old = value(doc, scene, at);
            let Some(tile) = id(scene, old).filter(|&tile| self.contains(scene, tile)) else {
                continue;
            };
            if next.level == old.level {
                continue;
            }
            let (template, geometry) = road_subtile(scene, tile, old.sub)?;
            let origin = stamp_origin(at, old, template)?;
            let base = old
                .level
                .checked_sub(geometry.height)
                .context("道路子块高度无效")?;
            let delta = i16::from(next.level) - i16::from(old.level);
            if checked.insert((tile, origin)) {
                for (p, original) in existing_stamp(doc, scene, tile, origin, base)? {
                    ensure!(
                        edits.get(&p).is_some_and(|v| i16::from(v.level)
                            - i16::from(original.level)
                            == delta),
                        "整块公路必须一起升降，请用笔刷覆盖整个模板"
                    );
                }
            }

            let pattern = RAMPS
                .get(usize::from(geometry.slope))
                .context("道路坡面无效")?;
            ensure!(
                pattern
                    .iter()
                    .all(|&h| next.level.checked_add(h).is_some_and(|l| l <= 127)),
                "道路坡面将越过 127 层"
            );
            for (dx, dy, edge, opposite) in [
                (-1, 0, [0, 3], [1, 2]),
                (1, 0, [1, 2], [0, 3]),
                (0, -1, [0, 1], [3, 2]),
                (0, 1, [3, 2], [0, 1]),
            ] {
                let Some(p) = offset(at, dx, dy) else {
                    continue;
                };
                let neighbor = value(doc, scene, p);
                let Some(tile) = id(scene, neighbor).filter(|&tile| self.contains(scene, tile))
                else {
                    continue;
                };
                let (_, geometry) = road_subtile(scene, tile, neighbor.sub)?;
                let other = RAMPS
                    .get(usize::from(geometry.slope))
                    .context("相邻公路坡面无效")?;
                let was_joined = edge.map(|i| u16::from(old.level) + u16::from(pattern[i]))
                    == opposite.map(|i| u16::from(neighbor.level) + u16::from(other[i]));
                let neighbor_level = edits.get(&p).unwrap_or(&neighbor).level;
                ensure!(
                    !was_joined || i16::from(neighbor_level) - i16::from(neighbor.level) == delta,
                    "升降会断开相连公路，请一起升降相连路段或开启自动生成连续坡面"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn plan(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        goals: &BTreeMap<Cell, (u8, u8)>,
        plan: &mut PlateauPlan,
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for (&at, &(level, slope)) in goals {
            let old = value(doc, scene, at);
            let tile = id(scene, old).context("道路编号无效")?;
            let t = &scene.palette.templates[&(tile, 0)];
            let origin = stamp_origin(at, old, t)?;
            if !seen.insert((tile, origin)) {
                continue;
            }
            let ids = scene.palette.tile_ids.context("缺少道路配置")?;
            let straight = (ids.paved_roads >= 0
                && (ids.paved_roads..ids.paved_roads + 2).contains(&i32::from(tile)))
                || self.slope_tiles.contains(&tile);
            let new_tile = if slope == 0 {
                if self.slope_tiles.contains(&tile) {
                    ensure!(ids.paved_roads >= 0, "缺少平直公路模板");
                    let offset = match (t.width, t.height) {
                        (1, 3) => 0,
                        (3, 1) => 1,
                        _ => anyhow::bail!("道路坡面尺寸无效"),
                    };
                    u16::try_from(ids.paved_roads + offset)?
                } else {
                    tile
                }
            } else {
                ensure!(straight, "公路弯道、路口和端头必须保持平坦");
                self.slopes
                    .get(usize::from(slope - 1))
                    .copied()
                    .flatten()
                    .context("当前地形集没有匹配的公路坡面")?
            };
            let new = scene
                .palette
                .templates
                .get(&(new_tile, 0))
                .context("道路模板缺失")?;
            ensure!(
                (t.width, t.height) == (new.width, new.height),
                "公路坡面须沿直路方向衔接"
            );
            ensure!(t.cells.len() == new.cells.len(), "公路模板尺寸不一致");
            let raw = *scene
                .catalog
                .raw_tiles
                .get(&new_tile)
                .context("道路编号无法保存")?;
            let mut changed = false;
            for (sub, geometry) in t.cells.iter().enumerate() {
                let Some(g) = geometry else { continue };
                let p = offset(
                    origin,
                    sub as i32 % i32::from(t.width),
                    sub as i32 / i32::from(t.width),
                )
                .context("道路模板越界")?;
                doc.check_cell(p)?;
                let prev = value(doc, scene, p);
                ensure!(
                    id(scene, prev) == Some(tile)
                        && usize::from(prev.sub) == sub
                        && prev.level == old.level
                        && g.height == 0,
                    "公路模板不完整，请先修复该路段"
                );
                ensure!(
                    goals.get(&p).copied().unwrap_or((prev.level, g.slope)) == (level, slope),
                    "整块公路需要相同的高度和坡向，请用笔刷覆盖整个路宽"
                );
                ensure!(
                    new.cells[sub].is_some_and(|c| c.height == 0 && c.slope == slope)
                        && scene.palette.images.contains_key(
                            &vera20k_core::map::theater::TileKey {
                                tile_id: new_tile,
                                sub_tile: sub as u8,
                                variant: 0
                            }
                        ),
                    "道路模板缺少匹配的子块"
                );
                let next = TerrainValue {
                    tile: raw,
                    level,
                    ..prev
                };
                if next != prev {
                    changed = true;
                    plan.edits.insert(p, next);
                }
            }
            if changed && slope != 0 {
                plan.cliffs.push(CliffStamp {
                    tile: new_tile,
                    origin,
                    base: level,
                });
            }
        }
        Ok(())
    }
}

fn road_subtile(
    scene: &MapScene,
    tile: u16,
    sub: u8,
) -> Result<(&TerrainTemplate, ra2_map_assets::catalog::TerrainSubtile)> {
    let template = scene
        .palette
        .templates
        .get(&(tile, 0))
        .context("道路模板缺失")?;
    let geometry = template
        .cells
        .get(usize::from(sub))
        .copied()
        .flatten()
        .context("道路子块编号无效")?;
    Ok((template, geometry))
}

fn decode(scene: &MapScene, tile: u16) -> Result<(u8, [i32; 2], [u16; 2])> {
    let ids = scene.palette.tile_ids.context("缺少道路配置")?;
    if ids.paved_road_ends >= 0
        && (ids.paved_road_ends..ids.paved_road_ends + 4).contains(&i32::from(tile))
    {
        // Stock end pieces connect W, S, E, N respectively. In particular,
        // the two 3x1 caps must face into the road along the Y axis.
        return Ok(match i32::from(tile) - ids.paved_road_ends {
            0 => (8, [0, 1], [1, 3]),
            1 => (4, [1, 0], [3, 1]),
            2 => (2, [0, 1], [1, 3]),
            _ => (1, [1, 0], [3, 1]),
        });
    }
    profile(tile - ids.paved_roads as u16).context("遇到毁坏道路或特殊道路，请避开该完整模板")
}
fn closest(points: &BTreeSet<Cell>, at: Cell) -> Cell {
    points
        .iter()
        .filter(|p| p[0].abs_diff(at[0]) <= 1 && p[1].abs_diff(at[1]) <= 1)
        .min_by_key(|p| (p[0].abs_diff(at[0]) + p[1].abs_diff(at[1]), **p))
        .copied()
        .unwrap_or(at)
}

/// Drag one axis-aligned segment. Consecutive strokes merge at the centreline;
/// ends become rounded turns, T junctions, or crossings. Cutting rebuilds caps.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    cut: bool,
) -> Result<PlateauPlan> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    ensure!(
        ids.paved_roads >= 0 && ids.paved_road_ends >= 0,
        "当前环境没有标准道路及道路端点模板"
    );
    doc.check_cell(a)?;
    doc.check_cell(b)?;
    let along_x = a[0].abs_diff(b[0]) >= a[1].abs_diff(b[1]);
    let end = if along_x { [b[0], a[1]] } else { [a[0], b[1]] };
    let roads = HeightRoads::new(scene);
    let on_slope =
        |at| id(scene, value(doc, scene, at)).is_some_and(|tile| roads.slope_tiles.contains(&tile));
    let mut queue = VecDeque::new();
    for at in cell_line(a, end) {
        ensure!(!on_slope(at), "平路绘制和切除请避开已有公路坡面");
        for dx in -2..=2 {
            for dy in -2..=2 {
                if let Some(p) = offset(at, dx, dy) {
                    queue.push_back(p);
                }
            }
        }
    }
    let mut visited = BTreeSet::new();
    let mut stamps = BTreeSet::new();
    let mut old = BTreeSet::new();
    let mut points = BTreeSet::new();
    let base = value(doc, scene, a).level;
    while let Some(at) = queue.pop_front() {
        if !visited.insert(at) || doc.check_cell(at).is_err() {
            continue;
        }
        let v = value(doc, scene, at);
        let Some(tile) = id(scene, v).filter(|&t| road(scene, t)) else {
            continue;
        };
        // Nearby roads on another level are not part of this network.
        if v.level != base {
            continue;
        }
        let t = scene
            .palette
            .templates
            .get(&(tile, 0))
            .context("已有道路模板缺失")?;
        let origin = stamp_origin(at, v, t)?;
        if !stamps.insert((tile, origin)) {
            continue;
        }
        let (mask, centre, size) = decode(scene, tile)?;
        ensure!(
            [t.width, t.height] == size
                && t.cells
                    .iter()
                    .all(|s| s.is_some_and(|s| s.height == 0 && s.slope == 0)),
            "道路模板几何被 Mod 修改，无法自动连接"
        );
        for p in existing_stamp(doc, scene, tile, origin, base)?.into_keys() {
            old.insert(p);
            queue.extend(crate::terrain_tools::neighbors(p));
        }
        let c = offset(origin, centre[0], centre[1]).context("道路中心越界")?;
        points.insert(c);
        for (dx, dy, bit) in DIRS {
            if mask & bit == 0 {
                continue;
            }
            let length = if dx < 0 {
                centre[0]
            } else if dx > 0 {
                i32::from(t.width) - 1 - centre[0]
            } else if dy < 0 {
                centre[1]
            } else {
                i32::from(t.height) - 1 - centre[1]
            };
            for d in 1..=length {
                points.insert(offset(c, dx * d, dy * d).context("道路越界")?);
            }
        }
    }
    let a = closest(&points, a);
    let b = closest(&points, b);
    let along_x = a[0].abs_diff(b[0]) >= a[1].abs_diff(b[1]);
    let b = if along_x { [b[0], a[1]] } else { [a[0], b[1]] };
    ensure!(a != b, "沿地图轴向拖动绘制或切除道路");
    let selected: BTreeSet<_> = cell_line(a, b).into_iter().collect();
    ensure!(
        !selected.iter().copied().any(on_slope),
        "平路绘制和切除请避开已有公路坡面"
    );
    if cut {
        ensure!(
            selected.iter().any(|p| points.contains(p)),
            "没有选中道路中心线"
        );
        points.retain(|p| !selected.contains(p));
    } else {
        points.extend(&selected);
    }
    let mut slope_ports = BTreeMap::new();
    for &p in &points {
        for (dx, dy, bit) in DIRS {
            if let Some(n) = offset(p, dx, dy)
                && roads.connects_to_slope(doc, scene, n, bit, base)?
            {
                *slope_ports.entry(p).or_insert(0) |= bit;
            }
        }
    }
    let connected = |p, dx, dy, bit| {
        offset(p, dx, dy).is_some_and(|n| points.contains(&n))
            || slope_ports.get(&p).is_some_and(|ports| ports & bit != 0)
    };
    let masks: BTreeMap<_, _> = points
        .iter()
        .map(|&p| {
            (
                p,
                DIRS.iter()
                    .filter(|&&(dx, dy, bit)| connected(p, dx, dy, bit))
                    .fold(0, |m, &(_, _, bit)| m | bit),
            )
        })
        .collect();
    let mut result = PlateauPlan {
        edits: BTreeMap::new(),
        overlays: BTreeMap::new(),
        cliffs: Vec::new(),
        interior: selected,
        base,
    };
    for &at in &old {
        result.edits.insert(
            at,
            cliff_authoring::clear(scene, value(doc, scene, at), base)?,
        );
    }
    let mut occupied = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    {
        let mut place = |tile: u16, origin: Cell, expected: [u16; 2], ports: u8| -> Result<()> {
            let t = scene
                .palette
                .templates
                .get(&(tile, 0))
                .context("缺少道路连接模板")?;
            ensure!(
                [t.width, t.height] == expected
                    && t.cells
                        .iter()
                        .all(|s| s.is_some_and(|s| s.height == 0 && s.slope == 0)),
                "道路连接模板不是标准平面几何"
            );
            let cells = cliff_authoring::stamp(doc, scene, tile, origin, base)?;
            for &p in cells.keys() {
                ensure!(
                    !occupied.contains(&p),
                    "道路转角或路口相距过近，请至少留出 4 格直道"
                );
                if points.contains(&p) {
                    consumed.insert(p);
                }
            }
            // No branch may disappear under a larger corner/junction template.
            for &p in cells.keys().filter(|p| points.contains(*p)) {
                for (dx, dy, bit) in DIRS {
                    if let Some(n) = offset(p, dx, dy)
                        && connected(p, dx, dy, bit)
                        && !cells.contains_key(&n)
                    {
                        ensure!(ports & bit != 0, "道路转角内出现额外分支，请加大路口间距");
                    }
                }
            }
            occupied.extend(cells.keys().copied());
            result.edits.extend(cells);
            result.cliffs.push(CliffStamp { tile, origin, base });
            Ok(())
        };
        for (&at, &mask) in &masks {
            let piece = match mask {
                3 => 9,
                6 => 13,
                9 => 12,
                12 => 10,
                7 => 8,
                11 => 7,
                13 => 6,
                14 => 5,
                15 => 2,
                _ => continue,
            };
            let (_, centre, size) = profile(piece).unwrap();
            let origin = offset(at, -centre[0], -centre[1]).context("道路转角越界")?;
            place(ids.paved_roads as u16 + piece, origin, size, mask)?;
        }
    }
    for (&at, &mask) in &masks {
        if consumed.contains(&at) || mask == 0 {
            continue;
        }
        let (tile, origin) = match mask {
            1 => (ids.paved_road_ends + 3, offset(at, -1, 0)),
            2 => (ids.paved_road_ends + 2, offset(at, 0, -1)),
            4 => (ids.paved_road_ends + 1, offset(at, -1, 0)),
            8 => (ids.paved_road_ends, offset(at, 0, -1)),
            5 => (ids.paved_roads + 1, offset(at, -1, 0)),
            10 => (ids.paved_roads, offset(at, 0, -1)),
            _ => anyhow::bail!("道路连接形状无效"),
        };
        let origin = origin.context("道路端点越界")?;
        let cells = cliff_authoring::stamp(doc, scene, tile as u16, origin, base)?;
        for &p in cells.keys() {
            ensure!(
                occupied.insert(p),
                "道路过窄或并排距离过近，请留出至少 3 格中心线间距"
            );
        }
        result.edits.extend(cells);
        result.cliffs.push(CliffStamp {
            tile: tile as u16,
            origin,
            base,
        });
    }
    for (&at, &v) in &result.edits {
        let previous = value(doc, scene, at);
        ensure!(
            previous.level == base && (old.contains(&at) || ground(scene, previous)),
            "{},{} 不是同高度平地或完整道路，请避开水面、悬崖和坡道",
            at[0],
            at[1]
        );
        ensure!(v.level == base, "道路高度无效");
    }
    result.edits.retain(|&at, v| *v != value(doc, scene, at));
    protect(doc, scene, &result.edits)?;
    Ok(result)
}
