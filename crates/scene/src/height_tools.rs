//! Height-only plans that preserve existing terrain art and complete cliff stamps.
use crate::{
    cliff_tools::PlateauPlan,
    map::MapScene,
    terrain_tools::{self, TerrainMode, value},
};
use anyhow::{Context, Result, ensure};
use ra2_map_assets::terrain_library::TerrainCategory;
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Per-cell geometry also works for deliberately hand-assembled partial stamps.
pub(crate) fn cliff_geometry(scene: &MapScene, v: TerrainValue) -> Result<Option<(u8, u8)>> {
    let Some(&tile) = scene.palette.raw_ids.get(&v.tile) else {
        return Ok(None);
    };
    if !scene
        .palette
        .groups
        .get(&tile)
        .is_some_and(|g| g.category == TerrainCategory::Cliff)
    {
        return Ok(None);
    }
    let t = scene
        .palette
        .templates
        .get(&(tile, 0))
        .context("悬崖模板几何缺失")?;
    let relative = t
        .cells
        .get(usize::from(v.sub))
        .and_then(|s| *s)
        .context("悬崖子块编号无效")?
        .height;
    let mut top = t
        .cells
        .iter()
        .flatten()
        .map(|s| s.height)
        .max()
        .context("悬崖模板为空")?;
    // Some standard cliff pieces contain only low-side cells. Their cliff top
    // is still four levels above the base, regardless of the local TMP maximum.
    if crate::coastal_cliffs::piece(scene, tile).is_some() {
        top = top.max(4);
    }
    Ok(Some((relative, top)))
}

struct CliffCells {
    base: u8,
    cells: Vec<(Cell, TerrainValue)>,
}

/// Recover a cliff from any subtile, requiring all its cells within Size.
fn cliff_cells(doc: &MapDocument, scene: &MapScene, at: Cell) -> Result<Option<CliffCells>> {
    let v = value(doc, scene, at);
    let Some(&tile) = scene.palette.raw_ids.get(&v.tile) else {
        return Ok(None);
    };
    if !scene
        .palette
        .groups
        .get(&tile)
        .is_some_and(|g| g.category == TerrainCategory::Cliff)
    {
        return Ok(None);
    }
    let t = scene
        .palette
        .templates
        .get(&(tile, 0))
        .context("悬崖模板几何缺失")?;
    ensure!(t.width > 0, "悬崖模板宽度无效");
    let sub = usize::from(v.sub);
    let s = t.cells.get(sub).and_then(|s| *s).context("悬崖子块无效")?;
    let base = v
        .level
        .checked_sub(s.height)
        .context("悬崖子块高度不一致")?;
    let x = i32::from(at[0]) - sub as i32 % i32::from(t.width);
    let y = i32::from(at[1]) - sub as i32 / i32::from(t.width);
    let mut cells = Vec::new();
    for (sub, s) in t.cells.iter().enumerate() {
        let Some(s) = s else { continue };
        let [Ok(px), Ok(py)] = [
            u16::try_from(x + sub as i32 % i32::from(t.width)),
            u16::try_from(y + sub as i32 / i32::from(t.width)),
        ] else {
            continue;
        };
        let p = [px, py];
        if !doc.contains_cell(p) {
            continue;
        }
        let old = value(doc, scene, p);
        ensure!(
            old.tile == v.tile
                && usize::from(old.sub) == sub
                && u16::from(old.level) == u16::from(base) + u16::from(s.height),
            "{},{} 的悬崖模板不完整或高度不一致，请先修复子块",
            p[0],
            p[1]
        );
        cells.push((p, old));
    }
    Ok(Some(CliffCells { base, cells }))
}

/// FA2's normal cliff click ignores brush size. Ctrl follows the connected
/// tileset (including diagonal contacts) before expanding complete stamps.
pub fn cliff_selection(
    doc: &MapDocument,
    scene: &MapScene,
    seed: Cell,
    connected: bool,
) -> Result<Option<BTreeSet<Cell>>> {
    doc.check_cell(seed)?;
    if cliff_geometry(scene, value(doc, scene, seed))?.is_none() {
        return Ok(None);
    }
    let tile = scene.palette.raw_ids[&value(doc, scene, seed).tile];
    let group = scene.palette.groups[&tile].id;
    let mut queue = VecDeque::from([seed]);
    let mut seen = BTreeSet::new();
    let mut selected = BTreeSet::new();
    while let Some(at) = queue.pop_front() {
        if !seen.insert(at) || !doc.contains_cell(at) {
            continue;
        }
        let v = value(doc, scene, at);
        let matched = scene
            .palette
            .raw_ids
            .get(&v.tile)
            .and_then(|tile| scene.palette.groups.get(tile))
            .is_some_and(|g| g.category == TerrainCategory::Cliff && g.id == group);
        if !matched {
            continue;
        }
        if let Some(stamp) = cliff_cells(doc, scene, at)? {
            selected.extend(stamp.cells.into_iter().map(|(at, _)| at));
        }
        if connected {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let (Ok(x), Ok(y)) = (
                        u16::try_from(i32::from(at[0]) + dx),
                        u16::try_from(i32::from(at[1]) + dy),
                    ) {
                        queue.push_back([x, y]);
                    }
                }
            }
        }
    }
    Ok(Some(selected))
}

/// A cliff click samples its top even when the picked subtile is the lower face.
pub fn sample(doc: &MapDocument, scene: &MapScene, at: Cell) -> Result<u8> {
    doc.check_cell(at)?;
    let v = value(doc, scene, at);
    let level = if let Some((relative, top)) = cliff_geometry(scene, v)? {
        v.level
            .checked_sub(relative)
            .and_then(|base| base.checked_add(top))
            .context("悬崖子块高度无效，请先使用修正悬崖高度工具")?
    } else {
        v.level
    };
    ensure!(level <= 127, "仅支持 0～127 层高度");
    Ok(level)
}

/// Explicit low-ground normalization for old, independently painted cliff
/// cells. Only their heights change; no complete-template topology is required.
pub fn repair(
    doc: &MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    base: u8,
) -> Result<PlateauPlan> {
    ensure!(base <= 127, "低地高度须在 0～127 层");
    let mut edits = BTreeMap::new();
    let mut found = false;
    for &at in selected {
        doc.check_cell(at)?;
        let old = value(doc, scene, at);
        let Some((relative, _)) = cliff_geometry(scene, old)? else {
            continue;
        };
        found = true;
        let level = base
            .checked_add(relative)
            .filter(|&level| level <= 127)
            .context("修正后的悬崖高度越过 0～127，未修改地形")?;
        if level == old.level {
            continue;
        }
        ensure!(
            doc.overlay(at).0 == 255,
            "{},{} 有覆盖物占用，未修正悬崖",
            at[0],
            at[1]
        );
        edits.insert(at, TerrainValue { level, ..old });
    }
    ensure!(found, "所选区域没有悬崖子块");
    for (_, record) in doc
        .record_iter()
        .filter(|(_, r)| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in crate::placement::footprint(scene, record)? {
            ensure!(
                !edits.contains_key(&at),
                "{},{} 有对象或建筑占地，未修正悬崖",
                at[0],
                at[1]
            );
        }
    }
    Ok(PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior: BTreeSet::new(),
        base,
    })
}

/// Raw raise/lower shifts all in-map subtiles of a cliff by the same amount.
pub(crate) fn raw(
    doc: &MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    mode: TerrainMode,
    level: u8,
    change: terrain_tools::HeightChange,
) -> Result<BTreeMap<Cell, TerrainValue>> {
    let mut edits = BTreeMap::new();
    for &at in selected {
        doc.check_cell(at)?;
        if edits.contains_key(&at) {
            continue;
        }
        // A flat-ground brush must not erase a cliff's relative subtile heights.
        // Explicit cliff normalization belongs to the repair tool.
        if mode == TerrainMode::Flatten && cliff_geometry(scene, value(doc, scene, at))?.is_some() {
            continue;
        }
        if matches!(mode, TerrainMode::Raise | TerrainMode::Lower)
            && let Some(stamp) = cliff_cells(doc, scene, at)?
        {
            let top = i16::from(sample(doc, scene, at)?);
            let delta = match change {
                terrain_tools::HeightChange::Step(amount) => {
                    if mode == TerrainMode::Raise {
                        i16::from(amount)
                    } else {
                        -i16::from(amount)
                    }
                }
                terrain_tools::HeightChange::To(_) => {
                    i16::from(change.target(mode, top as u8)) - top
                }
            };
            ensure!(
                (0..=127).contains(&(top + delta)),
                "整块悬崖的崖顶将越过 0～127 层，未修改地形"
            );
            // A cropped toe may leave only high subtiles, but its implied
            // base still needs to fit the engine's height range.
            ensure!(
                i16::from(stamp.base) + delta >= 0,
                "整块悬崖升降将越过 0～127 层，未修改地形"
            );
            for (p, old) in stamp.cells {
                let next = i16::from(old.level) + delta;
                ensure!(
                    (0..=127).contains(&next),
                    "整块悬崖升降将越过 0～127 层，未修改地形"
                );
                edits.insert(
                    p,
                    TerrainValue {
                        level: next as u8,
                        ..old
                    },
                );
            }
            continue;
        }
        let old = value(doc, scene, at);
        let next = match mode {
            TerrainMode::Raise | TerrainMode::Lower => change.target(mode, old.level),
            _ => level,
        }
        .min(127);
        edits.insert(at, TerrainValue { level: next, ..old });
    }
    if matches!(mode, TerrainMode::Raise | TerrainMode::Lower) {
        crate::road_tools::HeightRoads::new(scene).validate_raw_levels(doc, scene, &edits)?;
    }
    Ok(edits)
}

fn ground(scene: &MapScene, v: TerrainValue) -> bool {
    let tile = scene.palette.raw_ids.get(&v.tile).copied().or_else(|| {
        matches!(v.tile, -1 | 65535)
            .then(|| {
                scene
                    .palette
                    .tile_ids
                    .and_then(|ids| u16::try_from(ids.clear).ok())
            })
            .flatten()
    });
    tile.is_some_and(|tile| {
        scene
            .palette
            .groups
            .get(&tile)
            .is_some_and(|g| g.category == TerrainCategory::Ground)
            && scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
                t.width == 1
                    && t.height == 1
                    && t.cells
                        .get(usize::from(v.sub))
                        .and_then(|s| *s)
                        .is_some_and(|s| s.height == 0 && s.slope == 0)
            })
    })
}

/// Fill one enclosed four-connected, flat, same-level component. Neither cliffs
/// nor ramps are rewritten, and an open contour cannot flood the map exterior.
pub fn fill(doc: &MapDocument, scene: &MapScene, seed: Cell, target: u8) -> Result<PlateauPlan> {
    doc.check_cell(seed)?;
    ensure!(target <= 127, "目标高度须在 0～127 层");
    let source = value(doc, scene, seed);
    ensure!(
        ground(scene, source),
        "请点击悬崖内侧的平坦地表；悬崖、坡道、道路和水面不会填高"
    );
    if source.level == target {
        return Ok(PlateauPlan {
            edits: BTreeMap::new(),
            overlays: BTreeMap::new(),
            cliffs: vec![],
            interior: BTreeSet::from([seed]),
            base: target,
        });
    }
    let mut queue = VecDeque::from([seed]);
    let mut seen = BTreeSet::new();
    let mut interior = BTreeSet::new();
    let mut edits = BTreeMap::new();
    while let Some(at) = queue.pop_front() {
        if !seen.insert(at) {
            continue;
        }
        ensure!(
            doc.check_cell(at).is_ok(),
            "区域通向地图边缘，请先闭合悬崖或高度边界，未填高地形"
        );
        let old = value(doc, scene, at);
        if old.level != source.level || !ground(scene, old) {
            continue;
        }
        interior.insert(at);
        if old.level != target {
            ensure!(
                crate::bridge_tools::height_editable_overlay(doc, scene, at),
                "{},{} 有覆盖物占用，未填高地形",
                at[0],
                at[1]
            );
            edits.insert(
                at,
                TerrainValue {
                    level: target,
                    ..old
                },
            );
        }
        let neighbors: Vec<_> = terrain_tools::neighbors(at).collect();
        ensure!(
            neighbors.len() == 4,
            "区域通向地图边缘，请先闭合悬崖或高度边界，未填高地形"
        );
        queue.extend(neighbors);
    }
    for (_, record) in doc
        .record_iter()
        .filter(|(_, r)| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in crate::placement::footprint(scene, record)? {
            ensure!(
                !edits.contains_key(&at),
                "{},{} 有对象或建筑占地，未填高地形",
                at[0],
                at[1]
            );
        }
    }
    Ok(PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior,
        base: target,
    })
}

#[cfg(test)]
#[path = "height_tools_tests.rs"]
mod tests;
