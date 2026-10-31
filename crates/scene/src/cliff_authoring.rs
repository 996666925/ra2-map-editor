//! Existing cliff topology is read from complete disk templates, not UI state.
use crate::{
    cliff_tools::{self, PlateauPlan},
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

fn tile(scene: &MapScene, raw: i32) -> Option<u16> {
    scene.palette.raw_ids.get(&raw).copied()
}
fn offset(at: Cell, dx: i32, dy: i32) -> Option<Cell> {
    let x = i32::from(at[0]) + dx;
    let y = i32::from(at[1]) + dy;
    ((0..i32::from(MAX_CELL_SIDE)).contains(&x) && (0..i32::from(MAX_CELL_SIDE)).contains(&y))
        .then_some([x as u16, y as u16])
}

pub(crate) fn clear(scene: &MapScene, old: TerrainValue, level: u8) -> Result<TerrainValue> {
    let id = scene.palette.tile_ids.context("缺少地形配置")?.clear;
    let raw = scene
        .catalog
        .raw_tiles
        .get(&(id as u16))
        .context("缺少平地模板")?;
    Ok(TerrainValue {
        tile: *raw,
        sub: 0,
        level,
        ..old
    })
}

fn cliff(scene: &MapScene, v: TerrainValue, at: Cell) -> Option<(u16, Cell, u8)> {
    let id = tile(scene, v.tile)?;
    crate::coastal_cliffs::piece(scene, id)?;
    let t = scene.palette.templates.get(&(id, 0))?;
    let s = t.cells.get(usize::from(v.sub))?.as_ref()?;
    let origin = offset(
        at,
        -(i32::from(v.sub) % i32::from(t.width)),
        -(i32::from(v.sub) / i32::from(t.width)),
    )?;
    Some((id, origin, v.level.checked_sub(s.height)?))
}

/// Follow only the touched elevation component; unrelated plateaus are untouched.
fn existing(
    doc: &MapDocument,
    scene: &MapScene,
    seeds: &BTreeSet<Cell>,
    base: u8,
) -> Result<(BTreeSet<Cell>, BTreeSet<Cell>)> {
    let mut queue = VecDeque::new();
    for &at in seeds {
        for dx in -2..=2 {
            for dy in -2..=2 {
                if let Some(p) = offset(at, dx, dy) {
                    queue.push_back(p);
                }
            }
        }
    }
    let mut visited = BTreeSet::new();
    let mut core = BTreeSet::new();
    let mut old = BTreeSet::new();
    let mut stamps = BTreeSet::new();
    while let Some(at) = queue.pop_front() {
        if !visited.insert(at) || doc.check_cell(at).is_err() {
            continue;
        }
        let v = value(doc, scene, at);
        let c = cliff(scene, v, at).filter(|c| c.2 == base);
        if v.level != base + 4 && c.is_none() {
            continue;
        }
        if let Some((id, origin, _)) = c {
            stamps.insert((id, origin));
            old.insert(at);
        } else {
            core.insert(at);
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(p) = offset(at, dx, dy) {
                    queue.push_back(p);
                }
            }
        }
    }
    // The selector's high neighbours recover the logical boundary. A raised
    // TMP pixel at the stamp origin is a cliff lip, not another interior cell.
    for (id, origin) in stamps {
        let t = &scene.palette.templates[&(id, 0)];
        for (sub, s) in t.cells.iter().enumerate() {
            if let Some(s) = s {
                let p = offset(
                    origin,
                    sub as i32 % i32::from(t.width),
                    sub as i32 / i32::from(t.width),
                )
                .context("悬崖越界")?;
                let v = value(doc, scene, p);
                ensure!(
                    v.tile == scene.catalog.raw_tiles[&id]
                        && usize::from(v.sub) == sub
                        && v.level == base + s.height,
                    "{},{} 的已有悬崖模板不完整，请先修复手工覆盖的子块",
                    p[0],
                    p[1]
                );
                old.insert(p);
            }
        }
        let dirs: &[(i32, i32)] = match crate::coastal_cliffs::piece(scene, id)
            .context("无法识别已有悬崖")?
            .0
            + 1
        {
            1 | 3..=4 | 35..=38 => &[(1, 0)],
            2 => &[(1, -1)],
            5..=8 => &[(0, -1)],
            9..=11 | 34 => &[(0, -1), (-1, 0)],
            12..=14 => &[(-1, -1)],
            15..=18 => &[(-1, 0)],
            19..=21 | 23..=26 => &[(0, 1)],
            22 => &[(-1, 1)],
            27..=29 => &[(1, 1)],
            30..=33 => &[(1, 0), (0, 1)],
            39 => &[(1, 0), (0, -1)],
            40 => &[(-1, 0), (0, 1)],
            _ => &[],
        };
        for &(dx, dy) in dirs {
            if let Some(p) = offset(origin, dx, dy)
                && doc.check_cell(p).is_ok()
                && value(doc, scene, p).level == base + 4
            {
                core.insert(p);
            }
        }
    }
    Ok((core, old))
}

pub fn infer_base(doc: &MapDocument, scene: &MapScene, at: Cell) -> u8 {
    if let Some((_, _, base)) = cliff(scene, value(doc, scene, at), at) {
        return base;
    }
    let level = value(doc, scene, at).level;
    let mut queue = VecDeque::from([at]);
    let mut seen = BTreeSet::new();
    while let Some(p) = queue.pop_front() {
        if !seen.insert(p) || doc.check_cell(p).is_err() || value(doc, scene, p).level != level {
            continue;
        }
        if let Some((_, _, base)) = cliff(scene, value(doc, scene, p), p) {
            return base;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if let Some(n) = offset(p, dx, dy) {
                queue.push_back(n);
            }
        }
    }
    level
}

fn finish(
    doc: &MapDocument,
    scene: &MapScene,
    mut p: PlateauPlan,
    old: BTreeSet<Cell>,
) -> Result<PlateauPlan> {
    let stamped: BTreeSet<_> = p
        .cliffs
        .iter()
        .flat_map(|s| {
            let t = &scene.palette.templates[&(s.tile, 0)];
            t.cells.iter().enumerate().filter_map(move |(i, c)| {
                c.as_ref().and_then(|_| {
                    offset(
                        s.origin,
                        i as i32 % i32::from(t.width),
                        i as i32 / i32::from(t.width),
                    )
                })
            })
        })
        .collect();
    for at in old {
        if !stamped.contains(&at) {
            let level = if p.interior.contains(&at) {
                p.base + 4
            } else {
                p.base
            };
            p.edits
                .insert(at, clear(scene, value(doc, scene, at), level)?);
        }
    }
    p.edits.retain(|&at, v| *v != value(doc, scene, at));
    Ok(p)
}

fn portals(
    doc: &MapDocument,
    scene: &MapScene,
    core: &mut BTreeSet<Cell>,
    selection: &BTreeSet<Cell>,
    base: u8,
) -> Result<BTreeSet<Cell>> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let mut seen = BTreeSet::new();
    let mut protected = BTreeSet::new();
    let mut nearby = BTreeSet::new();
    for &at in core.iter() {
        for x in -1..=1 {
            for y in -1..=1 {
                if let Some(p) = offset(at, x, y) {
                    nearby.insert(p);
                }
            }
        }
    }
    for at in nearby {
        let v = value(doc, scene, at);
        let Some(id) = tile(scene, v.tile) else {
            continue;
        };
        // (axis, first high coordinate relative to origin, high towards +axis)
        let profile = if ids.slope_set_pieces >= 0
            && (ids.slope_set_pieces..ids.slope_set_pieces + 10).contains(&i32::from(id))
        {
            Some(match i32::from(id) - ids.slope_set_pieces {
                0 | 1 => (1, 0, false),
                2 | 3 => (0, 0, false),
                4 | 5 => (1, 3, true),
                6 => (1, 1, true),
                7 | 8 => (0, 3, true),
                9 => (0, 1, true),
                _ => unreachable!(),
            })
        } else {
            let family = [ids.cliffs.bridge_set, ids.cliffs.wood_bridge_set]
                .into_iter()
                .flatten()
                .find(|&b| (b..b + 16).contains(&id));
            family.and_then(|b| match id - b {
                0 | 1 => Some((0, 0, false)),
                2 => Some((0, 1, true)),
                3 | 4 => Some((1, 0, false)),
                5 => Some((1, 1, true)),
                _ => None,
            })
        };
        let Some((axis, edge, positive)) = profile else {
            continue;
        };
        let t = &scene.palette.templates[&(id, 0)];
        let sub = usize::from(v.sub);
        let h = t
            .cells
            .get(sub)
            .and_then(|s| *s)
            .context("已有入口子块无效")?
            .height;
        if v.level.checked_sub(h) != Some(base) {
            continue;
        }
        let origin = offset(
            at,
            -(sub as i32 % i32::from(t.width)),
            -(sub as i32 / i32::from(t.width)),
        )
        .context("入口越界")?;
        if !seen.insert((id, origin)) {
            continue;
        }
        for (i, c) in t.cells.iter().enumerate() {
            if let Some(c) = c {
                let p = offset(
                    origin,
                    i as i32 % i32::from(t.width),
                    i as i32 / i32::from(t.width),
                )
                .context("入口越界")?;
                let old = value(doc, scene, p);
                ensure!(
                    old.tile == v.tile && usize::from(old.sub) == i && old.level == base + c.height,
                    "已有坡道或桥头模板不完整"
                );
                ensure!(
                    !selection.contains(&p),
                    "所选区域触及已有坡道或桥头，请避开完整入口模板"
                );
                core.remove(&p);
                let relative = i32::from(p[axis]) - i32::from(origin[axis]);
                if if positive {
                    relative >= edge
                } else {
                    relative <= edge
                } {
                    core.insert(p);
                }
                protected.insert(p);
            }
        }
    }
    Ok(protected)
}

fn preserve_portals(
    doc: &MapDocument,
    scene: &MapScene,
    p: &mut PlateauPlan,
    protected: &BTreeSet<Cell>,
) {
    let mut restore = protected.clone();
    p.cliffs.retain(|s| {
        let t = &scene.palette.templates[&(s.tile, 0)];
        let cells: Vec<_> = t
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                c.as_ref().and_then(|_| {
                    offset(
                        s.origin,
                        i as i32 % i32::from(t.width),
                        i as i32 / i32::from(t.width),
                    )
                })
            })
            .collect();
        let keep = cells.iter().all(|p| !protected.contains(p));
        if !keep {
            restore.extend(cells);
        }
        keep
    });
    for at in restore {
        p.edits.remove(&at);
    }
    p.edits.retain(|&at, v| *v != value(doc, scene, at));
}

/// Union/subtraction rebuilds the touched connected components, including old
/// complete stamps, so expanding or cutting cannot leave a former cliff inside.
pub fn reshape(
    doc: &MapDocument,
    scene: &MapScene,
    selection: BTreeSet<Cell>,
    base: u8,
    cut: bool,
) -> Result<PlateauPlan> {
    ensure!(base <= 10, "标准悬崖高差为 4 层，低地高度须在 0～10 层");
    let (mut core, mut old) = existing(doc, scene, &selection, base)?;
    let protected = portals(doc, scene, &mut core, &selection, base)?;
    if cut {
        ensure!(!core.is_empty(), "所选区域没有可切除的同层高台");
        for at in &selection {
            core.remove(at);
        }
        old.extend(
            selection
                .iter()
                .copied()
                .filter(|&at| value(doc, scene, at).level == base + 4),
        );
    } else {
        core.extend(selection);
    }
    if core.is_empty() {
        let edits = old
            .into_iter()
            .map(|at| Ok((at, clear(scene, value(doc, scene, at), base)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        cliff_tools::validate(doc, scene, &edits, base, true)?;
        return Ok(PlateauPlan {
            edits,
            overlays: BTreeMap::new(),
            cliffs: vec![],
            interior: core,
            base,
        });
    }
    let p = cliff_tools::geometry(doc, scene, core, base)?;
    let mut p = finish(doc, scene, p, old)?;
    preserve_portals(doc, scene, &mut p, &protected);
    crate::coastal_cliffs::adapt(doc, scene, &mut p)?;
    cliff_tools::validate(doc, scene, &p.edits, base, true)?;
    Ok(p)
}

/// A snapped cliff stroke raises a narrow band on the selected high side.
/// Complete return faces close free ends and the rear boundary. Continuing a
/// stroke unions that band with existing high ground and removes internal faces.
pub fn line(
    doc: &MapDocument,
    scene: &MapScene,
    start: Cell,
    end: Cell,
    base: u8,
    back: bool,
) -> Result<PlateauPlan> {
    ensure!(base <= 10, "标准悬崖高差为 4 层，低地高度须在 0～10 层");
    ensure!(start != end, "拖动绘制悬崖线段");
    let dx = i32::from(end[0]) - i32::from(start[0]);
    let dy = i32::from(end[1]) - i32::from(start[1]);
    let (sx, sy) = if dx.abs() > dy.abs() * 2 {
        (dx.signum(), 0)
    } else if dy.abs() > dx.abs() * 2 {
        (0, dy.signum())
    } else {
        (dx.signum(), dy.signum())
    };
    let length = dx.abs().max(dy.abs());
    let mut normal = (-sy, sx);
    if normal.0 + normal.1 > 0 || normal.0 + normal.1 == 0 && normal.0 > 0 {
        normal = (-normal.0, -normal.1);
    }
    if back {
        normal = (-normal.0, -normal.1);
    }
    let norm = sx * sx + sy * sy;
    let project = |at: Cell| {
        let x = i32::from(at[0]) - i32::from(start[0]);
        let y = i32::from(at[1]) - i32::from(start[1]);
        ((x * sx + y * sy), x * normal.0 + y * normal.1)
    };
    let mut band = BTreeSet::new();
    for x in -length - 18..=length + 18 {
        for y in -length - 18..=length + 18 {
            if let Some(p) = offset(start, x, y) {
                let (a, d) = project(p);
                if (0..=length * norm).contains(&a) && (0..=3 * norm).contains(&d) {
                    band.insert(p);
                }
            }
        }
    }
    reshape(doc, scene, band, base, false)
}

pub(crate) fn stamp(
    doc: &MapDocument,
    scene: &MapScene,
    id: u16,
    origin: Cell,
    base: u8,
) -> Result<BTreeMap<Cell, TerrainValue>> {
    let t = scene
        .palette
        .templates
        .get(&(id, 0))
        .context("缺少完整地形模板")?;
    let raw = *scene.catalog.raw_tiles.get(&id).context("模板无法保存")?;
    let mut edits = BTreeMap::new();
    for (sub, c) in t.cells.iter().enumerate() {
        if let Some(c) = c {
            let at = offset(
                origin,
                sub as i32 % i32::from(t.width),
                sub as i32 / i32::from(t.width),
            )
            .context("模板越界")?;
            doc.check_cell(at)?;
            ensure!(
                scene
                    .palette
                    .images
                    .contains_key(&vera20k_core::map::theater::TileKey {
                        tile_id: id,
                        sub_tile: sub as u8,
                        variant: 0
                    }),
                "模板图像缺失"
            );
            let level = base
                .checked_add(c.height)
                .filter(|&v| v <= 14)
                .context("模板高度超过14层")?;
            edits.insert(
                at,
                TerrainValue {
                    tile: raw,
                    sub: sub as u8,
                    level,
                    ..value(doc, scene, at)
                },
            );
        }
    }
    Ok(edits)
}

/// Refuse a partial replacement of any old multi-cell special template.
pub(crate) fn complete_replacement(
    doc: &MapDocument,
    scene: &MapScene,
    edits: &BTreeMap<Cell, TerrainValue>,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let mut seen = BTreeSet::new();
    for &at in edits.keys() {
        let v = value(doc, scene, at);
        let Some(id) = tile(scene, v.tile) else {
            continue;
        };
        if scene.palette.morphable.contains(&id) || i32::from(id) == ids.clear {
            continue;
        }
        let t = scene
            .palette
            .templates
            .get(&(id, 0))
            .context("原地形模板缺失")?;
        let origin = offset(
            at,
            -(i32::from(v.sub) % i32::from(t.width)),
            -(i32::from(v.sub) / i32::from(t.width)),
        )
        .context("原模板越界")?;
        if !seen.insert((id, origin)) {
            continue;
        }
        for (sub, c) in t.cells.iter().enumerate() {
            if c.is_some() {
                let p = offset(
                    origin,
                    sub as i32 % i32::from(t.width),
                    sub as i32 / i32::from(t.width),
                )
                .context("原模板越界")?;
                ensure!(
                    edits.contains_key(&p),
                    "请避开拐角或加宽区域：操作将切断 {},{} 的完整模板",
                    p[0],
                    p[1]
                );
                let old = value(doc, scene, p);
                ensure!(
                    old.tile == v.tile && old.sub == sub as u8,
                    "原有地形模板不完整"
                );
            }
        }
    }
    Ok(())
}

/// Stock straight ramp ends enclose a two-cell-wide walkable centre. North
/// and west ends are sparse pairs completed by their separate corner TMP.
pub fn ramp(
    doc: &MapDocument,
    scene: &MapScene,
    start: Cell,
    end: Cell,
    base: u8,
) -> Result<PlateauPlan> {
    ensure!(start != end, "从高台向低地拖动，确定坡道方向");
    ensure!(base <= 10, "坡道高度超限");
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let end_base = u16::try_from(ids.slope_set_pieces)
        .ok()
        .or(ids.cliffs.cliff_ramps)
        .context("当前环境没有高台坡道端块")?;
    let dx = i32::from(end[0]) - i32::from(start[0]);
    let dy = i32::from(end[1]) - i32::from(start[1]);
    let patterns: &[(u16, i32, i32)] = if dy.abs() >= dx.abs() {
        if dy > 0 {
            &[(0, 0, 0), (1, 3, 0)]
        } else {
            &[(4, 0, 0), (5, 3, 0), (6, 4, 2)]
        }
    } else if dx > 0 {
        &[(3, 0, 0), (2, 0, 3)]
    } else {
        &[(7, 0, 0), (8, 0, 3), (9, 2, 4)]
    };
    let mut best = None;
    for ox in -6..=1 {
        for oy in -6..=1 {
            let Some(origin) = offset(start, ox, oy) else {
                continue;
            };
            let candidate = (|| -> Result<PlateauPlan> {
                let mut edits = BTreeMap::new();
                let mut cliffs = Vec::new();
                for &(n, x, y) in patterns {
                    let p = offset(origin, x, y).context("坡道越界")?;
                    let id = end_base.checked_add(n).context("坡道编号溢出")?;
                    for (at, v) in stamp(doc, scene, id, p, base)? {
                        ensure!(edits.insert(at, v).is_none(), "坡道模板重叠");
                    }
                    cliffs.push(cliff_tools::CliffStamp {
                        tile: id,
                        origin: p,
                        base,
                    });
                }
                ensure!(edits.contains_key(&start), "请从悬崖边缘拖动");
                let mut touched = false;
                for (&at, &v) in &edits {
                    let id = tile(scene, v.tile).context("坡道编号无效")?;
                    let s = scene.palette.templates[&(id, 0)].cells[usize::from(v.sub)].unwrap();
                    if s.slope == 0 {
                        ensure!(value(doc, scene, at).level == v.level, "坡道两侧高度不匹配");
                    }
                    touched |= cliff(scene, value(doc, scene, at), at).is_some();
                }
                ensure!(touched, "请放置在直线悬崖边界");
                complete_replacement(doc, scene, &edits)?;
                cliff_tools::validate(doc, scene, &edits, base, true)?;
                Ok(PlateauPlan {
                    edits,
                    overlays: BTreeMap::new(),
                    cliffs,
                    interior: BTreeSet::new(),
                    base,
                })
            })();
            if let Ok(p) = candidate {
                let cx =
                    p.edits.keys().map(|p| i32::from(p[0])).sum::<i32>() / p.edits.len() as i32;
                let cy =
                    p.edits.keys().map(|p| i32::from(p[1])).sum::<i32>() / p.edits.len() as i32;
                let score = (cx - i32::from(start[0])).abs() + (cy - i32::from(start[1])).abs();
                if best.as_ref().is_none_or(|(s, _)| score < *s) {
                    best = Some((score, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
        .context("此处无法衔接坡道：请从足够宽的直线悬崖向低地拖动，并避开对象、覆盖物与拐角")
}

#[cfg(test)]
#[path = "cliff_authoring_tests.rs"]
mod tests;
