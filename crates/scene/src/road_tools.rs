//! Three-cell paved-road networks reconstructed from complete stock templates.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    shore_tools::{ground, id, offset, protect},
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, cell_line};
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
fn decode(scene: &MapScene, tile: u16) -> Result<(u8, [i32; 2], [u16; 2])> {
    let ids = scene.palette.tile_ids.context("缺少道路配置")?;
    if ids.paved_road_ends >= 0
        && (ids.paved_road_ends..ids.paved_road_ends + 4).contains(&i32::from(tile))
    {
        return Ok(match i32::from(tile) - ids.paved_road_ends {
            0 => (8, [0, 1], [1, 3]),
            1 => (1, [1, 0], [3, 1]),
            2 => (2, [0, 1], [1, 3]),
            _ => (4, [1, 0], [3, 1]),
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
    let mut queue = VecDeque::new();
    for at in cell_line(a, end) {
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
        let origin = offset(
            at,
            -(i32::from(v.sub) % i32::from(t.width)),
            -(i32::from(v.sub) / i32::from(t.width)),
        )
        .context("道路越界")?;
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
        for (p, expected) in cliff_authoring::stamp(doc, scene, tile, origin, base)? {
            ensure!(
                value(doc, scene, p) == expected,
                "{},{} 的道路模板不完整，请先修复该完整模板",
                p[0],
                p[1]
            );
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
    if cut {
        ensure!(
            selected.iter().any(|p| points.contains(p)),
            "没有选中道路中心线"
        );
        points.retain(|p| !selected.contains(p));
    } else {
        points.extend(&selected);
    }
    let masks: BTreeMap<_, _> = points
        .iter()
        .map(|&p| {
            (
                p,
                DIRS.iter()
                    .filter(|&&(dx, dy, _)| offset(p, dx, dy).is_some_and(|n| points.contains(&n)))
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
                        && points.contains(&n)
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
            1 => (ids.paved_road_ends + 1, offset(at, -1, 0)),
            2 => (ids.paved_road_ends + 2, offset(at, 0, -1)),
            4 => (ids.paved_road_ends + 3, offset(at, -1, 0)),
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
