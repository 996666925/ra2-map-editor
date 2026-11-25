//! Two-point cuts of one existing four-level plateau.
use super::{cliff, cliff_rim, offset, portals, remesh, stamp, tile};
use crate::{
    cliff_tools::{self, PlateauPlan},
    flatten_tools::RAMPS,
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CutSide {
    #[default]
    Left,
    Right,
}

impl CutSide {
    fn contains(self, start: Cell, end: Cell, at: Cell) -> bool {
        let [dx, dy] = [0, 1].map(|axis| i32::from(end[axis]) - i32::from(start[axis]));
        let [x, y] = [0, 1].map(|axis| i32::from(at[axis]) - i32::from(start[axis]));
        // Iso projection preserves orientation. Screen y grows downward.
        let cross = dx * y - dy * x;
        match self {
            Self::Left => cross < 0,
            Self::Right => cross > 0,
        }
    }
}

pub struct CliffCut {
    start: Cell,
    base: u8,
    core: BTreeSet<Cell>,
    old_cliffs: BTreeSet<Cell>,
    entrance: BTreeSet<Cell>,
}

impl CliffCut {
    /// Find the first wall crossed by the segment between the two cell centers.
    pub fn first_wall(doc: &MapDocument, scene: &MapScene, start: Cell, end: Cell) -> Result<Cell> {
        cliff_tools::validate_polyline_coords(doc, &[start, end])?;
        ensure!(start != end, "切线需要两个不同的点");
        let [dx, dy] = [0, 1].map(|axis| i32::from(end[axis]) - i32::from(start[axis]));
        let (mut next_x, mut next_y) = (1, 1);
        let mut at = start;
        loop {
            if doc.contains_cell(at) && cliff(scene, value(doc, scene, at), at).is_some() {
                return Ok(at);
            }
            if at == end {
                break;
            }
            // Compare the boundary crossing times in half-cell units. At an
            // exact corner only enter the diagonally adjacent cell.
            let crossing = (next_x * dy.abs()).cmp(&(next_y * dx.abs()));
            if crossing != Ordering::Greater {
                at[0] = (i32::from(at[0]) + dx.signum()) as u16;
                next_x += 2;
            }
            if crossing != Ordering::Less {
                at[1] = (i32::from(at[1]) + dy.signum()) as u16;
                next_y += 2;
            }
        }
        anyhow::bail!("切线尚未穿过悬崖，请把第二点往高台方向移")
    }

    pub fn select(doc: &MapDocument, scene: &MapScene, start: Cell) -> Result<Self> {
        doc.check_cell(start)?;
        let picked = value(doc, scene, start);
        let message = "未找到可削切的连通高台";
        let base = cliff(scene, picked, start)
            .map(|(_, _, base)| base)
            .or_else(|| picked.level.checked_sub(4))
            .filter(|&base| base <= cliff_tools::MAX_CLIFF_BASE)
            .context(message)?;
        let (mut core, old_cliffs) = component(doc, scene, start, base)?;
        ensure!(!core.is_empty() && !old_cliffs.is_empty(), "{message}");
        let entrance = portals(doc, scene, &mut core, &BTreeSet::new(), base)?;
        Ok(Self {
            start,
            base,
            core,
            old_cliffs,
            entrance,
        })
    }

    pub fn contains_wall(&self, at: Cell) -> bool {
        self.old_cliffs.contains(&at)
    }

    pub fn base(&self) -> u8 {
        self.base
    }

    pub fn plan(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        end: Cell,
        side: CutSide,
        target: Option<u8>,
    ) -> Result<PlateauPlan> {
        self.plan_line(doc, scene, [self.start, end], side, target)
    }

    pub fn plan_line(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        [start, end]: [Cell; 2],
        side: CutSide,
        target: Option<u8>,
    ) -> Result<PlateauPlan> {
        cliff_tools::validate_polyline_coords(doc, &[start, end])?;
        ensure!(start != end, "切线需要两个不同的点");
        let base = self.base;
        let target = target.unwrap_or(base);
        ensure!(
            (base..base + 4).contains(&target),
            "目标高度须在崖脚与崖顶之间，并低于崖顶"
        );
        let removed: BTreeSet<_> = self
            .core
            .iter()
            .copied()
            .filter(|&at| side.contains(start, end, at))
            .collect();
        ensure!(!removed.is_empty(), "切线选中一侧没有可削切的高地");
        ensure!(
            self.entrance.is_disjoint(&removed),
            "削切区域触及已有坡道或桥头，请移开切线"
        );
        let kept = self.core.difference(&removed).copied().collect();
        let mut full = remesh(
            doc,
            scene,
            kept,
            self.old_cliffs.clone(),
            base,
            &self.entrance,
            Some(&removed),
        )?;
        // Only replace walls belonging to this component. A new stamp must
        // not consume a neighboring plateau's wall, or an existing slope.
        for &at in full.edits.keys() {
            if self.old_cliffs.contains(&at) {
                continue;
            }
            let old = value(doc, scene, at);
            ensure!(
                tile(scene, old.tile)
                    .and_then(|id| crate::coastal_cliffs::piece(scene, id))
                    .is_none(),
                "削切会触及另一组悬崖，请移开切线"
            );
            ensure!(
                geometry(scene, old).is_none_or(|slope| slope == 0),
                "削切区域触及已有坡道，请移开切线"
            );
        }
        if target > base {
            grade_floor(doc, scene, &removed, &mut full, target)?;
        }
        cliff_tools::validate(doc, scene, &full.edits, base, true)?;
        Ok(full)
    }
}

// Follow high ground, accepting an old stamp only from its own high rim.
// Neighboring low faces can touch even when their plateaus are disconnected.
fn component(
    doc: &MapDocument,
    scene: &MapScene,
    start: Cell,
    base: u8,
) -> Result<(BTreeSet<Cell>, BTreeSet<Cell>)> {
    let side = usize::from(MAX_CELL_SIDE);
    let index = |[x, y]: Cell| usize::from(y) * side + usize::from(x);
    let mut seen = vec![false; side * side];
    let mut top = vec![false; side * side];
    let mut queue = VecDeque::new();
    let mut core = Vec::new();
    let mut old = BTreeSet::new();
    let mut stamps = HashSet::new();
    let picked = cliff(scene, value(doc, scene, start), start);
    let high = base + 4;
    let enqueue = |at: Cell, queue: &mut VecDeque<Cell>, top: &mut [bool]| {
        if !std::mem::replace(&mut top[index(at)], true) {
            queue.push_back(at);
        }
    };
    if let Some((id, origin, _)) = picked {
        for at in cliff_rim(scene, id, origin, base) {
            if doc.check_cell(at).is_ok() && value(doc, scene, at).level == high {
                enqueue(at, &mut queue, &mut top);
            }
        }
    } else {
        enqueue(start, &mut queue, &mut top);
    }
    while let Some(at) = queue.pop_front() {
        core.push(at);
        let mut boundary = false;
        for (dx, dy, _, _) in EDGES {
            let Some(next) = offset(at, dx, dy) else {
                boundary = true;
                continue;
            };
            let i = index(next);
            if !std::mem::replace(&mut seen[i], true) && doc.check_cell(next).is_ok() {
                let v = value(doc, scene, next);
                if v.level == high && cliff(scene, v, next).is_none() {
                    enqueue(next, &mut queue, &mut top);
                    continue;
                }
            }
            // Known top cells either have already been processed or are queued.
            // Neighboring stamps are relevant only along the terrain boundary.
            boundary |= !top[i];
        }
        if !boundary {
            continue;
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(p) = offset(at, dx, dy) else {
                    continue;
                };
                if !doc.contains_cell(p) {
                    continue;
                }
                let Some((id, origin, stamp_base)) = cliff(scene, value(doc, scene, p), p) else {
                    continue;
                };
                if stamp_base != base || stamps.contains(&(id, origin)) {
                    continue;
                }
                let mut rim = cliff_rim(scene, id, origin, base);
                if !rim.any(|p| p == at) {
                    continue;
                }
                stamps.insert((id, origin));
                for p in cliff_rim(scene, id, origin, base) {
                    if doc.check_cell(p).is_ok() && value(doc, scene, p).level == high {
                        enqueue(p, &mut queue, &mut top);
                    }
                }
                super::collect_existing_cliff(doc, scene, id, origin, base, &mut old)?;
            }
        }
    }
    Ok((core.into_iter().collect(), old))
}

fn geometry(scene: &MapScene, old: TerrainValue) -> Option<u8> {
    let id = tile(scene, old.tile)?;
    Some(
        scene
            .palette
            .templates
            .get(&(id, 0))?
            .cells
            .get(usize::from(old.sub))?
            .as_ref()?
            .slope,
    )
}

const EDGES: [(i32, i32, [usize; 2], [usize; 2]); 4] = [
    (0, -1, [0, 1], [3, 2]),
    (1, 0, [1, 2], [0, 3]),
    (0, 1, [2, 3], [1, 0]),
    (-1, 0, [3, 0], [2, 1]),
];

/// Leave a low toe beside the rebuilt full-height walls, then grade to
/// the exact chosen level within the lowered land. Every wall keeps its
/// original four-level art and every ramp shares corners with its neighbors.
fn grade_floor(
    doc: &MapDocument,
    scene: &MapScene,
    removed: &BTreeSet<Cell>,
    plan: &mut PlateauPlan,
    target: u8,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("当前环境没有地形配置")?;
    let base = plan.base;
    let after = |at| {
        plan.edits
            .get(&at)
            .copied()
            .unwrap_or_else(|| value(doc, scene, at))
    };
    let domain: BTreeSet<_> = removed
        .iter()
        .copied()
        .filter(|&at| {
            let v = after(at);
            v.level == base && cliff(scene, v, at).is_none()
        })
        .collect();
    let narrow = "削切区域过窄，无法衔接中间高度，请移动切线或削到崖脚";
    let first = *domain.first().context(narrow)?;
    let (min, max) = domain.iter().fold((first, first), |(lo, hi), &[x, y]| {
        ([lo[0].min(x), lo[1].min(y)], [hi[0].max(x), hi[1].max(y)])
    });
    // Include the far vertices of the last row/column and a one-cell
    // guard band, so offsets around even an outer vertex are in bounds.
    let width = usize::from(max[0] - min[0]) + 4;
    let count = width * (usize::from(max[1] - min[1]) + 4);
    let index = |[x, y]: Cell| usize::from(y - min[1] + 1) * width + usize::from(x - min[0] + 1);
    let corner_offsets = [0, 1, width + 1, width];
    let mut editable = vec![false; count];
    for &at in &domain {
        editable[index(at)] = true;
    }
    let mut bounds = vec![[base, base + 4]; count];
    let mut queue = VecDeque::new();
    for &at in &domain {
        let cell = index(at);
        for ((dx, dy, edge, opposite), other) in
            EDGES
                .into_iter()
                .zip([cell - width, cell + 1, cell + width, cell - 1])
        {
            if editable[other] {
                continue;
            }
            let Some(next) = offset(at, dx, dy).filter(|&next| doc.check_cell(next).is_ok()) else {
                continue;
            };
            let neighbor = after(next);
            let (level, pattern) = if cliff(scene, neighbor, next).is_some() {
                (base, &RAMPS[0])
            } else {
                (
                    neighbor.level,
                    geometry(scene, neighbor)
                        .and_then(|slope| RAMPS.get(usize::from(slope)))
                        .unwrap_or(&RAMPS[0]),
                )
            };
            for (i, j) in edge.into_iter().zip(opposite) {
                let height = level.saturating_add(pattern[j]);
                ensure!(
                    (base..=base + 4).contains(&height),
                    "削切无法衔接周围的不同高度地形"
                );
                let vertex = cell + corner_offsets[i];
                if bounds[vertex][0] == bounds[vertex][1] {
                    ensure!(bounds[vertex][0] == height, "{narrow}");
                } else {
                    bounds[vertex] = [height, height];
                    queue.push_back(vertex);
                }
            }
        }
    }
    // All corners of an editable cell share a unit-step constraint.
    while let Some(vertex) = queue.pop_front() {
        let [low, high] = bounds[vertex];
        for offset in corner_offsets {
            let owner = vertex - offset;
            if !editable[owner] {
                continue;
            }
            for offset in corner_offsets {
                let next = owner + offset;
                let [a, b] = bounds[next];
                let limited = [a.max(low.saturating_sub(1)), b.min(high + 1)];
                ensure!(limited[0] <= limited[1], "{narrow}");
                if limited != bounds[next] {
                    bounds[next] = limited;
                    queue.push_back(next);
                }
            }
        }
    }
    let mut edits = BTreeMap::new();
    let mut has_floor = false;
    for &at in &domain {
        let cell = index(at);
        let levels = corner_offsets.map(|offset| {
            let [lo, hi] = bounds[cell + offset];
            target.clamp(lo, hi)
        });
        let level = *levels.iter().min().unwrap();
        let pattern = levels.map(|h| h - level);
        let slope = RAMPS.iter().position(|r| *r == pattern).context(narrow)?;
        let floor = after(at);
        let next = if slope == 0 {
            has_floor |= level == target;
            TerrainValue { level, ..floor }
        } else {
            ensure!(ids.ramp_base >= 0, "当前环境没有可用的坡面模板");
            let id = u16::try_from(ids.ramp_base + slope as i32 - 1)?;
            let patch = stamp(doc, scene, id, at, level)?;
            let next = *patch
                .get(&at)
                .filter(|_| patch.len() == 1)
                .context("坡面资源缺失，未修改地图")?;
            ensure!(
                geometry(scene, next) == Some(slope as u8),
                "坡面资源缺失，未修改地图"
            );
            next
        };
        if next != floor {
            edits.insert(at, next);
        }
    }
    ensure!(has_floor, "{narrow}");
    plan.edits.extend(edits);
    Ok(())
}
