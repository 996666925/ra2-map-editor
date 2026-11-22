//! FA2-style local reachability for a flatten brush, independent of its height.
use crate::{
    map::MapScene,
    placement,
    road_tools::HeightRoads,
    terrain_tools::{self, value},
};
use anyhow::ensure;
use anyhow::{Context, Result};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub steep: bool,
    pub vertices: bool,
    pub force: bool,
    /// Include paved road templates when solving a height stroke.
    pub paved_roads: bool,
}

fn morphable(doc: &MapDocument, scene: &MapScene, at: Cell) -> bool {
    if doc.check_cell(at).is_err() {
        return false;
    }
    let Some(ids) = scene.palette.tile_ids else {
        return false;
    };
    let v = value(doc, scene, at);
    let tile = scene.palette.raw_ids.get(&v.tile).copied().or_else(|| {
        matches!(v.tile, -1 | 65535)
            .then(|| u16::try_from(ids.clear).ok())
            .flatten()
    });
    tile.is_some_and(|tile| {
        let id = i32::from(tile);
        (scene.palette.morphable.contains(&tile)
            || id == ids.clear
            || ids.ramp_base >= 0 && (ids.ramp_base..ids.ramp_base + 20).contains(&id)
            || ids.ramp_smooth >= 0 && (ids.ramp_smooth..ids.ramp_smooth + 12).contains(&id))
            && scene
                .palette
                .templates
                .get(&(tile, 0))
                .is_some_and(|t| t.cells.get(usize::from(v.sub)).is_some_and(Option::is_some))
    })
}

/// Shared preflight for terrain height brushes, including complete footprints.
pub fn editable_cells(
    doc: &MapDocument,
    scene: &MapScene,
    cells: &BTreeSet<Cell>,
) -> Result<BTreeSet<Cell>> {
    editable_cells_with_force(doc, scene, cells, false)
}

pub fn editable_cells_with_force(
    doc: &MapDocument,
    scene: &MapScene,
    cells: &BTreeSet<Cell>,
    force: bool,
) -> Result<BTreeSet<Cell>> {
    editable_cells_with_options(
        doc,
        scene,
        cells,
        Options {
            force,
            ..Default::default()
        },
    )
}

pub fn editable_cells_with_options(
    doc: &MapDocument,
    scene: &MapScene,
    cells: &BTreeSet<Cell>,
    options: Options,
) -> Result<BTreeSet<Cell>> {
    let roads = options.paved_roads.then(|| HeightRoads::new(scene));
    let mut blocked = BTreeSet::new();
    for (_, record) in doc
        .record_iter()
        .filter(|(_, r)| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        blocked.extend(placement::footprint(scene, record)?);
    }
    Ok(cells
        .iter()
        .copied()
        .filter(|&at| {
            (morphable(doc, scene, at)
                || doc.check_cell(at).is_ok()
                    && (options.force || paved_road(roads.as_ref(), scene, value(doc, scene, at))))
                && crate::bridge_tools::height_editable_overlay(doc, scene, at)
                && !blocked.contains(&at)
        })
        .collect())
}

/// Restrict the flood to this brush window. Ground outside it must not provide
/// a route around a cliff and back into the far side of the window.
pub fn window(
    doc: &MapDocument,
    scene: &MapScene,
    center: Cell,
    size: u16,
    target: u8,
) -> Result<BTreeSet<Cell>> {
    window_with_force(doc, scene, center, size, target, false)
}

pub fn window_with_force(
    doc: &MapDocument,
    scene: &MapScene,
    center: Cell,
    size: u16,
    target: u8,
    force: bool,
) -> Result<BTreeSet<Cell>> {
    scene.palette.tile_ids.context("当前环境缺少地形配置")?;
    let mut allowed = BTreeSet::new();
    for dx in 0..size {
        for dy in 0..size {
            let x = i32::from(center[0]) + i32::from(dx) - i32::from(size / 2);
            let y = i32::from(center[1]) + i32::from(dy) - i32::from(size / 2);
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let at = [x, y];
            if (morphable(doc, scene, at) || force && doc.check_cell(at).is_ok())
                && crate::bridge_tools::height_editable_overlay(doc, scene, at)
            {
                allowed.insert(at);
            }
        }
    }
    for (_, record) in doc
        .record_iter()
        .filter(|(_, r)| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in placement::footprint(scene, record)? {
            allowed.remove(&at);
        }
    }
    let seed = if allowed.contains(&center) {
        Some(center)
    } else {
        // A wall cell must never seed both sides. Prefer a neighbouring ground
        // cell at the sampled height, then use a stable nearest-cell order.
        let neighbours: Vec<_> = terrain_tools::neighbors(center)
            .filter(|at| allowed.contains(at))
            .collect();
        let matched = neighbours
            .iter()
            .copied()
            .find(|&at| value(doc, scene, at).level == target);
        if crate::height_tools::cliff_geometry(scene, value(doc, scene, center))?.is_some() {
            matched
        } else {
            matched.or_else(|| neighbours.first().copied())
        }
    };
    let mut cells = BTreeSet::new();
    let mut queue: VecDeque<_> = seed.into_iter().collect();
    while let Some(at) = queue.pop_front() {
        if !allowed.contains(&at) || !cells.insert(at) {
            continue;
        }
        queue.extend(terrain_tools::neighbors(at));
    }
    Ok(cells)
}

// The stock slope patterns in the game's NW/NE/SE/SW corner order. FA2's
// GetRampType also permits the duplicated saddle tiles at offsets 18 and 19.
pub(crate) const RAMPS: [[u8; 4]; 21] = [
    [0, 0, 0, 0],
    [0, 1, 1, 0],
    [0, 0, 1, 1],
    [1, 0, 0, 1],
    [1, 1, 0, 0],
    [0, 0, 1, 0],
    [0, 0, 0, 1],
    [1, 0, 0, 0],
    [0, 1, 0, 0],
    [0, 1, 1, 1],
    [1, 0, 1, 1],
    [1, 1, 0, 1],
    [1, 1, 1, 0],
    [0, 1, 2, 1],
    [1, 0, 1, 2],
    [2, 1, 0, 1],
    [1, 2, 1, 0],
    [0, 1, 0, 1],
    [1, 0, 1, 0],
    [0, 1, 0, 1],
    [1, 0, 1, 0],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Vertex {
    x: i32,
    y: i32,
    // Incident ground cells connected around this vertex. Diagonally touching
    // cliff cells split the two ground surfaces even at their shared corner.
    part: u8,
}
#[derive(Clone, Copy)]
struct Ground {
    value: TerrainValue,
    slope: u8,
    replace: bool,
    road: bool,
}
fn paved_road(roads: Option<&HeightRoads>, scene: &MapScene, v: TerrainValue) -> bool {
    roads.is_some_and(|roads| {
        crate::shore_tools::id(scene, v).is_some_and(|tile| roads.contains(scene, tile))
    })
}

struct Surface<'a> {
    doc: &'a MapDocument,
    scene: &'a MapScene,
    blocked: BTreeSet<Cell>,
    ground: HashMap<Cell, Option<Ground>>,
    steep: bool,
    force: bool,
    roads: Option<HeightRoads>,
}
impl Surface<'_> {
    fn ground(&mut self, x: i32, y: i32) -> Option<Ground> {
        let at = [u16::try_from(x).ok()?, u16::try_from(y).ok()?];
        if let Some(&cached) = self.ground.get(&at) {
            return cached;
        }
        let v = value(self.doc, self.scene, at);
        let road = paved_road(self.roads.as_ref(), self.scene, v);
        let result = if v.level <= 12
            && self.doc.check_cell(at).is_ok()
            && (road || self.force || morphable(self.doc, self.scene, at))
            && crate::bridge_tools::height_editable_overlay(self.doc, self.scene, at)
            && !self.blocked.contains(&at)
        {
            let tile = self
                .scene
                .palette
                .raw_ids
                .get(&v.tile)
                .copied()
                .or_else(|| {
                    matches!(v.tile, -1 | 65535)
                        .then(|| {
                            self.scene
                                .palette
                                .tile_ids
                                .and_then(|ids| u16::try_from(ids.clear).ok())
                        })
                        .flatten()
                })?;
            let slope = self
                .scene
                .palette
                .templates
                .get(&(tile, 0))?
                .cells
                .get(usize::from(v.sub))?
                .as_ref()?
                .slope;
            Some(Ground {
                value: v,
                slope,
                replace: self.force && !road && !morphable(self.doc, self.scene, at),
                road,
            })
        } else {
            None
        };
        self.ground.insert(at, result);
        result
    }
    fn owners(v: Vertex) -> [(i32, i32); 4] {
        [
            (v.x - 1, v.y - 1),
            (v.x, v.y - 1),
            (v.x, v.y),
            (v.x - 1, v.y),
        ]
    }
    fn vertex(&mut self, x: i32, y: i32, owner: (i32, i32)) -> Vertex {
        let v = Vertex { x, y, part: 0 };
        let owners = Self::owners(v);
        let mut allowed = 0u8;
        for (i, &(cx, cy)) in owners.iter().enumerate() {
            if self.ground(cx, cy).is_some() {
                allowed |= 1 << i;
            }
        }
        let index = owners.iter().position(|&p| p == owner).unwrap();
        let mut part = 1u8 << index;
        loop {
            let previous = part;
            for i in 0..4 {
                if part & (1 << i) != 0 {
                    part |= allowed & ((1 << ((i + 1) % 4)) | (1 << ((i + 3) % 4)));
                }
            }
            if previous == part {
                break;
            }
        }
        Vertex { part, ..v }
    }
    fn corners(&mut self, x: i32, y: i32) -> [Vertex; 4] {
        [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)].map(|(vx, vy)| self.vertex(vx, vy, (x, y)))
    }
    fn initial(&mut self, v: Vertex) -> u8 {
        Self::owners(v)
            .iter()
            .enumerate()
            .filter_map(|(i, &(x, y))| {
                if v.part & (1 << i) == 0 {
                    return None;
                }
                let g = self.ground(x, y)?;
                let delta = RAMPS[usize::from(g.slope).min(20)][[2, 3, 0, 1][i]];
                Some(g.value.level + delta)
            })
            .max()
            .unwrap_or(0)
    }
    fn neighbours(&mut self, v: Vertex) -> BTreeSet<Vertex> {
        let mut result = BTreeSet::new();
        for (i, (x, y)) in Self::owners(v).into_iter().enumerate() {
            if v.part & (1 << i) != 0 {
                result.extend(self.corners(x, y));
            }
        }
        result.remove(&v);
        if self.steep {
            result.retain(|next| (next.x - v.x).abs() + (next.y - v.y).abs() == 1);
        }
        result
    }
}

fn surface<'a>(doc: &'a MapDocument, scene: &'a MapScene, options: Options) -> Result<Surface<'a>> {
    let mut blocked = BTreeSet::new();
    for (_, r) in doc
        .record_iter()
        .filter(|(_, r)| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        blocked.extend(placement::footprint(scene, r)?);
    }
    Ok(Surface {
        doc,
        scene,
        blocked,
        ground: HashMap::new(),
        steep: options.steep,
        force: options.force,
        roads: options.paved_roads.then(|| HeightRoads::new(scene)),
    })
}

fn picked_vertex(surface: &mut Surface<'_>, at: Cell) -> Option<Vertex> {
    let (x, y) = (i32::from(at[0]), i32::from(at[1]));
    Surface::owners(Vertex { x, y, part: 0 })
        .into_iter()
        .find(|&(x, y)| surface.ground(x, y).is_some())
        .map(|owner| surface.vertex(x, y, owner))
}

pub fn vertex_height(
    doc: &MapDocument,
    scene: &MapScene,
    at: Cell,
    force: bool,
) -> Result<Option<u8>> {
    let mut s = surface(
        doc,
        scene,
        Options {
            force,
            ..Default::default()
        },
    )?;
    Ok(picked_vertex(&mut s, at).map(|v| s.initial(v)))
}

pub fn vertex_heights(
    doc: &MapDocument,
    scene: &MapScene,
    cells: &BTreeSet<Cell>,
    force: bool,
) -> Result<BTreeMap<Cell, u8>> {
    vertex_heights_with_options(
        doc,
        scene,
        cells,
        Options {
            force,
            ..Default::default()
        },
    )
}

pub fn vertex_heights_with_options(
    doc: &MapDocument,
    scene: &MapScene,
    cells: &BTreeSet<Cell>,
    options: Options,
) -> Result<BTreeMap<Cell, u8>> {
    let mut s = surface(doc, scene, options)?;
    Ok(cells
        .iter()
        .copied()
        .filter_map(|at| picked_vertex(&mut s, at).map(|v| (at, s.initial(v))))
        .collect())
}

/// Smooth through editable ground surfaces, treating cliff faces as holes in
/// that surface rather than locking every adjacent corner (the RMG rule).
/// This follows FA2's vertex maximum, one-level neighbour limit, and ramp
/// matching while retaining an independent surface at a closed cliff corner.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    target: u8,
    smooth: bool,
) -> Result<crate::cliff_tools::PlateauPlan> {
    plan_with_steep(doc, scene, selected, target, smooth, false)
}

/// Shift steep mode uses orthogonal vertex constraints, as FA2sp does,
/// allowing the stock two-level corner slopes along diagonals.
pub fn plan_with_steep(
    doc: &MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    target: u8,
    smooth: bool,
    steep: bool,
) -> Result<crate::cliff_tools::PlateauPlan> {
    plan_with_options(
        doc,
        scene,
        selected,
        target,
        smooth,
        Options {
            steep,
            ..Default::default()
        },
    )
}

pub fn plan_with_options(
    doc: &MapDocument,
    scene: &MapScene,
    selected: &BTreeSet<Cell>,
    target: u8,
    smooth: bool,
    options: Options,
) -> Result<crate::cliff_tools::PlateauPlan> {
    use crate::{cliff_tools::PlateauPlan, terrain_tools::TerrainMode};
    if !smooth {
        let mut copy = doc.fork();
        let changed = terrain_tools::height(
            &mut copy,
            scene,
            selected,
            TerrainMode::Flatten,
            target,
            false,
        )?;
        return Ok(PlateauPlan {
            edits: changed
                .into_iter()
                .map(|at| (at, value(&copy, scene, at)))
                .collect(),
            overlays: BTreeMap::new(),
            cliffs: vec![],
            interior: selected.clone(),
            base: target,
        });
    }
    ensure!(
        target <= 12,
        "自动坡面支持 0～12 层；可关闭自动坡面编辑到 127 层"
    );
    let ids = scene.palette.tile_ids.context("当前环境缺少地形配置")?;
    ensure!(
        ids.clear >= 0 && ids.ramp_base >= 0,
        "当前环境没有可用的坡面模板"
    );
    let mut surface = surface(doc, scene, options)?;
    let mut seeds = BTreeSet::new();
    for &at in selected {
        doc.check_cell(at)?;
        let (x, y) = (i32::from(at[0]), i32::from(at[1]));
        if options.vertices {
            seeds.extend(picked_vertex(&mut surface, at));
        } else if surface.ground(x, y).is_some() {
            seeds.extend(surface.corners(x, y));
        }
    }
    let mut queue: VecDeque<_> = seeds.iter().map(|&v| (v, 0u8)).collect();
    let mut seen = HashSet::new();
    let mut heights = HashMap::new();
    while let Some((v, distance)) = queue.pop_front() {
        if !seen.insert(v) {
            continue;
        }
        let original = surface.initial(v);
        let height = original.clamp(
            target.saturating_sub(distance),
            target.saturating_add(distance),
        );
        if seeds.contains(&v) || height != original {
            heights.insert(v, height);
            for next in surface.neighbours(v) {
                queue.push_back((next, distance.saturating_add(1)));
            }
        }
    }
    let mut affected = BTreeSet::new();
    for &v in heights.keys() {
        for (i, (x, y)) in Surface::owners(v).into_iter().enumerate() {
            if v.part & (1 << i) != 0 {
                affected.insert([x as u16, y as u16]);
            }
        }
    }
    let mut edits = BTreeMap::new();
    let mut road_goals = BTreeMap::new();
    for at in affected {
        let (x, y) = (i32::from(at[0]), i32::from(at[1]));
        let Some(g) = surface.ground(x, y) else {
            continue;
        };
        let corners = surface.corners(x, y).map(|v| {
            heights
                .get(&v)
                .copied()
                .unwrap_or_else(|| surface.initial(v))
        });
        let level = *corners.iter().min().unwrap();
        let relative = corners.map(|h| h - level);
        if g.road {
            let slope = RAMPS[..=4]
                .iter()
                .position(|pattern| *pattern == relative)
                .context("公路需要整排同向的直线坡面，请调整抬高范围")?
                as u8;
            if level != g.value.level || slope != g.slope {
                road_goals.insert(at, (level, slope));
            }
            continue;
        }
        let slope = if relative == [0; 4] {
            0
        } else {
            (1..RAMPS.len())
                .min_by_key(|&i| {
                    RAMPS[i]
                        .iter()
                        .zip(relative)
                        .map(|(&a, b)| a.abs_diff(b) as u16)
                        .sum::<u16>()
                })
                .unwrap()
        };
        let mut next = TerrainValue { level, ..g.value };
        if slope != 0 || g.slope != 0 || g.replace {
            let tile = if slope == 0 {
                ids.clear
            } else {
                ids.ramp_base + slope as i32 - 1
            };
            let tile = u16::try_from(tile)?;
            let key = vera20k_core::map::theater::TileKey {
                tile_id: tile,
                sub_tile: 0,
                variant: 0,
            };
            ensure!(
                scene.palette.images.contains_key(&key),
                "坡面资源缺失，未修改地图"
            );
            next.tile = *scene
                .catalog
                .raw_tiles
                .get(&tile)
                .context("坡面编号无法保存")?;
            next.sub = 0;
        }
        if next != g.value {
            edits.insert(at, next);
        }
    }
    let mut plan = PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior: selected.clone(),
        base: target,
    };
    if surface.roads.is_some() {
        // Occupied road cells are holes in the solver, but a road cannot end
        // at the resulting height discontinuity next to the protected cell.
        for (&at, &(level, slope)) in &road_goals {
            for (dx, dy, edge, opposite) in [
                (-1, 0, [0, 3], [1, 2]),
                (1, 0, [1, 2], [0, 3]),
                (0, -1, [0, 1], [3, 2]),
                (0, 1, [3, 2], [0, 1]),
            ] {
                let Some(next) = crate::shore_tools::offset(at, dx, dy) else {
                    continue;
                };
                if !paved_road(surface.roads.as_ref(), scene, value(doc, scene, next))
                    || surface
                        .ground(i32::from(next[0]), i32::from(next[1]))
                        .is_some()
                {
                    continue;
                }
                let v = value(doc, scene, next);
                let tile = scene.palette.raw_ids[&v.tile];
                let geometry = scene
                    .palette
                    .templates
                    .get(&(tile, 0))
                    .and_then(|t| t.cells.get(usize::from(v.sub)))
                    .and_then(|c| *c)
                    .context("相邻公路模板不完整")?;
                let pattern = RAMPS
                    .get(usize::from(geometry.slope))
                    .context("相邻公路坡面无效")?;
                ensure!(
                    edge.map(|i| u16::from(level) + u16::from(RAMPS[usize::from(slope)][i]))
                        == opposite.map(|i| u16::from(v.level) + u16::from(pattern[i])),
                    "相邻公路受保护，无法衔接坡面"
                );
            }
        }
    }
    if let Some(roads) = &surface.roads {
        roads.plan(doc, scene, &road_goals, &mut plan)?;
    }
    Ok(plan)
}

#[cfg(test)]
#[path = "flatten_tools_tests.rs"]
mod tests;
