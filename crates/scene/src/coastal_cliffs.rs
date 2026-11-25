//! Match compatible front cliff pieces to water without changing their geometry.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    cliff_variants,
    map::MapScene,
    shore_tools::{id, offset, protect, wet},
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn same_tileset(scene: &MapScene, a: u16, b: u16) -> bool {
    scene
        .palette
        .groups
        .get(&a)
        .zip(scene.palette.groups.get(&b))
        .is_some_and(|(a, b)| a.id == b.id)
}

/// WCliff24/25 join a vertical cliff to a flat shore in Tsunami. Recognize
/// them here only to recover complete pieces from older ramp experiments.
pub(crate) fn legacy_ramp_end(scene: &MapScene, outward: [i32; 2]) -> Option<u16> {
    let (index, size) = match outward {
        [0, 1] => (23, [1, 3]),
        [1, 0] => (24, [3, 1]),
        _ => return None,
    };
    let base = scene.palette.tile_ids?.cliffs.water_cliffs?;
    let tile = base.checked_add(index)?;
    if !same_tileset(scene, base, tile) || !cliff_variants::available(scene, tile) {
        return None;
    }
    let t = &scene.palette.templates[&(tile, 0)];
    ([t.width, t.height] == size
        && t.cells.len() == 3
        && t.cells.iter().zip([15, 10, 10]).all(|(cell, terrain)| {
            cell.is_some_and(|m| (m.height, m.slope, m.terrain) == (0, 0, terrain))
        }))
    .then_some(tile)
}

/// WaterCliffs only shares the first 22 orientations with CliffSet. Later
/// entries are caves/shore joins, not the land set's back faces.
pub(crate) fn piece(scene: &MapScene, tile: u16) -> Option<(u16, bool)> {
    let ids = scene.palette.tile_ids?;
    if let Some(base) = ids.cliffs.cliff_set
        && let Some(index) = tile.checked_sub(base).filter(|&index| index < 40)
        && same_tileset(scene, base, tile)
    {
        return Some((index, false));
    }
    if let Some(base) = ids.cliffs.water_cliffs
        && let Some(index) = tile.checked_sub(base).filter(|&index| index < 22)
        && supports(scene, index)
    {
        return Some((index, true));
    }
    None
}
/// Some theaters declare WaterCliffs but point it at unrelated templates.
pub fn supports(scene: &MapScene, index: u16) -> bool {
    if index >= 22 {
        return false;
    }
    let Some(ids) = scene.palette.tile_ids else {
        return false;
    };
    let (Some(land), Some(water)) = (ids.cliffs.cliff_set, ids.cliffs.water_cliffs) else {
        return false;
    };
    let (Some(land_tile), Some(water_tile)) = (land.checked_add(index), water.checked_add(index))
    else {
        return false;
    };
    if !same_tileset(scene, land, land_tile) || !same_tileset(scene, water, water_tile) {
        return false;
    }
    let (Some(a), Some(b)) = (
        scene.palette.templates.get(&(land_tile, 0)),
        scene.palette.templates.get(&(water_tile, 0)),
    ) else {
        return false;
    };
    a.width == b.width
        && a.height == b.height
        && a.cells.len() == b.cells.len()
        && a.cells.iter().zip(&b.cells).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => a.height == b.height && a.slope == b.slope,
            (None, None) => true,
            _ => false,
        })
}
pub(crate) fn adapt(doc: &MapDocument, scene: &MapScene, p: &mut PlateauPlan) -> Result<()> {
    adapt_inner(doc, scene, p, false)
}
fn adapt_inner(
    doc: &MapDocument,
    scene: &MapScene,
    p: &mut PlateauPlan,
    strict: bool,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let land = ids.cliffs.cliff_set.context("缺少陆地悬崖")?;
    let target = |index, water| {
        if water {
            ids.cliffs
                .water_cliffs
                .and_then(|base| base.checked_add(index))
                .filter(|_| supports(scene, index))
        } else {
            land.checked_add(index)
        }
        .filter(|&tile| cliff_variants::available(scene, tile))
    };
    let mut cliffs = Vec::with_capacity(p.cliffs.len());
    for s in &p.cliffs {
        let Some((index, _)) = piece(scene, s.tile).filter(|(index, _)| *index < 22) else {
            cliffs.push(s.clone());
            continue;
        };
        if let Some(halves) = cliff_variants::narrow_halves(scene, s) {
            let [first, second] = [
                touches_water(doc, scene, &p.edits, &halves[0]),
                touches_water(doc, scene, &p.edits, &halves[1]),
            ];
            if first != second {
                // At a shoreline boundary, converting the wide tile as a
                // whole would also repaint the half whose foot stays dry.
                let narrow_index = halves[0].tile - (s.tile - index);
                let targets = [first, second].map(|water| target(narrow_index, water));
                if let [Some(a), Some(b)] = targets {
                    for (mut half, tile) in halves.into_iter().zip([a, b]) {
                        half.tile = tile;
                        let cells = cliff_authoring::stamp_clipped(
                            doc,
                            scene,
                            tile,
                            half.origin,
                            half.base,
                        )?;
                        if !cells.is_empty() {
                            p.edits.extend(cells);
                            cliffs.push(half);
                        }
                    }
                    continue;
                }
                ensure!(!strict, "当前环境缺少拆分临水悬崖所需的窄模板");
                cliffs.push(s.clone());
                continue;
            }
        }
        let water = touches_water(doc, scene, &p.edits, s);
        let Some(tile) = target(index, water) else {
            ensure!(
                !strict,
                "当前环境缺少与第 {} 块悬崖兼容的水悬崖模板",
                index + 1
            );
            cliffs.push(s.clone());
            continue;
        };
        if tile != s.tile {
            p.edits.extend(cliff_authoring::stamp_clipped(
                doc, scene, tile, s.origin, s.base,
            )?);
        }
        cliffs.push(CliffStamp { tile, ..*s });
    }
    p.cliffs = cliffs;
    Ok(())
}

fn touches_water(
    doc: &MapDocument,
    scene: &MapScene,
    edits: &BTreeMap<Cell, TerrainValue>,
    s: &CliffStamp,
) -> bool {
    let template = &scene.palette.templates[&(s.tile, 0)];
    for (sub, meta) in template.cells.iter().enumerate() {
        if meta.is_none_or(|m| m.height != 0) {
            continue;
        }
        let Some(at) = offset(
            s.origin,
            sub as i32 % i32::from(template.width),
            sub as i32 / i32::from(template.width),
        )
        .filter(|&at| doc.contains_cell(at)) else {
            continue;
        };
        if std::iter::once(at)
            .chain(crate::terrain_tools::neighbors(at))
            .any(|n| {
                doc.contains_cell(n) && {
                    let v = edits
                        .get(&n)
                        .copied()
                        .unwrap_or_else(|| value(doc, scene, n));
                    v.level == s.base && wet(scene, v)
                }
            })
        {
            return true;
        }
    }
    false
}
pub(crate) fn gather(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
) -> Result<Vec<CliffStamp>> {
    let mut stamps = BTreeSet::new();
    for &at in selection {
        doc.check_cell(at)?;
        let v = value(doc, scene, at);
        let Some(tile) = id(scene, v).filter(|&t| piece(scene, t).is_some_and(|(i, _)| i < 22))
        else {
            continue;
        };
        let t = scene
            .palette
            .templates
            .get(&(tile, 0))
            .context("悬崖模板缺失")?;
        let origin = offset(
            at,
            -(i32::from(v.sub) % i32::from(t.width)),
            -(i32::from(v.sub) / i32::from(t.width)),
        )
        .context("悬崖越界")?;
        let base = v
            .level
            .checked_sub(
                t.cells
                    .get(usize::from(v.sub))
                    .and_then(|s| *s)
                    .context("悬崖子块无效")?
                    .height,
            )
            .context("悬崖高度无效")?;
        stamps.insert((tile, origin, base));
    }
    stamps
        .into_iter()
        .map(|(tile, origin, base)| {
            let cells = cliff_authoring::stamp_clipped(doc, scene, tile, origin, base)?;
            ensure!(
                cells.iter().all(|(&at, &v)| value(doc, scene, at) == v),
                "已有悬崖模板不完整，请先修复该模板"
            );
            Ok(CliffStamp { tile, origin, base })
        })
        .collect()
}
pub(crate) fn barrier(scene: &MapScene, v: ra2_map_document::document::TerrainValue) -> bool {
    id(scene, v).is_some_and(|t| {
        piece(scene, t).is_some()
            && scene
                .palette
                .templates
                .get(&(t, 0))
                .and_then(|t| t.cells.get(usize::from(v.sub)))
                .and_then(|s| *s)
                .is_some_and(|s| s.height == 0)
    })
}
pub fn plan(doc: &MapDocument, scene: &MapScene, selection: BTreeSet<Cell>) -> Result<PlateauPlan> {
    let stamps = gather(doc, scene, &selection)?;
    ensure!(
        !stamps.is_empty(),
        "请选择标准正面悬崖；水悬崖不包含陆地背面方向"
    );
    let mut p = PlateauPlan {
        edits: BTreeMap::new(),
        overlays: BTreeMap::new(),
        cliffs: Vec::new(),
        interior: BTreeSet::new(),
        base: 0,
    };
    p.cliffs = stamps;
    adapt_inner(doc, scene, &mut p, true)?;
    p.edits.retain(|&at, v| *v != value(doc, scene, at));
    protect(doc, scene, &p.edits)?;
    Ok(p)
}
