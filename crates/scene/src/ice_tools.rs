//! Paint VERA's bundled ice and the water-side tiles that finish its edges.
use crate::{
    cliff_variants::style_score, fill_tools::FillPlan, map::MapScene, terrain_tools::value,
};
use anyhow::{Context, Result};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::map::theater::TileKey;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub mark_growth: bool,
    pub soften_shore: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mark_growth: true,
            soften_shore: true,
        }
    }
}

const SIDES: [(i16, i16); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
// Edge_Ice's bit order starts at north-east and moves clockwise.
const EDGE_NEIGHBORS: [(i16, i16); 8] = [
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
];
// VERA's Edge_Ice mask-to-piece table (Firestorm 2.0 0x6D3C04).
// -1 has no ice edge, -2 selects piece 15; the pieces start at tile offset 16.
#[rustfmt::skip]
const WATER_EDGE_PIECE: [i8; 256] = [
    -1, 33, 2, 2, 34, 37, 2, 2, 4, 26, 6, 6, 4, 26, 6, 6,
    35, 45, 17, 17, 38, 41, 17, 17, 4, 26, 6, 6, 4, 26, 6, 6,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    32, 36, 25, 25, 44, 40, 25, 25, 19, 30, 20, 20, 19, 30, 20, 20,
    39, 43, 29, 29, 42, 46, 29, 29, 19, 30, 20, 20, 19, 30, 20, 20,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    1, 1, 3, 3, 16, 16, 3, 3, 5, 5, 7, 7, 5, 5, 7, 7,
    24, 24, 18, 18, 28, 28, 18, 18, 5, 5, 7, 7, 5, 5, 7, 7,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    1, 1, 3, 3, 16, 16, 3, 3, 5, 5, 7, 7, 5, 5, 7, 7,
    24, 24, 18, 18, 28, 28, 18, 18, 5, 5, 7, 7, 5, 5, 7, 7,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
];

fn offset([x, y]: Cell, (dx, dy): (i16, i16)) -> Cell {
    [x.wrapping_add_signed(dx), y.wrapping_add_signed(dy)]
}

fn neighbor_mask<const N: usize>(
    at: Cell,
    steps: [(i16, i16); N],
    hits: impl Fn(Cell) -> bool,
) -> usize {
    steps.into_iter().enumerate().fold(0, |mask, (bit, step)| {
        mask | (usize::from(hits(offset(at, step))) << bit)
    })
}

fn ice_tile(scene: &MapScene, sets: [u16; 3], cell: TerrainValue) -> Option<(u16, u16)> {
    if cell.sub != 0 {
        return None;
    }
    let tile = *scene.palette.raw_ids.get(&cell.tile)?;
    sets.into_iter().find_map(|base| {
        let offset = tile.checked_sub(base)?;
        (offset < 64).then_some((base, offset))
    })
}

pub(crate) fn bundled_tile(scene: &MapScene, cell: TerrainValue) -> bool {
    scene
        .palette
        .bundled_ice_sets
        .is_some_and(|sets| ice_tile(scene, sets, cell).is_some())
}

fn raw_ice_piece(scene: &MapScene, tile_id: u16) -> Option<i32> {
    let key = TileKey {
        tile_id,
        sub_tile: 0,
        variant: 0,
    };
    scene
        .catalog
        .raw_tiles
        .get(&tile_id)
        .copied()
        .filter(|_| scene.palette.images.contains_key(&key))
}

fn water_edge_raw(scene: &MapScene, sets: [u16; 3], mut at: Cell, piece: u16) -> Option<i32> {
    // Ice1 and Ice2 are the two styles the engine preserves when re-edging
    // the same shape and checks for ice growth. Keep the piece in either set.
    let [first, second, _] = sets;
    let second_in_pair = match piece {
        // Straight runs use different textures within each pair and a hashed
        // order between pairs. This breaks long runs without strict alternation.
        17 | 20 => Some(&mut at[0]), // ice to the north or south
        18 | 24 => Some(&mut at[1]), // ice to the east or west
        _ => None,
    }
    .is_some_and(|along| {
        let odd = !along.is_multiple_of(2);
        *along &= !1;
        odd
    });
    let flip = (style_score(at, piece) & 1 != 0) ^ second_in_pair;
    let bases = if flip {
        [second, first]
    } else {
        [first, second]
    };
    bases
        .into_iter()
        .find_map(|base| raw_ice_piece(scene, base + piece))
}

fn flat_water(scene: &MapScene, cell: TerrainValue) -> bool {
    // Stock snow has 2x2 open-water art. A flat deep-water subtile can be
    // replaced independently; shallow crossings in the same tileset cannot.
    crate::shore_tools::flat_water(scene, cell)
        && scene
            .palette
            .raw_ids
            .get(&cell.tile)
            .and_then(|&tile| scene.palette.templates.get(&(tile, 0)))
            .and_then(|template| template.cells.get(usize::from(cell.sub)))
            .and_then(|sub| *sub)
            .is_some_and(|sub| sub.terrain == 9)
}

fn occupied_cells(doc: &MapDocument, scene: &MapScene) -> BTreeSet<Cell> {
    doc.record_iter()
        .filter(|(_, record)| !matches!(record.section.as_str(), "Waypoints" | "Smudge"))
        .flat_map(|(_, record)| {
            crate::placement::footprint(scene, record)
                .unwrap_or_else(|_| record.cell().into_iter().collect())
        })
        .collect()
}

fn smooth_open(scene: &MapScene, sets: [u16; 3], v: TerrainValue) -> bool {
    flat_water(scene, v) || matches!(ice_tile(scene, sets, v), Some((_, 16..=63)))
}

fn smooth_piece(mask: usize) -> u16 {
    // Mask 15 is the cracked tile, not a solid ice shape.
    if (1..15).contains(&mask) {
        1 + mask as u16
    } else {
        0
    }
}

fn water_piece(mask: usize) -> Option<u16> {
    match WATER_EDGE_PIECE[mask] {
        -1 => None,
        -2 => Some(31),
        piece => Some(16 + piece as u16),
    }
}

fn beach(doc: &MapDocument, scene: &MapScene, at: Cell, level: u8) -> bool {
    if !doc.contains_cell(at) {
        return false;
    }
    let v = value(doc, scene, at);
    v.level == level
        && crate::shore_tools::id(scene, v)
            .filter(|&id| crate::shore_tools::shore(scene, id))
            .and_then(|id| scene.palette.templates.get(&(id, 0)))
            .and_then(|t| t.cells.get(usize::from(v.sub)))
            .and_then(|sub| *sub)
            .is_some_and(|sub| matches!(sub.terrain, 9 | 10) && sub.slope == 0 && sub.height == 0)
}

fn touches_beach(doc: &MapDocument, scene: &MapScene, at: Cell, level: u8) -> bool {
    SIDES
        .into_iter()
        .any(|step| beach(doc, scene, offset(at, step), level))
}

fn shore_band(
    doc: &MapDocument,
    scene: &MapScene,
    sets: [u16; 3],
    painted: &BTreeSet<Cell>,
) -> BTreeSet<Cell> {
    let at_level = |at, level| {
        doc.contains_cell(at)
            .then(|| value(doc, scene, at))
            .filter(|v| v.level == level)
    };
    let rim = |at, level| {
        // Count earlier fringe in the width check so a small touch-up doesn't
        // punch a solid hole in an existing transition.
        at_level(at, level).is_some_and(|v| {
            (painted.contains(&at) || ice_tile(scene, sets, v).is_some())
                && touches_beach(doc, scene, at, level)
        })
    };
    let core = |at, level| {
        at_level(at, level).is_some_and(|v| {
            (painted.contains(&at) || matches!(ice_tile(scene, sets, v), Some((_, 0..=15))))
                && !touches_beach(doc, scene, at, level)
        })
    };
    let wide_rim = |at, level| {
        // Keep the bank connection on one- or two-cell ice lanes.
        rim(at, level)
            && SIDES.into_iter().any(|inward @ (dx, dy)| {
                let backed = |n: i16| {
                    let cell = offset(at, (dy * n, dx * n));
                    (n == 0 || rim(cell, level)) && core(offset(cell, inward), level)
                };
                if !backed(0) {
                    return false;
                }
                let before = backed(-1);
                let after = backed(1);
                (before && (after || backed(-2))) || (after && backed(2))
            })
    };
    let rounds_corner = |at, level| {
        // A convex corner can be backed diagonally. Follow a wide run around
        // the corner for one cell, leaving the core solid.
        std::iter::once(at)
            .chain(SIDES.map(|step| offset(at, step)))
            .filter(|&corner| rim(corner, level))
            .any(|corner| {
                [-1, 1].into_iter().any(|dx| {
                    [-1, 1].into_iter().any(|dy| {
                        let a = offset(corner, (dx, 0));
                        let b = offset(corner, (0, dy));
                        (at == corner || at == a || at == b)
                            && core(offset(corner, (dx, dy)), level)
                            && rim(a, level)
                            && rim(b, level)
                            && (wide_rim(a, level) || wide_rim(b, level))
                    })
                })
            })
    };
    let stair_tip = |corner, (dx, dy): (i16, i16), level| {
        rim(corner, level)
            && beach(doc, scene, offset(corner, (-dx, 0)), level)
            && beach(doc, scene, offset(corner, (0, -dy)), level)
            && rim(offset(corner, (dx, 0)), level)
            && rim(offset(corner, (0, dy)), level)
            && [(dx, 0), (0, dy)].into_iter().any(|(ix, iy)| {
                core(offset(corner, (2 * ix, 2 * iy)), level)
                    && (2..=3).all(|depth| {
                        (-1..=1).all(|side| {
                            let p =
                                offset(corner, (depth * ix + side * iy, depth * iy + side * ix));
                            core(p, level) || rim(p, level)
                        })
                    })
            })
    };
    let rounds_step = |at, level| {
        // Staircases have short convex corners with no straight run beside
        // them. Round the rim of each 2×2 tip backed by a three-cell-wide shelf.
        EDGE_NEIGHBORS.into_iter().step_by(2).any(|(dx, dy)| {
            [(0, 0), (-dx, 0), (0, -dy), (-dx, -dy)]
                .into_iter()
                .any(|step| stair_tip(offset(at, step), (dx, dy), level))
        })
    };
    painted
        .iter()
        .copied()
        .filter(|&at| {
            let level = value(doc, scene, at).level;
            rim(at, level)
                && (wide_rim(at, level) || rounds_corner(at, level) || rounds_step(at, level))
        })
        .collect()
}

pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    selected: BTreeSet<Cell>,
    options: Options,
) -> Result<FillPlan> {
    let sets = scene
        .palette
        .bundled_ice_sets
        .context("当前地图未加载 VERA 可破裂冰面素材")?;
    let mut result = FillPlan {
        edits: BTreeMap::new(),
        cells: BTreeSet::new(),
        skipped: BTreeSet::new(),
    };
    let occupied = occupied_cells(doc, scene);
    for at in selected
        .into_iter()
        .filter(|&at| doc.check_cell(at).is_ok())
    {
        let old = value(doc, scene, at);
        // Protected water must not count toward the ice being added.
        if doc.overlay(at).0 == 255
            && !occupied.contains(&at)
            && (flat_water(scene, old) || ice_tile(scene, sets, old).is_some())
        {
            result.cells.insert(at);
        } else {
            result.skipped.insert(at);
        }
    }

    let band = if options.soften_shore {
        shore_band(doc, scene, sets, &result.cells)
    } else {
        BTreeSet::new()
    };
    result.cells.retain(|at| !band.contains(at));
    let mut affected = result.cells.clone();
    affected.extend(&band);
    affected.extend(
        result
            .cells
            .iter()
            .chain(&band)
            .flat_map(|&at| EDGE_NEIGHBORS.map(|step| offset(at, step))),
    );
    for at in affected.into_iter().filter(|&at| {
        doc.check_cell(at).is_ok()
            && doc.overlay(at).0 == 255
            && !occupied.contains(&at)
            && !result.skipped.contains(&at)
    }) {
        let old = value(doc, scene, at);
        let existing = ice_tile(scene, sets, old);
        let selected = result.cells.contains(&at);
        let on_band = band.contains(&at);
        let neighbor = |cell| {
            doc.contains_cell(cell)
                .then(|| value(doc, scene, cell))
                .filter(|v| v.level == old.level)
        };
        let piece = if !on_band && (selected || matches!(existing, Some((_, 0..=15)))) {
            if !selected
                && !SIDES.into_iter().any(|step| {
                    let n = offset(at, step);
                    result.cells.contains(&n) || band.contains(&n)
                })
            {
                continue;
            }
            let edge = neighbor_mask(at, SIDES, |other| {
                !result.cells.contains(&other)
                    && neighbor(other)
                        .is_some_and(|v| band.contains(&other) || smooth_open(scene, sets, v))
            });
            smooth_piece(edge)
        } else if on_band || flat_water(scene, old) || matches!(existing, Some((_, 17..=63))) {
            // Resolve the ring against the finished footprint, not the order of tile edits.
            let mask = neighbor_mask(at, EDGE_NEIGHBORS, |other| {
                neighbor(other).is_some_and(|v| {
                    !band.contains(&other)
                        && (result.cells.contains(&other)
                            || matches!(ice_tile(scene, sets, v), Some((_, 0..=16))))
                })
            });
            match water_piece(mask) {
                None if on_band => 63, // The very tip may no longer touch the core.
                None if matches!(existing, Some((_, 17..=62)))
                    && EDGE_NEIGHBORS
                        .into_iter()
                        .any(|step| band.contains(&offset(at, step))) =>
                {
                    63
                }
                None => continue,
                Some(piece) => piece,
            }
        } else {
            continue;
        };
        let raw = if (17..=62).contains(&piece) {
            water_edge_raw(scene, sets, at, piece)
        } else {
            // Filling a water-side edge should not carry its decorative style
            // into the new solid ice. Existing solid/cracked ice keeps its set.
            let base = existing
                .filter(|&(_, old_piece)| piece == 63 || old_piece <= 16)
                .map_or(sets[0], |(base, _)| base);
            raw_ice_piece(scene, base + piece)
        }
        .context("当前地图未加载 VERA 可破裂冰面素材")?;
        let ice = if on_band {
            0
        } else if selected && options.mark_growth {
            1
        } else {
            old.ice
        };
        let new = TerrainValue {
            tile: raw,
            sub: 0,
            ice,
            ..old
        };
        if new != old {
            result.edits.insert(at, new);
        }
    }
    Ok(result)
}

/// Re-edge the one-cell ring around ice replaced by a water stroke. Compute
/// every mask from the fitted terrain so edits do not depend on visit order.
pub(crate) fn refresh_after_water(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
    water: i32,
    edits: &mut BTreeMap<Cell, TerrainValue>,
) -> Result<()> {
    let Some(sets) = scene.palette.bundled_ice_sets else {
        return Ok(());
    };
    let original = |at| value(doc, scene, at);
    let replaced: BTreeSet<_> = selection
        .iter()
        .copied()
        .filter(|at| {
            edits.get(at).is_some_and(|&v| {
                ice_tile(scene, sets, v).is_none() && ice_tile(scene, sets, original(*at)).is_some()
            })
        })
        .collect();
    if replaced.is_empty() {
        return Ok(());
    }
    let affected: BTreeSet<_> = replaced
        .iter()
        .flat_map(|&at| std::iter::once(at).chain(EDGE_NEIGHBORS.map(|step| offset(at, step))))
        .collect();
    let occupied = occupied_cells(doc, scene);
    let fitted = |at| edits.get(&at).copied().unwrap_or_else(|| original(at));
    let mut refreshed = BTreeMap::new();
    for at in affected
        .into_iter()
        .filter(|&at| doc.contains_cell(at) && doc.overlay(at).0 == 255 && !occupied.contains(&at))
    {
        let mut v = fitted(at);
        let existing = ice_tile(scene, sets, v);
        let painted = replaced.contains(&at);
        let same_level = |other, lookup: &dyn Fn(Cell) -> TerrainValue| {
            doc.contains_cell(other)
                .then(|| lookup(other))
                .filter(|n| n.level == v.level)
        };
        let raw = if let Some((base, 0..=15)) = existing {
            let mask = |lookup: &dyn Fn(Cell) -> TerrainValue| {
                neighbor_mask(at, SIDES, |other| {
                    same_level(other, lookup).is_some_and(|n| smooth_open(scene, sets, n))
                })
            };
            let after = mask(&fitted);
            if after == mask(&original) {
                continue;
            }
            raw_ice_piece(scene, base + smooth_piece(after))
        } else if matches!(existing, Some((_, 17..=63))) || painted && flat_water(scene, v) {
            let mask = |lookup: &dyn Fn(Cell) -> TerrainValue| {
                neighbor_mask(at, EDGE_NEIGHBORS, |other| {
                    same_level(other, lookup)
                        .is_some_and(|n| matches!(ice_tile(scene, sets, n), Some((_, 0..=16))))
                })
            };
            let after = mask(&fitted);
            if !painted && after == mask(&original) {
                continue;
            }
            if let Some(piece) = water_piece(after) {
                let old = original(at);
                if ice_tile(scene, sets, old).is_some_and(|(_, p)| p == piece) {
                    Some(old.tile)
                } else {
                    water_edge_raw(scene, sets, at, piece)
                }
            } else {
                v.ice = 0;
                Some(water)
            }
        } else {
            continue;
        };
        v.tile = raw.context("当前地图未加载 VERA 可破裂冰面素材")?;
        v.sub = 0;
        refreshed.insert(at, v);
    }
    edits.extend(refreshed);
    Ok(())
}
