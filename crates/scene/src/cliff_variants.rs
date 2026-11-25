//! Select visual siblings after the cliff compiler has fixed the occupied cells.
use crate::{
    cliff_tools::{CliffStamp, PlateauPlan},
    coastal_cliffs,
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Result, ensure};
use ra2_map_assets::catalog::{TerrainSubtile, TerrainTemplate};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, HashMap};
use vera20k_core::map::theater::TileKey;

// One-based CliffSet piece numbers. The four straight families have a
// double-length template in three styles, plus a narrow piece for seams.
const STRAIGHT: [(u16, [i16; 2], [u16; 3]); 4] = [
    (8, [1, 0], [5, 6, 7]),
    (18, [0, 1], [15, 16, 17]),
    (26, [1, 0], [23, 24, 25]),
    (38, [0, 1], [35, 36, 37]),
];

fn offset(at: Cell, [dx, dy]: [i16; 2]) -> Option<Cell> {
    Some([at[0].checked_add_signed(dx)?, at[1].checked_add_signed(dy)?])
}

fn footprint(
    t: &TerrainTemplate,
    origin: Cell,
) -> impl Iterator<Item = (usize, Cell, TerrainSubtile)> + Clone {
    t.cells.iter().enumerate().filter_map(move |(sub, cell)| {
        let cell = (*cell)?;
        let x = (sub % usize::from(t.width)) as u16;
        let y = (sub / usize::from(t.width)) as u16;
        Some((sub, [origin[0] + x, origin[1] + y], cell))
    })
}

fn metadata_at(t: &TerrainTemplate, origin: Cell, at: Cell) -> Option<TerrainSubtile> {
    let x = at[0].checked_sub(origin[0])?;
    let y = at[1].checked_sub(origin[1])?;
    if x >= t.width || y >= t.height {
        return None;
    }
    *t.cells
        .get(usize::from(y) * usize::from(t.width) + usize::from(x))?
}

fn matches_footprint(
    source: &TerrainTemplate,
    target: &TerrainTemplate,
    origin: Cell,
    next: Option<Cell>,
) -> bool {
    if target.width == 0
        || target.height == 0
        || usize::from(target.width) * usize::from(target.height) != target.cells.len()
        || origin[0].checked_add(target.width - 1).is_none()
        || origin[1].checked_add(target.height - 1).is_none()
    {
        return false;
    }
    let count = source.cells.iter().flatten().count() * if next.is_some() { 2 } else { 1 };
    count != 0
        && target.cells.iter().flatten().count() == count
        && footprint(target, origin).all(|(_, at, cell)| {
            metadata_at(source, origin, at)
                .or_else(|| next.and_then(|origin| metadata_at(source, origin, at)))
                .is_some_and(|old| {
                    (old.height, old.slope, old.terrain) == (cell.height, cell.slope, cell.terrain)
                })
        })
}

pub(crate) fn available(scene: &MapScene, tile: u16) -> bool {
    scene.catalog.raw_tiles.contains_key(&tile)
        && scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
            t.cells.iter().enumerate().all(|(sub, cell)| {
                cell.is_none()
                    || u8::try_from(sub).is_ok_and(|sub_tile| {
                        scene.palette.images.contains_key(&TileKey {
                            tile_id: tile,
                            sub_tile,
                            variant: 0,
                        })
                    })
            })
        })
}

/// Recognize a wide stamp only when the current theater's narrow TMPs really
/// reconstruct its two logical anchors and all of its occupied cells.
pub(crate) fn narrow_halves(scene: &MapScene, stamp: &CliffStamp) -> Option<[CliffStamp; 2]> {
    let (index, water) = coastal_cliffs::piece(scene, stamp.tile)?;
    let (narrow, step, _) = STRAIGHT
        .iter()
        .find(|(_, _, wide)| wide.contains(&(index + 1)))?;
    if water && !coastal_cliffs::supports(scene, narrow - 1) {
        return None;
    }
    let tile = (stamp.tile - index).checked_add(narrow - 1)?;
    if !coastal_cliffs::same_tileset(scene, stamp.tile, tile) {
        return None;
    }
    let second = offset(stamp.origin, *step)?;
    let wide = scene.palette.templates.get(&(stamp.tile, 0))?;
    let source = scene.palette.templates.get(&(tile, 0))?;
    matches_footprint(source, wide, stamp.origin, Some(second)).then(|| {
        [stamp.origin, second].map(|origin| CliffStamp {
            tile,
            origin,
            base: stamp.base,
        })
    })
}

pub(crate) fn style_score(origin: Cell, tile: u16) -> u64 {
    // SplitMix64 finalizer provides a stable coordinate hash across sessions.
    let mut n = u64::from(origin[0]) | (u64::from(origin[1]) << 16) | (u64::from(tile) << 32);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}

fn pick_style(
    candidates: [Option<u16>; 3],
    origin: Cell,
    tangent: Option<[i16; 2]>,
) -> Option<u16> {
    let ids = candidates.into_iter().flatten();
    let best_at = |at| ids.clone().min_by_key(|&tile| style_score(at, tile));
    let Some(step) = tangent else {
        return best_at(origin);
    };
    let axis = usize::from(step[1] != 0);
    let even_pair = (origin[axis] / 2).is_multiple_of(2);
    if ids.clone().count() == 2 {
        // With only two styles, alternate using a stable per-line phase.
        let mut line = origin;
        line[axis] = 0;
        return ids.min_by_key(|&tile| {
            let rank = style_score(line, tile);
            if even_pair { rank } else { !rank }
        });
    }
    if even_pair {
        return best_at(origin);
    }
    // Even pairs choose only from their own hash. Odd pairs exclude the
    // choices their two even neighbours would make, even if absent today.
    let before = offset(origin, step.map(|d| -2 * d)).and_then(best_at);
    let after = offset(origin, step.map(|d| 2 * d)).and_then(best_at);
    ids.clone()
        .filter(|&tile| Some(tile) != before && Some(tile) != after)
        .min_by_key(|&tile| style_score(origin, tile))
        .or_else(|| best_at(origin))
}

pub(crate) fn apply(doc: &MapDocument, scene: &MapScene, plan: &mut PlateauPlan) {
    let stamps = std::mem::take(&mut plan.cliffs);
    let mut pending: HashMap<_, _> = stamps.iter().map(|s| (s.origin, s)).collect();
    plan.cliffs.reserve(stamps.len());
    for stamp in &stamps {
        if pending.remove(&stamp.origin).is_none() {
            continue;
        }
        let mut tile = stamp.tile;
        if let Some((index, water)) = coastal_cliffs::piece(scene, stamp.tile) {
            let piece = index + 1;
            let set = stamp.tile - index;
            let mut next = None;
            let mut tangent = None;
            let siblings: &[u16] = match piece {
                1 | 3 | 4 => &[1, 3, 4],
                9..=11 => &[9, 10, 11],
                12..=14 => &[12, 13, 14],
                19..=21 => &[19, 20, 21],
                29 | 30 => &[29, 30],
                _ => {
                    // Pair on a fixed grid so dragging past either end does
                    // not shift the boundaries of all previously fitted pairs.
                    if (stamp.origin[0] + stamp.origin[1]).is_multiple_of(2)
                        && let Some((_, step, wide)) =
                            STRAIGHT.iter().find(|(narrow, _, _)| *narrow == piece)
                        && let Some(other) = offset(stamp.origin, *step)
                        && let Some(neighbor) = pending.get(&other)
                        && neighbor.tile == stamp.tile
                        && neighbor.base == stamp.base
                    {
                        next = Some(other);
                        tangent = Some(*step);
                        wide
                    } else {
                        &[]
                    }
                }
            };
            let mut candidates = [None; 3];
            for (slot, tile) in candidates.iter_mut().zip(
                siblings
                    .iter()
                    .filter(|&&p| !water || coastal_cliffs::supports(scene, p - 1))
                    .filter_map(|&p| set.checked_add(p - 1))
                    .filter(|&tile| {
                        coastal_cliffs::same_tileset(scene, stamp.tile, tile)
                            && available(scene, tile)
                            && matches_footprint(
                                &scene.palette.templates[&(stamp.tile, 0)],
                                &scene.palette.templates[&(tile, 0)],
                                stamp.origin,
                                next,
                            )
                    }),
            ) {
                *slot = Some(tile);
            }
            if let Some(chosen) = pick_style(candidates, stamp.origin, tangent) {
                tile = chosen;
                if let Some(other) = next {
                    pending.remove(&other);
                }
            }
        }
        if tile != stamp.tile {
            let template = &scene.palette.templates[&(tile, 0)];
            let raw = scene.catalog.raw_tiles[&tile];
            for (sub, at, _) in footprint(template, stamp.origin) {
                let old = plan
                    .edits
                    .get(&at)
                    .copied()
                    .unwrap_or_else(|| value(doc, scene, at));
                plan.edits.insert(
                    at,
                    TerrainValue {
                        tile: raw,
                        sub: sub as u8,
                        ..old
                    },
                );
            }
        }
        plan.cliffs.push(CliffStamp { tile, ..*stamp });
    }
}

/// Restore the untouched half of a straight double-length cliff when a
/// bridge or ramp replaces the other half. Other partial trims stay invalid.
pub(crate) fn split_remainder(
    doc: &MapDocument,
    scene: &MapScene,
    stamp: CliffStamp,
    edits: &BTreeMap<Cell, TerrainValue>,
) -> Result<Option<CliffStamp>> {
    let Some(halves) = narrow_halves(scene, &stamp) else {
        return Ok(None);
    };
    let tile = halves[0].tile;
    if !available(scene, tile) {
        return Ok(None);
    }
    let template = &scene.palette.templates[&(tile, 0)];
    let mut remainder = None;
    for half in halves {
        let cells = footprint(template, half.origin).filter(|&(_, at, _)| doc.contains_cell(at));
        if cells.clone().all(|(_, at, _)| edits.contains_key(&at)) {
            continue;
        }
        if remainder.is_some() || cells.clone().any(|(_, at, _)| edits.contains_key(&at)) {
            return Ok(None);
        }
        for (_, at, meta) in cells {
            ensure!(
                meta.height.checked_add(half.base) == Some(value(doc, scene, at).level),
                "原有悬崖模板高度不完整"
            );
        }
        remainder = Some(half);
    }
    Ok(remainder)
}
