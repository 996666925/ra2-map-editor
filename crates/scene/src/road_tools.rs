//! Road networks reconstructed from complete theater templates.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    shore_tools::{ground, id, offset, protect},
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_assets::{catalog::TerrainTemplate, terrain_library::TerrainCategory};
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

fn road_slope(template: &TerrainTemplate, width: u16) -> Option<u8> {
    let slope = template.cells.first().copied().flatten()?.slope;
    let size = match slope {
        1 | 3 => (1, width),
        2 | 4 => (width, 1),
        _ => return None,
    };
    ((template.width, template.height) == size
        && template.cells.len() == usize::from(width)
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
            if let Some(slope) = road_slope(t, 3) {
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
            let Some(tile) = id(scene, old)
                .filter(|&tile| self.contains(scene, tile) || dirt_road_tile(scene, tile))
            else {
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
                let Some(tile) = id(scene, neighbor)
                    .filter(|&tile| self.contains(scene, tile) || dirt_road_tile(scene, tile))
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RoadStyle {
    #[default]
    Paved,
    Dirt,
}

#[derive(Clone, Copy)]
struct Piece {
    tile: u16,
    ports: u8,
    anchor: [i32; 2],
    size: [u16; 2],
}

struct RoadKit {
    width: u16,
    pieces: BTreeMap<u16, Piece>,
    by_ports: BTreeMap<u8, Piece>,
    slopes: BTreeMap<u16, u8>,
    slope_choices: [Option<u16>; 4],
}

fn group_name_is(actual: &str, expected: &str) -> bool {
    actual
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| b.to_ascii_lowercase())
        .eq(expected.bytes().map(|b| b.to_ascii_lowercase()))
}

fn dirt_road_tile(scene: &MapScene, tile: u16) -> bool {
    scene.palette.groups.get(&tile).is_some_and(|group| {
        [
            "DirtRoadsStraight",
            "DirtRoadsBendy",
            "DirtRoadsJunctions",
            "DirtRoadSlopes",
            "DirtRoadsSlopes",
        ]
        .iter()
        .any(|name| group_name_is(&group.name, name))
    })
}

fn stock_group_start(scene: &MapScene, name: &str) -> Option<u16> {
    scene
        .palette
        .groups
        .iter()
        .filter_map(|(&tile, group)| group_name_is(&group.name, name).then_some(tile))
        .min()
}

impl RoadKit {
    fn load(scene: &MapScene, style: RoadStyle) -> Option<Self> {
        let width = if style == RoadStyle::Paved { 3 } else { 2 };
        let mut kit = Self {
            width,
            pieces: BTreeMap::new(),
            by_ports: BTreeMap::new(),
            slopes: BTreeMap::new(),
            slope_choices: [None; 4],
        };
        let mut add =
            |start: u16, index: u16, ports: u8, anchor: [i32; 2], size: [u16; 2]| -> Option<()> {
                let tile = start.checked_add(index)?;
                let template = scene.palette.templates.get(&(tile, 0))?;
                if scene.palette.groups.get(&tile)?.id != scene.palette.groups.get(&start)?.id
                    || [template.width, template.height] != size
                    || template.cells.len() != usize::from(size[0]) * usize::from(size[1])
                    || !template
                        .cells
                        .iter()
                        .flatten()
                        .all(|c| c.height == 0 && c.slope == 0)
                    || template.cells[anchor_index(anchor, size)].is_none()
                    || (style == RoadStyle::Paved && template.cells.iter().any(Option::is_none))
                {
                    return None;
                }
                let piece = Piece {
                    tile,
                    ports,
                    anchor,
                    size,
                };
                kit.pieces.insert(tile, piece);
                kit.by_ports.insert(ports, piece);
                Some(())
            };
        let slope_group = match style {
            RoadStyle::Paved => {
                let ids = scene.palette.tile_ids?;
                let paved = u16::try_from(ids.paved_roads).ok()?;
                let ends = u16::try_from(ids.paved_road_ends).ok()?;
                // Empty theater sets may share the next set's cumulative ID.
                if scene.palette.groups.get(&paved)?.category != TerrainCategory::Road
                    || scene.palette.groups.get(&ends)?.category != TerrainCategory::Road
                {
                    return None;
                }
                for index in 0..14 {
                    if let Some((ports, anchor, size)) = profile(index) {
                        add(paved, index, ports, anchor, size)?;
                    }
                }
                for (index, ports, anchor, size) in [
                    (0, 8, [0, 1], [1, 3]),
                    (1, 4, [1, 0], [3, 1]),
                    (2, 2, [0, 1], [1, 3]),
                    (3, 1, [1, 0], [3, 1]),
                ] {
                    add(ends, index, ports, anchor, size)?;
                }
                "PavedRoadSlopes"
            }
            RoadStyle::Dirt => {
                let straight = stock_group_start(scene, "DirtRoadsStraight")?;
                let bendy = stock_group_start(scene, "DirtRoadsBendy")?;
                let junction = stock_group_start(scene, "DirtRoadsJunctions")?;
                // The two-cell-wide templates use the lower-coordinate lane
                // for the logical centerline. IDs here are offsets within a set.
                for (index, ports, anchor, size) in [
                    (22, 10, [0, 0], [1, 2]),
                    (23, 10, [0, 0], [1, 2]),
                    (55, 5, [0, 0], [2, 1]),
                    (56, 5, [0, 0], [2, 1]),
                    (25, 8, [1, 0], [2, 2]),
                    (28, 2, [0, 0], [2, 2]),
                    (59, 4, [0, 0], [2, 3]),
                    (62, 1, [0, 1], [2, 2]),
                ] {
                    add(straight, index, ports, anchor, size)?;
                }
                for (index, ports) in [(1, 3), (4, 9), (13, 6), (20, 12)] {
                    add(bendy, index, ports, [0, 0], [2, 2])?;
                }
                for (index, ports) in [(0, 7), (1, 14), (2, 13), (3, 11), (4, 15)] {
                    add(junction, index, ports, [0, 0], [2, 2])?;
                }
                "DirtRoadSlopes"
            }
        };
        for (&tile, group) in &scene.palette.groups {
            if !group_name_is(&group.name, slope_group)
                && !(style == RoadStyle::Dirt && group_name_is(&group.name, "DirtRoadsSlopes"))
            {
                continue;
            }
            if let Some(slope) = scene
                .palette
                .templates
                .get(&(tile, 0))
                .and_then(|t| road_slope(t, width))
            {
                kit.slopes.insert(tile, slope);
                let slot = &mut kit.slope_choices[usize::from(slope - 1)];
                *slot = Some(slot.map_or(tile, |old| old.min(tile)));
            }
        }
        Some(kit)
    }

    fn cross_section(&self, ports: u8) -> Piece {
        self.by_ports[&ports]
    }

    fn template_at(&self, scene: &MapScene, v: TerrainValue) -> Option<Piece> {
        let tile = id(scene, v)?;
        self.pieces.get(&tile).copied().or_else(|| {
            let slope = *self.slopes.get(&tile)?;
            Some(Piece {
                tile,
                ..self.cross_section(slope_ports(slope))
            })
        })
    }
}

fn anchor_index(anchor: [i32; 2], size: [u16; 2]) -> usize {
    (anchor[1] * i32::from(size[0]) + anchor[0]) as usize
}

pub fn supported_styles(scene: &MapScene) -> impl Iterator<Item = RoadStyle> + '_ {
    [RoadStyle::Paved, RoadStyle::Dirt]
        .into_iter()
        .filter(|&style| RoadKit::load(scene, style).is_some())
}

fn slope_ports(slope: u8) -> u8 {
    if matches!(slope, 1 | 3) { 10 } else { 5 }
}

// The high edge is given by the decoded TMP ramp direction, in map axes.
fn edge_height(level: u8, slope: u8, port: u8) -> Option<u8> {
    if slope == 0 {
        return Some(level);
    }
    let high = match slope {
        1 => 2,
        2 => 4,
        3 => 8,
        4 => 1,
        _ => return None,
    };
    (slope_ports(slope) & port != 0).then(|| level + u8::from(port == high))
}

fn opposite(port: u8) -> u8 {
    ((port << 2) | (port >> 2)) & 15
}

fn stamp_points(origin: Cell, piece: Piece) -> impl Iterator<Item = Cell> {
    let center = offset(origin, piece.anchor[0], piece.anchor[1]);
    center.into_iter().flat_map(move |center| {
        std::iter::once(center).chain(
            DIRS.into_iter()
                .filter(move |&(_, _, bit)| piece.ports & bit != 0)
                .flat_map(move |(dx, dy, _)| {
                    let length = match (dx, dy) {
                        (-1, _) => piece.anchor[0],
                        (1, _) => i32::from(piece.size[0]) - 1 - piece.anchor[0],
                        (_, -1) => piece.anchor[1],
                        _ => i32::from(piece.size[1]) - 1 - piece.anchor[1],
                    };
                    (1..=length).filter_map(move |d| offset(center, dx * d, dy * d))
                }),
        )
    })
}

fn snap_endpoint(doc: &MapDocument, scene: &MapScene, kit: &RoadKit, at: Cell) -> Result<Cell> {
    let old = value(doc, scene, at);
    // Prefer the template under the cursor, particularly alongside bends.
    if let Some(piece) = kit.template_at(scene, old) {
        let template = &scene.palette.templates[&(piece.tile, 0)];
        let origin = stamp_origin(at, old, template)?;
        return Ok(stamp_points(origin, piece)
            .min_by_key(|p| (p[0].abs_diff(at[0]) + p[1].abs_diff(at[1]), *p))
            .unwrap_or(at));
    }
    let mut best = None;
    for (dx, dy, _) in DIRS {
        let Some(n) = offset(at, dx, dy).filter(|&n| doc.check_cell(n).is_ok()) else {
            continue;
        };
        let v = value(doc, scene, n);
        if let Some(piece) = kit.template_at(scene, v)
            && v.level == old.level
            && !kit.slopes.contains_key(&piece.tile)
        {
            let origin = stamp_origin(n, v, &scene.palette.templates[&(piece.tile, 0)])?;
            for p in stamp_points(origin, piece)
                .filter(|p| p[0].abs_diff(at[0]) + p[1].abs_diff(at[1]) <= 1)
            {
                best = Some(best.map_or(p, |old: Cell| old.min(p)));
            }
        }
    }
    Ok(best.unwrap_or(at))
}

/// The legacy axis-aligned paved operation used by existing callers.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    cut: bool,
) -> Result<PlateauPlan> {
    plan_selected(doc, scene, a, b, RoadStyle::Paved, cut, true, false)
}

/// Preview and commit share this operation. A diagonal is approximated with
/// axis-aligned runs long enough for the theater's complete corner templates.
pub fn plan_segment(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    style: RoadStyle,
    cut: bool,
    axis_lock: bool,
) -> Result<PlateauPlan> {
    plan_selected(doc, scene, a, b, style, cut, axis_lock || cut, true)
}

#[expect(clippy::too_many_arguments)]
fn plan_selected(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    style: RoadStyle,
    cut: bool,
    axis_lock: bool,
    grades: bool,
) -> Result<PlateauPlan> {
    let kit = RoadKit::load(scene, style).context("当前环境没有这种道路的完整连接模板")?;
    doc.check_cell(a)?;
    doc.check_cell(b)?;
    let a = snap_endpoint(doc, scene, &kit, a)?;
    let mut b = snap_endpoint(doc, scene, &kit, b)?;
    if axis_lock {
        b = axis_endpoint(a, b);
    }
    ensure!(a != b, "拖动一段距离以生成道路");
    let (a, b) = (a.min(b), a.max(b));
    let span = [a[0].abs_diff(b[0]), a[1].abs_diff(b[1])];
    if span.contains(&0) {
        return build_segment(doc, scene, &kit, &cell_line(a, b), cut, grades);
    }
    let runs = segment_runs(a, b, kit.width);
    // Try both bend orientations and progressively fewer bends when a bend
    // would land on an obstruction or has too little room for its full TMP.
    let mut first_error = None;
    for steps in [runs, runs.div_ceil(2), 1]
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev()
    {
        for first_axis in 0..2 {
            let mut path = vec![a];
            append_segment(&mut path, b, steps, first_axis);
            match build_segment(doc, scene, &kit, &path, cut, grades) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
    }
    Err(first_error.unwrap())
}

/// Project the next endpoint onto the dominant map axis of the current leg.
pub fn axis_endpoint(start: Cell, end: Cell) -> Cell {
    if start[0].abs_diff(end[0]) >= start[1].abs_diff(end[1]) {
        [end[0], start[1]]
    } else {
        [start[0], end[1]]
    }
}

fn segment_runs(a: Cell, b: Cell, width: u16) -> u16 {
    let spacing = if width == 3 { 5 } else { 4 };
    (a[0].abs_diff(b[0]).min(a[1].abs_diff(b[1])) / spacing).clamp(1, 12)
}

fn append_segment(path: &mut Vec<Cell>, end: Cell, steps: u16, first_axis: usize) {
    let start = *path.last().unwrap();
    let mut at = start;
    for n in 1..=steps {
        for axis in [first_axis, 1 - first_axis] {
            let mut next = at;
            next[axis] = (i32::from(start[axis])
                + (i32::from(end[axis]) - i32::from(start[axis])) * i32::from(n) / i32::from(steps))
                as u16;
            path.extend(cell_line(at, next).into_iter().skip(1));
            at = next;
        }
    }
}

/// Lay out all legs before choosing road templates. Shared vertices are part
/// of the same network, so corners, intersections and slopes are checked once.
pub fn plan_polyline(
    doc: &MapDocument,
    scene: &MapScene,
    vertices: &[Cell],
    style: RoadStyle,
) -> Result<PlateauPlan> {
    plan_route(doc, scene, vertices, style).map(|(_, plan)| plan)
}

/// A resolved draft in drawing order, including the exact staircase chosen
/// for each diagonal. Extending it can refit road pieces at a connection
/// without rerouting an accepted leg.
pub struct RoadRoute {
    centerline: Vec<Cell>,
    style: RoadStyle,
}

impl RoadRoute {
    /// `ends` contains the new vertices after the last resolved one. Use the
    /// same map revision and theater that this route was planned against.
    pub fn extend(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        ends: &[Cell],
    ) -> Result<(Self, PlateauPlan)> {
        plan_route_with_prefix(doc, scene, ends, self.style, &self.centerline)
    }
}

pub fn plan_route(
    doc: &MapDocument,
    scene: &MapScene,
    vertices: &[Cell],
    style: RoadStyle,
) -> Result<(RoadRoute, PlateauPlan)> {
    plan_route_with_prefix(doc, scene, vertices, style, &[])
}

fn plan_route_with_prefix(
    doc: &MapDocument,
    scene: &MapScene,
    ends: &[Cell],
    style: RoadStyle,
    prefix: &[Cell],
) -> Result<(RoadRoute, PlateauPlan)> {
    let kit = RoadKit::load(scene, style).context("当前环境没有这种道路的完整连接模板")?;
    let mut vertices = Vec::with_capacity(ends.len() + 1);
    vertices.extend(prefix.last().copied());
    for &at in ends {
        doc.check_cell(at)?;
        vertices.push(snap_endpoint(doc, scene, &kit, at)?);
    }
    vertices.dedup();
    ensure!(
        prefix.len() + vertices.len() >= 2,
        "连续道路至少需要两个不同的点"
    );
    // Canonical traversal gives an initial line the same bends in either
    // direction. Extensions always start at the accepted drawing endpoint.
    let reverse = prefix.is_empty() && vertices.iter().cmp(vertices.iter().rev()).is_gt();
    if reverse {
        vertices.reverse();
    }
    let build = |mut path: Vec<Cell>| {
        let plan = build_segment(doc, scene, &kit, &path, false, true)?;
        if reverse {
            path.reverse();
        }
        Ok((
            RoadRoute {
                centerline: path,
                style,
            },
            plan,
        ))
    };
    let new_path = || {
        if prefix.is_empty() {
            vec![vertices[0]]
        } else {
            prefix.to_vec()
        }
    };
    let runs: Vec<_> = vertices
        .windows(2)
        .map(|pair| segment_runs(pair[0], pair[1], kit.width))
        .collect();
    let diagonals = vertices
        .windows(2)
        .filter(|pair| pair[0][0] != pair[1][0] && pair[0][1] != pair[1][1])
        .count();
    if diagonals == 0 {
        let mut path = new_path();
        for &end in &vertices[1..] {
            append_segment(&mut path, end, 1, 0);
        }
        return build(path);
    }
    // Keep the search bounded during previews. Try both staircase phases,
    // alternating phases on successive diagonal legs, and longer straight runs.
    let mut first_error = None;
    for resolution in 0..3 {
        for phase in 0..if diagonals == 1 { 2 } else { 4 } {
            let mut path = new_path();
            let mut diagonal_index = 0;
            for (pair, &runs) in vertices.windows(2).zip(&runs) {
                let steps = match resolution {
                    0 => runs,
                    1 => runs.div_ceil(2),
                    _ => 1,
                };
                let first_axis = (phase + (phase / 2) * diagonal_index) % 2;
                append_segment(&mut path, pair[1], steps, first_axis);
                if pair[0][0] != pair[1][0] && pair[0][1] != pair[1][1] {
                    diagonal_index += 1;
                }
            }
            match build(path) {
                Ok(route) => return Ok(route),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
    }
    Err(first_error.unwrap())
}

#[derive(Clone, Copy)]
struct Grade {
    level: u8,
    slope: u8,
}

fn grade(scene: &MapScene, v: TerrainValue) -> Grade {
    let slope = id(scene, v)
        .and_then(|tile| road_subtile(scene, tile, v.sub).ok())
        .map_or(0, |(_, c)| c.slope);
    Grade {
        level: v.level,
        slope,
    }
}

fn build_segment(
    doc: &MapDocument,
    scene: &MapScene,
    kit: &RoadKit,
    path: &[Cell],
    cut: bool,
    grades: bool,
) -> Result<PlateauPlan> {
    let selected: BTreeSet<_> = path.iter().copied().collect();
    let base = value(doc, scene, path[0]).level;
    let mut queue = VecDeque::new();
    for &at in path {
        doc.check_cell(at)?;
        let g = grade(scene, value(doc, scene, at));
        ensure!(
            g.slope == 0 || (grades && !cut && g.slope <= 4 && g.level < 127),
            "道路坡面须为可衔接的一层直坡"
        );
        if !grades || cut {
            ensure!(g.level == base, "平路绘制和切除需要相同高度");
        }
        // Only pull in complete existing stamps touched by the proposed
        // centerline or its approaches, at the corresponding elevation.
        for dx in -2..=2 {
            for dy in -2..=2 {
                if let Some(p) = offset(at, dx, dy) {
                    queue.push_back((p, g.level));
                }
            }
        }
    }
    if !cut {
        for pair in path.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let port = DIRS
                .iter()
                .find(|&&(dx, dy, _)| offset(a, dx, dy) == Some(b))
                .unwrap()
                .2;
            let a = grade(scene, value(doc, scene, a));
            let b = grade(scene, value(doc, scene, b));
            let edge = edge_height(a.level, a.slope, port);
            ensure!(
                edge.is_some() && edge == edge_height(b.level, b.slope, opposite(port)),
                "高低路段需要沿同方向的一层坡道连接"
            );
        }
    }
    let mut visited = BTreeSet::new();
    let mut stamps = BTreeSet::new();
    let mut old = BTreeSet::new();
    let mut points = BTreeMap::new();
    while let Some((at, level)) = queue.pop_front() {
        if !visited.insert((at, level)) || doc.check_cell(at).is_err() {
            continue;
        }
        let v = value(doc, scene, at);
        if v.level != level {
            continue;
        }
        let Some(piece) = kit.template_at(scene, v) else {
            continue;
        };
        // Slopes carry fixed ports and are processed separately from the
        // neighboring flat network unless the new line traverses one.
        if kit.slopes.contains_key(&piece.tile) {
            continue;
        }
        let template = &scene.palette.templates[&(piece.tile, 0)];
        let origin = stamp_origin(at, v, template)?;
        if !stamps.insert((piece.tile, origin)) {
            continue;
        }
        for p in existing_stamp(doc, scene, piece.tile, origin, level)?.into_keys() {
            old.insert(p);
            queue.extend(crate::terrain_tools::neighbors(p).map(|p| (p, level)));
        }
        points.extend(stamp_points(origin, piece).map(|p| (p, Grade { level, slope: 0 })));
    }
    if cut {
        ensure!(
            selected.iter().any(|p| points.contains_key(p)),
            "没有选中道路中心线"
        );
        points.retain(|p, _| !selected.contains(p));
    } else {
        points.extend(
            path.iter()
                .map(|&p| (p, grade(scene, value(doc, scene, p)))),
        );
    }
    let mut preserved_slopes = BTreeSet::new();
    let mut masks = BTreeMap::new();
    for (&p, &g) in &points {
        let mut mask = 0;
        for (dx, dy, bit) in DIRS {
            let Some(n) = offset(p, dx, dy).filter(|&n| doc.check_cell(n).is_ok()) else {
                continue;
            };
            let Some(edge) = edge_height(g.level, g.slope, bit) else {
                continue;
            };
            if let Some(other) = points.get(&n) {
                if edge_height(other.level, other.slope, opposite(bit)) == Some(edge) {
                    mask |= bit;
                }
                continue;
            }
            let v = value(doc, scene, n);
            let Some(tile) = id(scene, v) else { continue };
            let Some(&slope) = kit.slopes.get(&tile) else {
                continue;
            };
            let cross = kit.cross_section(slope_ports(slope));
            if usize::from(v.sub) == anchor_index(cross.anchor, cross.size)
                && edge_height(v.level, slope, opposite(bit)) == Some(edge)
            {
                let origin = stamp_origin(n, v, &scene.palette.templates[&(tile, 0)])?;
                if preserved_slopes.insert((tile, origin)) {
                    existing_stamp(doc, scene, tile, origin, v.level)?;
                }
                mask |= bit;
            }
        }
        if g.slope != 0 {
            ensure!(
                mask == slope_ports(g.slope),
                "坡道两端需要直路衔接，请将线段延伸至平地"
            );
        }
        masks.insert(p, mask);
    }
    let mut result = PlateauPlan {
        edits: BTreeMap::new(),
        overlays: BTreeMap::new(),
        cliffs: Vec::new(),
        interior: selected,
        base,
    };
    for &at in &old {
        let v = value(doc, scene, at);
        result
            .edits
            .insert(at, cliff_authoring::clear(scene, v, v.level)?);
    }
    let mut occupied = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    // Large pieces reserve their footprint before the one-cell straight runs.
    for priority in [0, 1, 2] {
        for (&at, &mask) in &masks {
            let shape = match mask {
                0 => continue,
                1 | 2 | 4 | 8 => 1,
                5 | 10 => 2,
                _ => 0,
            };
            if shape != priority || consumed.contains(&at) {
                continue;
            }
            let g = points[&at];
            let mut piece = kit.by_ports[&mask];
            if g.slope != 0 {
                let v = value(doc, scene, at);
                piece.tile = id(scene, v)
                    .filter(|tile| kit.slopes.get(tile) == Some(&g.slope))
                    .or(kit.slope_choices[usize::from(g.slope - 1)])
                    .context("当前道路类型缺少匹配方向的坡道素材")?;
            }
            let origin = offset(at, -piece.anchor[0], -piece.anchor[1]).context("道路模板越界")?;
            let cells = cliff_authoring::stamp(doc, scene, piece.tile, origin, g.level)?;
            let covered: BTreeSet<_> = stamp_points(origin, piece).collect();
            for &p in cells.keys() {
                ensure!(occupied.insert(p), "道路转角或路口相距过近，请拉开直道间距");
                let previous = value(doc, scene, p);
                ensure!(
                    previous.level == g.level
                        && (if g.slope == 0 {
                            old.contains(&p) || ground(scene, previous)
                        } else {
                            road_slope_surface(doc, scene, kit, p, previous, g.slope, origin)?
                        }),
                    "{},{} 的地形与道路宽度、高度或坡向不匹配",
                    p[0],
                    p[1]
                );
                if let Some(&other_mask) = masks.get(&p) {
                    ensure!(
                        covered.contains(&p) && points[&p].slope == g.slope,
                        "道路模板会覆盖相邻路线，请拉开间距"
                    );
                    consumed.insert(p);
                    for (dx, dy, bit) in DIRS {
                        if other_mask & bit != 0
                            && !offset(p, dx, dy).is_some_and(|n| cells.contains_key(&n))
                        {
                            ensure!(mask & bit != 0, "道路转角内出现额外分支，请加大路口间距");
                        }
                    }
                }
            }
            result.edits.extend(cells);
            result.cliffs.push(CliffStamp {
                tile: piece.tile,
                origin,
                base: g.level,
            });
        }
    }
    result.edits.retain(|&at, v| *v != value(doc, scene, at));
    protect(doc, scene, &result.edits)?;
    Ok(result)
}

fn road_slope_surface(
    doc: &MapDocument,
    scene: &MapScene,
    kit: &RoadKit,
    at: Cell,
    v: TerrainValue,
    slope: u8,
    target_origin: Cell,
) -> Result<bool> {
    let Some(tile) = id(scene, v) else {
        return Ok(false);
    };
    let (t, g) = road_subtile(scene, tile, v.sub)?;
    if g.slope != slope || g.height != 0 {
        return Ok(false);
    }
    if kit.slopes.get(&tile) == Some(&slope) {
        let origin = stamp_origin(at, v, t)?;
        if origin != target_origin {
            return Ok(false);
        }
        existing_stamp(doc, scene, tile, origin, v.level)?;
        return Ok(true);
    }
    Ok(t.width == 1
        && t.height == 1
        && scene
            .palette
            .groups
            .get(&tile)
            .is_some_and(|g| g.category == TerrainCategory::Ramp))
}
