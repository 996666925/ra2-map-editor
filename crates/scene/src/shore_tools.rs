//! Local shoreline reconstruction from complete TMP water/land patterns.
//! No random-generator erosion, partial stamps, or changes outside the plan.
use crate::{
    cliff_authoring,
    cliff_tools::{self, CliffStamp, PlateauPlan},
    cliff_variants::{available, style_score},
    ice_tools,
    map::MapScene,
    terrain_tools::{self, value},
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, Target, TerrainValue, Value, cell_line};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

pub(crate) fn offset(at: Cell, dx: i32, dy: i32) -> Option<Cell> {
    let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
    ((0..i32::from(MAX_CELL_SIDE)).contains(&x) && (0..i32::from(MAX_CELL_SIDE)).contains(&y))
        .then_some([x as u16, y as u16])
}
pub(crate) fn id(scene: &MapScene, v: TerrainValue) -> Option<u16> {
    scene.palette.raw_ids.get(&v.tile).copied()
}
pub(crate) fn shore(scene: &MapScene, tile: u16) -> bool {
    scene
        .palette
        .tile_ids
        .is_some_and(|i| i.shore >= 0 && (i.shore..i.shore + 40).contains(&i32::from(tile)))
}
/// Recover the complete shoreline piece owning an existing subtile.
pub(crate) fn existing_shore(
    doc: &MapDocument,
    scene: &MapScene,
    at: Cell,
    tile: u16,
    v: TerrainValue,
) -> Result<BTreeMap<Cell, TerrainValue>> {
    let t = scene
        .palette
        .templates
        .get(&(tile, 0))
        .context("已有海岸模板缺失")?;
    let origin = offset(
        at,
        -(i32::from(v.sub) % i32::from(t.width)),
        -(i32::from(v.sub) / i32::from(t.width)),
    )
    .context("已有海岸越界")?;
    let base = v
        .level
        .checked_sub(
            t.cells
                .get(usize::from(v.sub))
                .and_then(|s| *s)
                .context("海岸子块无效")?
                .height,
        )
        .context("海岸高度无效")?;
    let cells = cliff_authoring::stamp(doc, scene, tile, origin, base)?;
    for (&p, &expected) in &cells {
        ensure!(
            value(doc, scene, p) == expected,
            "{},{} 的海岸模板不完整，请先恢复该模板",
            p[0],
            p[1]
        );
    }
    Ok(cells)
}

fn water_tile(scene: &MapScene, tile: u16) -> bool {
    scene.palette.tile_ids.is_some_and(|ids| {
        ids.water_base >= 0
            && (ids.water_base..ids.water_base + 14).contains(&i32::from(tile))
            && crate::coastal_cliffs::same_tileset(scene, ids.water_base as u16, tile)
    })
}

/// Flat WaterSet cells can be replaced independently of shoreline templates.
pub(crate) fn flat_water(scene: &MapScene, v: TerrainValue) -> bool {
    id(scene, v)
        .filter(|&tile| water_tile(scene, tile))
        .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
        .and_then(|t| t.cells.get(usize::from(v.sub)).copied().flatten())
        .is_some_and(|sub| sub.height == 0 && sub.slope == 0)
}

/// Shore beach cells are the wet half of the authored shoreline, even though
/// the engine lets amphibious/ground units walk on those cells.
pub fn wet(scene: &MapScene, v: TerrainValue) -> bool {
    id(scene, v).is_some_and(|tile| {
        water_tile(scene, tile)
            || shore(scene, tile)
                && scene
                    .palette
                    .templates
                    .get(&(tile, 0))
                    .and_then(|t| t.cells.get(usize::from(v.sub)))
                    .and_then(|s| *s)
                    .is_some_and(|s| matches!(s.terrain, 9 | 10))
    })
}

fn fit_water(scene: &MapScene, v: TerrainValue, painting_water: bool) -> bool {
    // Frozen cells still belong to the basin when painting water. Do not
    // build a stock shoreline between the stroke and remaining ice.
    wet(scene, v) || painting_water && ice_tools::bundled_tile(scene, v)
}

pub(crate) fn ground(scene: &MapScene, v: TerrainValue) -> bool {
    id(scene, v).is_some_and(|tile| {
        scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
            t.width == 1
                && t.height == 1
                && t.cells
                    .first()
                    .and_then(|s| *s)
                    .is_some_and(|s| s.height == 0 && s.slope == 0)
        }) && (scene.palette.tile_ids.is_some_and(|ids| {
            let tile = i32::from(tile);
            tile == ids.clear
                || [
                    (ids.rough, ids.rough_lat),
                    (ids.sand, ids.sand_lat),
                    (ids.green, ids.green_lat),
                    (ids.pave, ids.pave_lat),
                ]
                .into_iter()
                .any(|(base, lat)| {
                    base >= 0 && tile == base || lat >= 0 && (lat..lat + 16).contains(&tile)
                })
        }) || scene.palette.morphable.contains(&tile))
    })
}
pub(crate) fn protect(
    doc: &MapDocument,
    scene: &MapScene,
    edits: &BTreeMap<Cell, TerrainValue>,
) -> Result<()> {
    for (&at, &v) in edits {
        doc.check_cell(at)?;
        ensure!(v.level <= 127, "地形高度超过 127 层");
        if v != value(doc, scene, at) {
            let tile = id(scene, v).context("自动地形无法映射到游戏素材")?;
            ensure!(
                scene
                    .palette
                    .images
                    .contains_key(&vera20k_core::map::theater::TileKey {
                        tile_id: tile,
                        sub_tile: v.sub,
                        variant: 0
                    }),
                "{},{} 缺少目标地形资源，未修改地图",
                at[0],
                at[1]
            );
            ensure!(
                doc.overlay(at).0 == 255,
                "{},{} 有覆盖物或桥面，未修改地图",
                at[0],
                at[1]
            );
        }
    }
    for record in doc
        .records()
        .values()
        .filter(|r| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in crate::placement::footprint(scene, record)? {
            ensure!(
                edits.get(&at).is_none_or(|v| *v == value(doc, scene, at)),
                "{},{} 有对象或建筑占地，未修改地图",
                at[0],
                at[1]
            );
        }
    }
    Ok(())
}
type Edge = (Cell, Cell);
fn edge(a: Cell, b: Cell) -> Edge {
    if a < b { (a, b) } else { (b, a) }
}
struct Candidate {
    stamp: CliffStamp,
    cells: Vec<(Cell, TerrainValue)>,
    edges: Vec<usize>,
}

#[derive(Debug)]
struct ShoreGeometryError(&'static str);

const NO_SHORE_FIT: ShoreGeometryError = ShoreGeometryError(
    "岸线过窄、斜角相接或资源缺失，无法完整拼接；请让水道和陆地至少宽 2 格并加宽急转角",
);
impl std::fmt::Display for ShoreGeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ShoreGeometryError {}

/// Cover every water/land edge exactly once, with disjoint, complete templates.
/// Explicit search frames keep long coastlines off the call stack.
fn solve(candidates: &[Candidate], by_edge: &[Vec<usize>]) -> Result<Vec<usize>> {
    struct Frame {
        options: Vec<usize>,
        next: usize,
        active: Option<usize>,
    }
    let mut occupied = BTreeSet::new();
    let mut covered = vec![false; by_edge.len()];
    let mut stack: Vec<Frame> = Vec::new();
    let mut steps = 0;
    loop {
        steps += 1;
        if steps > 50_000 {
            return Err(
                ShoreGeometryError("岸线转角过密，无法构造完整模板；请加宽水道或分段绘制").into(),
            );
        }
        let mut best: Option<Vec<usize>> = None;
        for (_, choices) in by_edge.iter().enumerate().filter(|(e, _)| !covered[*e]) {
            let options: Vec<_> = choices
                .iter()
                .copied()
                .filter(|&c| {
                    candidates[c]
                        .cells
                        .iter()
                        .all(|(at, _)| !occupied.contains(at))
                })
                .collect();
            if best.as_ref().is_none_or(|b| options.len() < b.len()) {
                best = Some(options);
            }
            if best.as_ref().is_some_and(|b| b.len() <= 1) {
                break;
            }
        }
        if best.is_none() {
            return Ok(stack.iter().filter_map(|f| f.active).collect());
        }
        stack.push(Frame {
            options: best.unwrap(),
            next: 0,
            active: None,
        });
        loop {
            let Some(f) = stack.last_mut() else {
                return Err(NO_SHORE_FIT.into());
            };
            if let Some(c) = f.active.take() {
                for (at, _) in &candidates[c].cells {
                    occupied.remove(at);
                }
                for &e in &candidates[c].edges {
                    covered[e] = false;
                }
            }
            if let Some(&c) = f.options.get(f.next) {
                f.next += 1;
                f.active = Some(c);
                for (at, _) in &candidates[c].cells {
                    occupied.insert(*at);
                }
                for &e in &candidates[c].edges {
                    covered[e] = true;
                }
                break;
            }
            stack.pop();
        }
    }
}

/// Close a click-authored path with a straight edge and fill its interior.
/// Validate that edge together with the drawn edges before rasterizing.
pub fn polyline(doc: &MapDocument, scene: &MapScene, path: &[Cell]) -> Result<PlateauPlan> {
    let mut path = path.to_vec();
    path.dedup();
    if path.len() > 1 && path.first() == path.last() {
        path.pop();
    }
    ensure!(path.len() >= 3, "折线水域需要至少三个不同的折点");
    cliff_tools::validate_polyline_coords(doc, &path)?;
    path.push(path[0]);
    cliff_tools::validate_polyline_path(
        &path,
        "水域折线不能回折重叠，请撤回或补画折点",
        "水域轮廓不能自交或重复经过折点，请撤回或补画折点",
    )?;
    let mut selection = polygon_cells(&path)?;
    selection.retain(|&at| doc.contains_cell(at));
    fit_outline(doc, scene, selection)
}

/// Fill a validated, closed polygon using its actual edge crossings. A flood
/// on rounded edges can mistake a dry bay with a narrow mouth for an interior.
fn polygon_cells(path: &[Cell]) -> Result<BTreeSet<Cell>> {
    let mut cells = BTreeSet::new();
    let mut crossings = BTreeMap::<u16, Vec<u16>>::new();
    for edge in path.windows(2) {
        let (mut a, mut b) = (edge[0], edge[1]);
        cells.extend(cell_line(a, b));
        if a[1] > b[1] {
            std::mem::swap(&mut a, &mut b);
        }
        // Half-open Y counts a shared vertex once when the path crosses the
        // row. Round each X up for the half-open integer span between crossings.
        for y in a[1]..b[1] {
            let numerator =
                u32::from(a[0]) * u32::from(b[1] - y) + u32::from(b[0]) * u32::from(y - a[1]);
            crossings
                .entry(y)
                .or_default()
                .push(numerator.div_ceil(u32::from(b[1] - a[1])) as u16);
        }
    }
    let mut has_interior = false;
    for (y, mut row) in crossings {
        row.sort_unstable();
        for &[left, right] in row.as_chunks::<2>().0 {
            for x in left..right {
                has_interior |= cells.insert([x, y]);
            }
        }
    }
    ensure!(has_interior, "轮廓没有围出内部区域，请扩大轮廓");
    Ok(cells)
}

/// Freehand sampling produces one-cell stair steps that complete shore TMPs
/// cannot cover. Keep exact outlines when possible; otherwise expand to 2x2
/// blocks, adding at most one cell per axis around the enclosed selection.
/// Only tiling failures trigger snapping; both attempts validate terrain and protection.
pub fn outline(doc: &MapDocument, scene: &MapScene, path: &[Cell]) -> Result<PlateauPlan> {
    let mut selection = cliff_tools::enclose(path)?;
    selection.retain(|&at| doc.check_cell(at).is_ok());
    fit_outline(doc, scene, selection)
}

fn fit_outline(
    doc: &MapDocument,
    scene: &MapScene,
    selection: BTreeSet<Cell>,
) -> Result<PlateauPlan> {
    match plan(doc, scene, selection.clone(), Some(true)) {
        Ok(plan) => Ok(plan),
        Err(e) if e.is::<ShoreGeometryError>() => {
            let mut snapped = BTreeSet::new();
            for at in selection {
                let x = at[0] / 2 * 2;
                let y = at[1] / 2 * 2;
                snapped.extend([[x, y], [x + 1, y], [x, y + 1], [x + 1, y + 1]]);
            }
            snapped.retain(|&at| doc.check_cell(at).is_ok());
            plan(doc, scene, snapped, Some(true))
        }
        Err(e) => Err(e),
    }
}

/// ShorePieces end in GreenTile art, not ClearTile. Surround their dry cells
/// with that ground family, then let LAT blend its outer edge back to clear.
/// The engine's Green LAT group explicitly connects to ShorePieces.
fn ground_transitions(
    doc: &MapDocument,
    scene: &MapScene,
    zone: &BTreeSet<Cell>,
    plan: &mut PlateauPlan,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let mut scratch = doc.fork();
    plan.apply(&mut scratch)?;
    let mut touched: BTreeSet<_> = plan.edits.keys().copied().collect();
    let shores: Vec<_> = zone
        .iter()
        .copied()
        .filter(|&at| id(scene, value(&scratch, scene, at)).is_some_and(|t| shore(scene, t)))
        .collect();
    touched.extend(&shores);
    if ids.green >= 0 && ids.green_lat >= 0 {
        let mut green = None;
        for at in shores {
            let v = value(&scratch, scene, at);
            if wet(scene, v) {
                continue;
            }
            // Diagonal neighbours also need a transition at convex corners.
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(n) = offset(at, dx, dy).filter(|&n| doc.check_cell(n).is_ok()) else {
                        continue;
                    };
                    let old = value(&scratch, scene, n);
                    if id(scene, old).is_some_and(|t| i32::from(t) == ids.clear)
                        && ground(scene, old)
                        && old.level == v.level
                    {
                        let tile = if let Some(tile) = green {
                            tile
                        } else {
                            // recalculate skips unavailable art for manual brushes;
                            // automatic shores need a complete transition family.
                            for tile in std::iter::once(ids.green)
                                .chain(ids.green_lat + 1..ids.green_lat + 16)
                            {
                                let tile = u16::try_from(tile).context("岸边地表编号无效")?;
                                ensure!(
                                    scene.palette.images.contains_key(
                                        &vera20k_core::map::theater::TileKey {
                                            tile_id: tile,
                                            sub_tile: 0,
                                            variant: 0
                                        }
                                    ) && scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
                                        t.width == 1
                                            && t.height == 1
                                            && t.cells
                                                .first()
                                                .and_then(|s| *s)
                                                .is_some_and(|s| s.height == 0 && s.slope == 0)
                                    }) && scene.catalog.raw_tiles.contains_key(&tile),
                                    "岸边地表过渡资源不完整，未修改地图"
                                );
                            }
                            let tile = scene.catalog.raw_tiles[&(ids.green as u16)];
                            green = Some(tile);
                            tile
                        };
                        scratch.set(
                            Target::Terrain(n),
                            Some(Value::Terrain(TerrainValue {
                                tile,
                                sub: 0,
                                ..old
                            })),
                        )?;
                        touched.insert(n);
                    }
                }
            }
        }
    }
    if !touched.is_empty() {
        let changed = terrain_tools::recalculate(&mut scratch, scene, &touched)?;
        touched.extend(changed);
        for at in touched {
            let v = value(&scratch, scene, at);
            // Keep complete shore/cliff templates, including their subtiles.
            if ground(scene, v) && v != value(doc, scene, at) {
                plan.edits.insert(at, v);
            }
        }
    }
    Ok(())
}

/// Include both sides of painted edges and the complete footprints of
/// existing pieces they touch. The fitting apron alone never selects a piece.
fn repair_scope(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
    zone: &BTreeSet<Cell>,
) -> Result<BTreeSet<Cell>> {
    let mut scope = selection.clone();
    for &at in selection {
        let w = wet(scene, value(doc, scene, at));
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
            if let Some(n) = offset(at, dx, dy).filter(|n| zone.contains(n))
                && wet(scene, value(doc, scene, n)) != w
            {
                scope.insert(n);
            }
        }
    }
    let mut pieces = BTreeSet::new();
    let mut invalid = Vec::new();
    for &at in zone {
        let v = value(doc, scene, at);
        let Some(tile) = id(scene, v).filter(|&t| shore(scene, t)) else {
            continue;
        };
        let Some(template) = scene.palette.templates.get(&(tile, 0)).filter(|template| {
            template.width > 0
                && template
                    .cells
                    .get(usize::from(v.sub))
                    .is_some_and(Option::is_some)
        }) else {
            invalid.push(at);
            continue;
        };
        let width = i32::from(template.width);
        let sub = i32::from(v.sub);
        let footprint: Vec<_> = template
            .cells
            .iter()
            .enumerate()
            .filter_map(|(i, cell)| {
                cell.as_ref()?;
                let i = i as i32;
                offset(at, i % width - sub % width, i / width - sub / width)
                    .filter(|&p| doc.check_cell(p).is_ok())
            })
            .collect();
        pieces.insert(footprint);
    }
    loop {
        let before = scope.len();
        pieces.retain(|footprint| {
            if footprint.iter().any(|at| scope.contains(at)) {
                scope.extend(footprint);
                false
            } else {
                true
            }
        });
        if scope.len() == before {
            break;
        }
    }
    ensure!(invalid.iter().all(|at| !scope.contains(at)), "海岸子块无效");
    Ok(scope)
}

/// Try small 2×2 steps when intact TMPs cannot fit the original contour.
/// Each attempt changes water extent only on brush cells in mixed blocks.
fn snapped_repair(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
    area: &FitArea,
) -> Result<Option<PlateauPlan>> {
    let mut attempts = Vec::new();
    for phase_x in 0..=1 {
        for phase_y in 0..=1 {
            let origins: BTreeSet<_> = selection
                .iter()
                .filter_map(|&[x, y]| {
                    Some([
                        (x.checked_sub(phase_x)? & !1) + phase_x,
                        (y.checked_sub(phase_y)? & !1) + phase_y,
                    ])
                })
                .collect();
            let mut candidates: [BTreeMap<Cell, bool>; 4] = Default::default();
            for [x, y] in origins {
                let block = [[x, y], [x + 1, y], [x, y + 1], [x + 1, y + 1]];
                if block.iter().any(|&at| doc.check_cell(at).is_err()) {
                    continue;
                }
                let values = block.map(|at| value(doc, scene, at));
                let waters = values.map(|v| wet(scene, v));
                let count = waters.iter().filter(|&&w| w).count();
                if count == 0 || count == 4 || values.iter().any(|v| v.level != values[0].level) {
                    continue;
                }
                for (target, changes) in [count >= 2, count > 2, true, false]
                    .into_iter()
                    .zip(&mut candidates)
                {
                    let changed: Vec<_> = block
                        .into_iter()
                        .zip(waters)
                        .enumerate()
                        .filter(|&(_, (_, w))| w != target)
                        .collect();
                    if changed.iter().all(|&(i, (at, _))| {
                        selection.contains(&at)
                            && (ground(scene, values[i])
                                || flat_water(scene, values[i])
                                || id(scene, values[i]).is_some_and(|tile| shore(scene, tile)))
                    }) {
                        changes.extend(changed.into_iter().map(|(_, (at, _))| (at, target)));
                    }
                }
            }
            attempts.extend(candidates.into_iter().filter(|changes| !changes.is_empty()));
        }
    }
    attempts.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    attempts.dedup();
    'attempt: for changes in attempts {
        let mut logical = area.logical.clone();
        let mut edges = area.edges.clone();
        let mut edits = area.edits.clone();
        for (&at, &water) in &changes {
            let old = value(doc, scene, at);
            logical.insert(at, (water, old.level));
            edits.insert(at, base_terrain(scene, old, water, area.water)?);
        }
        // A snap only changes selected cells. Unchanged edges already
        // passed validation and belong to the same fit area.
        for &at in changes.keys() {
            for neighbor in terrain_tools::neighbors(at) {
                edges.remove(&edge(at, neighbor));
            }
        }
        for &at in changes.keys() {
            // Unlike an existing height mismatch, one introduced by a
            // proposed snap only invalidates this particular contour.
            if collect_edges(doc, scene, &logical, &mut edges, at, false).is_err() {
                continue 'attempt;
            }
        }
        match fit_plan(doc, scene, selection, area, &logical, edges, edits) {
            Ok(plan) if plan.cliffs.iter().any(|stamp| shore(scene, stamp.tile)) => {
                return Ok(Some(plan));
            }
            // Avoid a "repair" that only fills or drains an isolated feature.
            Ok(_) => continue,
            Err(e) if e.is::<ShoreGeometryError>() => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

/// `paint`: Some(true) adds water, Some(false) fills land, None repairs only.
/// Existing shores in the neighbourhood are first decoded back into wet/dry
/// cells, so extending, cutting, and filling remove obsolete corners as well.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    selection: BTreeSet<Cell>,
    paint: Option<bool>,
) -> Result<PlateauPlan> {
    let area = prepare_fit_area(doc, scene, &selection, paint)?;
    match fit_plan(
        doc,
        scene,
        &selection,
        &area,
        &area.logical,
        area.edges.clone(),
        area.edits.clone(),
    ) {
        Err(e) if paint.is_none() && e.is::<ShoreGeometryError>() => {
            snapped_repair(doc, scene, &selection, &area)?.ok_or(e)
        }
        result => result,
    }
}

struct FitArea {
    zone: BTreeSet<Cell>,
    repair: Option<BTreeSet<Cell>>,
    styles: BTreeMap<u16, Vec<u16>>,
    water: i32,
    painting_water: bool,
    logical: HashMap<Cell, (bool, u8)>,
    edges: BTreeSet<Edge>,
    edits: BTreeMap<Cell, TerrainValue>,
}

fn prepare_fit_area(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
    paint: Option<bool>,
) -> Result<FitArea> {
    ensure!(!selection.is_empty(), "请先选择水域范围");
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    ensure!(
        ids.water_base >= 0 && ids.shore >= 0,
        "当前环境没有水面与海岸模板"
    );
    // Expand the three-cell apron one axis at a time. Clip only after both
    // passes so a diagonal expansion can reenter the playable diamond.
    let mut columns = BTreeSet::new();
    for &at in selection {
        doc.check_cell(at)?;
        columns.extend((-3..=3).filter_map(|dx| offset(at, dx, 0)));
    }
    let mut zone = BTreeSet::new();
    for at in columns {
        zone.extend((-3..=3).filter_map(|dy| offset(at, 0, dy)));
    }
    zone.retain(|&at| doc.check_cell(at).is_ok());
    let repair = if paint.is_none() {
        let scope = repair_scope(doc, scene, selection, &zone)?;
        zone.extend(&scope);
        Some(scope)
    } else {
        // Include every cell of old templates crossing the local paint border.
        let mut examined = BTreeSet::new();
        loop {
            let pending: Vec<_> = zone.difference(&examined).copied().collect();
            if pending.is_empty() {
                break;
            }
            for at in pending {
                examined.insert(at);
                let v = value(doc, scene, at);
                if let Some(tile) = id(scene, v).filter(|&t| shore(scene, t)) {
                    zone.extend(existing_shore(doc, scene, at, tile, v)?.into_keys());
                }
            }
        }
        None
    };
    let water_id = plain_water_tile(scene)?;
    let water = *scene
        .catalog
        .raw_tiles
        .get(&water_id)
        .context("缺少可保存的水面编号")?;
    cliff_authoring::stamp(doc, scene, water_id, *selection.first().unwrap(), 0)?;
    let scope = repair.as_ref().unwrap_or(&zone);
    let mut logical = HashMap::with_capacity(zone.len());
    let mut edits = BTreeMap::new();
    let painting_water = paint == Some(true);
    for &at in &zone {
        let old = value(doc, scene, at);
        let is_shore = id(scene, old).is_some_and(|t| shore(scene, t));
        let old_wet = fit_water(scene, old, painting_water);
        let painted = paint.filter(|_| selection.contains(&at));
        if painted.is_some() {
            ensure!(
                is_shore || old_wet || ground(scene, old),
                "{},{} 不是可自动拼接的平地或水面，请避开道路、悬崖和坡道",
                at[0],
                at[1],
            );
        }
        let w = painted.unwrap_or(old_wet);
        logical.insert(at, (w, old.level));
        if (is_shore && scope.contains(&at)) || painted.is_some() {
            edits.insert(at, base_terrain(scene, old, w, water)?);
        }
    }
    let mut edges = BTreeSet::new();
    // Only edges with at least one selected or edited endpoint need a fit.
    for &at in selection
        .iter()
        .chain(edits.keys().filter(|&at| !selection.contains(at)))
    {
        collect_edges(doc, scene, &logical, &mut edges, at, painting_water)?;
    }
    Ok(FitArea {
        zone,
        repair,
        styles: shore_styles(scene, ids.shore),
        water,
        painting_water,
        logical,
        edges,
        edits,
    })
}

fn base_terrain(
    scene: &MapScene,
    old: TerrainValue,
    wet: bool,
    water: i32,
) -> Result<TerrainValue> {
    if wet {
        Ok(TerrainValue {
            tile: water,
            sub: 0,
            ice: 0,
            ..old
        })
    } else {
        cliff_authoring::clear(scene, old, old.level)
    }
}

fn collect_edges(
    doc: &MapDocument,
    scene: &MapScene,
    logical: &HashMap<Cell, (bool, u8)>,
    edges: &mut BTreeSet<Edge>,
    at: Cell,
    painting_water: bool,
) -> Result<()> {
    let (w, level) = logical[&at];
    for n in terrain_tools::neighbors(at) {
        let (nw, nl) = if let Some(&neighbor) = logical.get(&n) {
            neighbor
        } else if doc.check_cell(n).is_ok() {
            let v = value(doc, scene, n);
            (fit_water(scene, v, painting_water), v.level)
        } else {
            continue;
        };
        let dry = if w { n } else { at };
        if w != nw && !crate::coastal_cliffs::barrier(scene, value(doc, scene, dry)) {
            ensure!(level == nl, "水陆边界高度不同，请使用悬崖或先平整地形");
            edges.insert(edge(at, n));
        }
    }
    Ok(())
}

fn fit_plan(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
    area: &FitArea,
    logical: &HashMap<Cell, (bool, u8)>,
    edges: BTreeSet<Edge>,
    mut edits: BTreeMap<Cell, TerrainValue>,
) -> Result<PlateauPlan> {
    let scope = area.repair.as_ref().unwrap_or(&area.zone);
    let fitted = fit_pieces(
        doc,
        scene,
        logical,
        edges,
        area.styles.keys().copied(),
        |p| {
            let old = value(doc, scene, p);
            if id(scene, old).is_some_and(|t| shore(scene, t)) {
                scope.contains(&p)
            } else if ice_tools::bundled_tile(scene, old) {
                // Only an ice cell explicitly replaced by this stroke can
                // participate in a new, complete shore template.
                edits.contains_key(&p)
            } else {
                ground(scene, old) || wet(scene, old)
            }
        },
    )?;
    let mut cliffs = Vec::new();
    for mut candidate in fitted {
        // Use the anchor to keep the same look through preview, save, and repair.
        let tile = area.styles[&candidate.stamp.tile]
            .iter()
            .copied()
            .min_by_key(|&tile| style_score(candidate.stamp.origin, tile))
            .unwrap_or(candidate.stamp.tile);
        if tile != candidate.stamp.tile {
            candidate.stamp.tile = tile;
            let raw = scene.catalog.raw_tiles[&tile];
            for (_, cell) in &mut candidate.cells {
                cell.tile = raw;
            }
        }
        edits.extend(candidate.cells);
        cliffs.push(candidate.stamp);
    }
    cliffs.extend(crate::coastal_cliffs::gather(doc, scene, scope)?);
    let mut result = PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs,
        interior: selection.clone(),
        base: 0,
    };
    if scene
        .palette
        .tile_ids
        .is_some_and(|i| i.cliffs.cliff_set.is_some())
    {
        crate::coastal_cliffs::adapt(doc, scene, &mut result)?;
    }
    if let Some(scope) = &area.repair {
        let mut transitions = scope.clone();
        transitions.extend(result.edits.keys().copied());
        ground_transitions(doc, scene, &transitions, &mut result)?;
    } else {
        ground_transitions(doc, scene, &area.zone, &mut result)?;
    }
    if area.painting_water {
        ice_tools::refresh_after_water(doc, scene, selection, area.water, &mut result.edits)?;
    }
    result.edits.retain(|&at, v| {
        let old = value(doc, scene, at);
        // A fitted shoreline template preserves the old cell's ice byte by
        // default. Clear it too if the water stroke replaced an ice tile.
        if selection.contains(&at) && ice_tools::bundled_tile(scene, old) {
            v.ice = 0;
        }
        *v != old
    });
    protect(doc, scene, &result.edits)?;
    Ok(result)
}

/// Index the four oriented water/land joins by axis and the wet side of
/// the first endpoint in grid order.
fn join_direction(along_y: bool, first_wet: bool) -> usize {
    2 * usize::from(along_y) + usize::from(first_wet)
}

struct ShorePattern {
    tile: u16,
    cells: Vec<(i32, i32, bool, u8)>,
    joins: [Vec<[i32; 2]>; 4],
}

impl ShorePattern {
    fn new(scene: &MapScene, tile: u16) -> Option<Self> {
        let template = &scene.palette.templates[&(tile, 0)];
        if template.width == 0 || template.cells.first().is_none_or(Option::is_none) {
            return None;
        }
        let width = i32::from(template.width);
        let cells: Vec<_> = template
            .cells
            .iter()
            .enumerate()
            .filter_map(|(sub, meta)| {
                let meta = (*meta)?;
                Some((
                    sub as i32 % width,
                    sub as i32 / width,
                    matches!(meta.terrain, 9 | 10),
                    meta.height,
                ))
            })
            .collect();
        if template.cells.iter().flatten().any(|meta| meta.slope != 0) {
            return None;
        }
        let mut joins: [Vec<_>; 4] = Default::default();
        for &(x, y, wet, _) in &cells {
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if cells
                    .iter()
                    .any(|&(px, py, w, _)| (px, py) == (nx, ny) && w != wet)
                {
                    joins[join_direction(x == nx, wet)].push([x, y]);
                }
            }
        }
        Some(Self { tile, cells, joins })
    }
}

fn fit_pieces(
    doc: &MapDocument,
    scene: &MapScene,
    logical: &HashMap<Cell, (bool, u8)>,
    edges: BTreeSet<Edge>,
    tiles: impl Iterator<Item = u16>,
    replaceable: impl Fn(Cell) -> bool,
) -> Result<Vec<Candidate>> {
    let edge_list: Vec<_> = edges.into_iter().collect();
    let mut incident: HashMap<Cell, Vec<usize>> = HashMap::new();
    for (i, &(a, b)) in edge_list.iter().enumerate() {
        incident.entry(a).or_default().push(i);
        incident.entry(b).or_default().push(i);
    }
    let patterns: Vec<_> = tiles
        .filter_map(|tile| ShorePattern::new(scene, tile))
        .collect();
    let mut origins = HashSet::new();
    for &(a, b) in &edge_list {
        for p in [a, b] {
            for dx in -2..=0 {
                for dy in -2..=0 {
                    if let Some(p) = offset(p, dx, dy)
                        && logical.contains_key(&p)
                    {
                        origins.insert(p);
                    }
                }
            }
        }
    }
    let mut candidates = Vec::new();
    let mut has_candidate = vec![false; edge_list.len()];
    let mut tried = HashSet::new();
    // Align template water/land joins to actual coast edges. This avoids
    // stamping every shape at every nearby anchor, and lets us stop as soon
    // as an edge cannot be covered by any complete template.
    for (i, &(a, b)) in edge_list.iter().enumerate() {
        let Some(&(wet_a, _)) = logical.get(&a) else {
            return Err(NO_SHORE_FIT.into());
        };
        let direction = join_direction(a[0] == b[0], wet_a);
        for pattern in &patterns {
            for &[dx, dy] in &pattern.joins[direction] {
                let Some(at) = offset(a, -dx, -dy).filter(|at| origins.contains(at)) else {
                    continue;
                };
                if !tried.insert((pattern.tile, at)) {
                    continue;
                }
                let base = logical[&at].1;
                if !pattern.cells.iter().all(|&(dx, dy, wet, height)| {
                    offset(at, dx, dy).is_some_and(|p| {
                        logical.get(&p).is_some_and(|&(expected_wet, level)| {
                            wet == expected_wet
                                && level.checked_sub(base) == Some(height)
                                && replaceable(p)
                        })
                    })
                }) {
                    continue;
                }
                let Ok(cells) = cliff_authoring::stamp(doc, scene, pattern.tile, at, base) else {
                    continue;
                };
                let mut covered = Vec::new();
                let mut complete = true;
                for p in cells.keys() {
                    for &e in incident.get(p).into_iter().flatten() {
                        let (a, b) = edge_list[e];
                        if !cells.contains_key(if *p == a { &b } else { &a }) {
                            complete = false;
                            break;
                        }
                        if *p == a {
                            covered.push(e);
                        }
                    }
                    if !complete {
                        break;
                    }
                }
                if !complete {
                    continue;
                }
                for &e in &covered {
                    has_candidate[e] = true;
                }
                candidates.push(Candidate {
                    stamp: CliffStamp {
                        tile: pattern.tile,
                        origin: at,
                        base,
                    },
                    cells: cells.into_iter().collect(),
                    edges: covered,
                });
            }
        }
        if !has_candidate[i] {
            return Err(NO_SHORE_FIT.into());
        }
    }
    candidates.sort_by_key(|c| {
        (
            std::cmp::Reverse(c.edges.len()),
            std::cmp::Reverse(c.cells.len()),
            c.stamp.tile,
            c.stamp.origin,
        )
    });
    let mut by_edge = vec![Vec::new(); edge_list.len()];
    for (c, candidate) in candidates.iter().enumerate() {
        for &e in &candidate.edges {
            by_edge[e].push(c);
        }
    }
    let chosen = solve(&candidates, &by_edge)?;
    let mut candidates: Vec<_> = candidates.into_iter().map(Some).collect();
    Ok(chosen
        .into_iter()
        .filter_map(|i| candidates[i].take())
        .collect())
}

/// Solve with one representative per geometry. Retain identical TMP
/// metadata as alternate art for that representative.
fn shore_styles(scene: &MapScene, shore_base: i32) -> BTreeMap<u16, Vec<u16>> {
    let mut geometry = BTreeMap::new();
    let mut styles = BTreeMap::<_, Vec<_>>::new();
    // The two decorative beach scenes after these 40 pieces do not join.
    for tile in (shore_base..shore_base + 40).filter_map(|tile| u16::try_from(tile).ok()) {
        if !available(scene, tile) {
            continue;
        }
        let t = &scene.palette.templates[&(tile, 0)];
        let pattern: Vec<_> = t
            .cells
            .iter()
            .map(|s| s.map(|s| (matches!(s.terrain, 9 | 10), s.height, s.slope)))
            .collect();
        let representative = *geometry.entry((t.width, t.height, pattern)).or_insert(tile);
        let original = &scene.palette.templates[&(representative, 0)];
        if t.cells
            .iter()
            .map(|s| s.map(|s| s.terrain))
            .eq(original.cells.iter().map(|s| s.map(|s| s.terrain)))
        {
            styles.entry(representative).or_default().push(tile);
        }
    }
    styles
}

fn plain_water_tile(scene: &MapScene) -> Result<u16> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    Ok((ids.water_base..ids.water_base + 14)
        .filter(|&tile| water_tile(scene, tile as u16))
        .find(|&id| {
            scene
                .palette
                .templates
                .get(&(id as u16, 0))
                .is_some_and(|t| {
                    t.width == 1
                        && t.height == 1
                        && t.cells
                            .first()
                            .and_then(|s| *s)
                            .is_some_and(|s| s.height == 0 && s.slope == 0 && s.terrain == 9)
                })
        })
        .context("缺少 1×1 纯水面模板")? as u16)
}

/// Match a complete, flat ShorePieces template with one dry subtile.
fn shore_tip(scene: &MapScene, shore_base: u16, size: [u16; 2], dry: usize) -> Option<u16> {
    (shore_base..shore_base.saturating_add(40)).find(|&tile| {
        crate::coastal_cliffs::same_tileset(scene, shore_base, tile)
            && crate::cliff_variants::available(scene, tile)
            && {
                let template = &scene.palette.templates[&(tile, 0)];
                [template.width, template.height] == size
                    && template.cells.len() == usize::from(size[0] * size[1])
                    && template.cells.iter().enumerate().all(|(i, s)| {
                        s.is_some_and(|s| {
                            s.height == 0
                                && s.slope == 0
                                && matches!(s.terrain, 9 | 10) == (i != dry)
                        })
                    })
            }
    })
}

fn has_low_edge(scene: &MapScene, v: TerrainValue, base: u8, edge: [usize; 2]) -> bool {
    v.level == base
        && id(scene, v)
            .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
            .and_then(|t| t.cells.get(usize::from(v.sub)).copied().flatten())
            .filter(|s| s.slope != 0)
            .and_then(|s| crate::flatten_tools::RAMPS.get(usize::from(s.slope)))
            .is_some_and(|corners| edge.into_iter().all(|i| corners[i] == 0))
}

const SHORE_EDGES: [([i32; 2], [usize; 2]); 4] = [
    ([1, 0], [1, 2]),
    ([-1, 0], [0, 3]),
    ([0, 1], [2, 3]),
    ([0, -1], [0, 1]),
];

#[derive(Clone, Copy)]
struct RampLane {
    dry: Cell,
    wet: Cell,
    outward: [i32; 2],
    round_end: bool,
}

impl RampLane {
    fn straight_tile(self, scene: &MapScene, shore_base: u16) -> Option<u16> {
        let [dx, dy] = self.outward;
        let size = if dx == 0 { [1, 2] } else { [2, 1] };
        shore_tip(scene, shore_base, size, usize::from(dx < 0 || dy < 0))
    }
}

/// Determine where a complete flat shore piece can meet a slope at its base.
/// The small returns turn a straight landing back towards the cliff, along
/// the exposed low side of each triangular ramp cap.
fn ramp_lanes(
    doc: &MapDocument,
    scene: &MapScene,
    ramp: &PlateauPlan,
    outward: [i32; 2],
) -> (Vec<RampLane>, Vec<[RampLane; 2]>) {
    let lane = |at, direction: [i32; 2], round_end| {
        let [dx, dy] = direction;
        let dry = offset(at, dx, dy)?;
        let wet = offset(dry, dx, dy)?;
        [dry, wet]
            .into_iter()
            .all(|p| !ramp.edits.contains_key(&p) && doc.check_cell(p).is_ok())
            .then_some(RampLane {
                dry,
                wet,
                outward: direction,
                round_end,
            })
    };
    let mut front = Vec::new();
    let mut returns = Vec::new();
    for (direction @ [dx, dy], edge) in SHORE_EDGES {
        if dx != 0 && outward[0] != dx || dy != 0 && outward[1] != dy {
            continue;
        }
        for (&at, &v) in &ramp.edits {
            if !has_low_edge(scene, v, ramp.base, edge) {
                continue;
            }
            let Some(contact) = lane(at, direction, true) else {
                continue;
            };
            front.push(contact);
            if direction != outward {
                continue;
            }
            for (side @ [sx, sy], side_edge) in SHORE_EDGES {
                if dx * sx + dy * sy == 0
                    && has_low_edge(scene, v, ramp.base, side_edge)
                    && let Some(flank) = lane(at, side, false)
                    && let Some(turn) = lane(flank.dry, direction, true)
                {
                    returns.push([flank, turn]);
                }
            }
        }
    }
    (front, returns)
}

fn ramp_shore_tiles(scene: &MapScene) -> Option<(u16, [Option<u16>; 4])> {
    let base = u16::try_from(scene.palette.tile_ids?.shore).ok()?;
    Some((
        base,
        std::array::from_fn(|dry| shore_tip(scene, base, [2, 2], dry)),
    ))
}

type RampCoast = (BTreeMap<Cell, TerrainValue>, Vec<CliffStamp>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RampCoastProfile {
    Rounded,
    ProbeOriginal,
    LegacyTaper,
}

/// Reconstruct the footprint independently of what occupies it, so the same
/// complete pieces can also be recognized after saving and reopening.
fn fitted_ramp_coast(
    doc: &MapDocument,
    scene: &MapScene,
    ramp: &PlateauPlan,
    outward: [i32; 2],
    front: &[RampLane],
    returns: &[[RampLane; 2]],
    profile: RampCoastProfile,
) -> Option<RampCoast> {
    let [dx, dy] = outward;
    if dx.abs() + dy.abs() != 1 || front.len() < 4 {
        return None;
    }
    let axis = usize::from(dx != 0);
    let (first, last) = (front.first()?.dry, front.last()?.dry);
    let across = |p, n| offset(p, n * dy.abs(), n * dx.abs());
    let forward = |p, n| offset(p, n * dx, n * dy);
    if first[1 - axis] != last[1 - axis]
        || front.len() != usize::from(last[axis].checked_sub(first[axis])?) + 1
        || front.iter().any(|l| l.dry[1 - axis] != first[1 - axis])
    {
        return None;
    }
    let mut land: BTreeSet<_> = front.iter().map(|l| l.dry).collect();
    let mut cliff_side = BTreeSet::new();
    let probe = std::env::var("COAST_ROOT_PROBE").unwrap_or_default();
    for (p, side) in [(first, -1), (last, 1)] {
        let end = across(p, side)?;
        let [flank, _] = returns.iter().find(|[_, turn]| turn.dry == end)?;
        land.extend([flank.dry, end]);
        if profile != RampCoastProfile::LegacyTaper {
            land.extend([forward(end, 1)?, forward(end, 2)?]);
            if profile == RampCoastProfile::Rounded {
                for (row, flag) in probe.bytes().take(4).enumerate() {
                    let cell = forward(flank.dry, row as i32)?;
                    for extension in 1..=i32::from(flag.saturating_sub(b'0').min(5)) {
                        let added = across(cell, side * extension)?;
                        land.insert(added);
                        if row == 0 {
                            cliff_side.insert(edge(added, forward(added, -1)?));
                        }
                    }
                }
            }
        }
        cliff_side.insert(edge(flank.dry, forward(flank.dry, -1)?));
    }
    let width = i32::try_from(front.len()).ok()?;
    for i in 0..width {
        for step in 1..=2 {
            land.insert(forward(across(first, i)?, step)?);
        }
    }
    let mut logical = HashMap::new();
    let mut edges = BTreeSet::new();
    for &at in &land {
        doc.check_cell(at).ok()?;
        if ramp.edits.contains_key(&at) {
            return None;
        }
        for sx in -2..=2 {
            for sy in -2..=2 {
                if let Some(n) = offset(at, sx, sy)
                    && !ramp.edits.contains_key(&n)
                    && doc.check_cell(n).is_ok()
                {
                    logical.insert(n, (!land.contains(&n), ramp.base));
                }
            }
        }
        for n in terrain_tools::neighbors(at) {
            if !land.contains(&n)
                && !ramp.edits.contains_key(&n)
                && !cliff_side.contains(&edge(at, n))
            {
                edges.insert(edge(at, n));
            }
        }
    }
    let mut edits = BTreeMap::new();
    let mut stamps = Vec::new();
    let ids = scene.palette.tile_ids?;
    // Ramp recognition also reconstructs legacy approaches with these IDs.
    let styles = shore_styles(scene, ids.shore);
    for part in fit_pieces(doc, scene, &logical, edges, styles.keys().copied(), |_| {
        true
    })
    .ok()?
    {
        edits.extend(part.cells);
        stamps.push(part.stamp);
    }
    let green = u16::try_from(ids.green).ok().filter(|&id| {
        let Some(template) = scene.palette.templates.get(&(id, 0)) else {
            return false;
        };
        [template.width, template.height] == [1, 1]
            && template
                .cells
                .first()
                .copied()
                .flatten()
                .is_some_and(|s| s.height == 0 && s.slope == 0)
            && crate::cliff_variants::available(scene, id)
    });
    let shore_cells: BTreeSet<_> = edits
        .iter()
        .filter_map(|(&p, &v)| (!wet(scene, v)).then_some(p))
        .collect();
    let mut filled = false;
    for at in land {
        if let std::collections::btree_map::Entry::Vacant(entry) = edits.entry(at) {
            let mut v = cliff_authoring::clear(scene, value(doc, scene, at), ramp.base).ok()?;
            // In stock temperate GreenTile is sand. Keep clear ground at
            // the foot, with LAT where the floor meets the front and where
            // it meets the two returns. Do not repaint the ramp subtiles.
            if let Some(green) = green
                && (forward(at, 1).is_some_and(|p| shore_cells.contains(&p))
                    || profile != RampCoastProfile::LegacyTaper
                        && [-1, 1]
                            .into_iter()
                            .any(|side| across(at, side).is_some_and(|p| shore_cells.contains(&p))))
            {
                v.tile = *scene.catalog.raw_tiles.get(&green)?;
            }
            entry.insert(v);
            filled = true;
        }
    }
    filled.then_some((edits, stamps))
}

/// Recover complete shore pieces associated with the saved ramp geometry.
/// This includes the older strip without returns, so it can be repaired or
/// widened in one operation using only terrain stored in the map.
pub(crate) fn ramp_shore_cells(
    doc: &MapDocument,
    scene: &MapScene,
    ramp: &PlateauPlan,
    outward: [i32; 2],
) -> BTreeSet<Cell> {
    let Some((shore_base, corners)) = ramp_shore_tiles(scene) else {
        return BTreeSet::new();
    };
    let (front, returns) = ramp_lanes(doc, scene, ramp, outward);
    for profile in [RampCoastProfile::Rounded, RampCoastProfile::LegacyTaper] {
        let profile = if std::env::var_os("COAST_ROOT_PROBE").is_some()
            && profile == RampCoastProfile::Rounded
        {
            RampCoastProfile::ProbeOriginal
        } else {
            profile
        };
        if let Some((edits, _)) =
            fitted_ramp_coast(doc, scene, ramp, outward, &front, &returns, profile)
            && edits.iter().all(|(&at, &v)| {
                let old = value(doc, scene, at);
                old == v
                    || (TerrainValue {
                        tile: v.tile,
                        ..old
                    } == v
                        && scene.palette.tile_ids.is_some_and(|ids| {
                            id(scene, v).map(i32::from) == Some(ids.green)
                                && id(scene, old).is_some_and(|t| ids.is_green_lat(i32::from(t)))
                        }))
            })
        {
            return edits.into_keys().collect();
        }
    }
    let mut old_shore = BTreeSet::new();
    if let Some(tile) = crate::coastal_cliffs::legacy_ramp_end(scene, outward) {
        for [flank, _] in &returns {
            if id(scene, value(doc, scene, flank.dry)) == Some(tile)
                && let Ok(cells) = cliff_authoring::stamp(doc, scene, tile, flank.dry, ramp.base)
                && cells
                    .iter()
                    .all(|(&at, &v)| !ramp.edits.contains_key(&at) && value(doc, scene, at) == v)
            {
                old_shore.extend(cells.into_keys());
            }
        }
    }
    for lane in front.into_iter().chain(returns.into_iter().flatten()) {
        let old = value(doc, scene, lane.dry);
        if old.level == ramp.base
            && let Some(tile) = id(scene, old).filter(|&tile| shore(scene, tile))
            && let Some(t) = scene.palette.templates.get(&(tile, 0))
            && (lane.straight_tile(scene, shore_base) == Some(tile)
                || ([t.width, t.height] == [2, 2]
                    && corners.get(usize::from(old.sub)) == Some(&Some(tile))))
            && let Ok(cells) = existing_shore(doc, scene, lane.dry, tile, old)
            && cells.get(&lane.wet).is_some_and(|&v| wet(scene, v))
            && cells.keys().all(|p| !ramp.edits.contains_key(p))
        {
            old_shore.extend(cells.into_keys());
        }
    }
    old_shore
}

/// Prefer a rounded stock-TMP shore. In tight spaces or at corners keep a
/// compact shore without changing the ramp's footprint.
pub(crate) fn join_ramp(
    doc: &MapDocument,
    scene: &MapScene,
    ramp: &mut PlateauPlan,
    outward: [i32; 2],
    old_shore: BTreeSet<Cell>,
) -> Result<()> {
    let Some((shore_base, corners)) = ramp_shore_tiles(scene) else {
        return Ok(());
    };
    let base = ramp.base;
    let (front, returns) = ramp_lanes(doc, scene, ramp, outward);
    let water_available = |at| {
        old_shore.contains(&at) || {
            let old = value(doc, scene, at);
            old.level == base && flat_water(scene, old)
        }
    };
    if let Some((edits, stamps)) = fitted_ramp_coast(
        doc,
        scene,
        ramp,
        outward,
        &front,
        &returns,
        RampCoastProfile::Rounded,
    ) && edits.keys().copied().all(water_available)
        && protect(doc, scene, &edits).is_ok()
    {
        return finish_ramp_coast(doc, scene, ramp, old_shore, edits, stamps);
    }
    let usable = |lane: RampLane| {
        lane.straight_tile(scene, shore_base).is_some()
            && [lane.dry, lane.wet].into_iter().all(water_available)
    };
    let mut lanes = BTreeMap::new();
    let mut reserved = BTreeSet::new();
    for lane in front.into_iter().filter(|&lane| usable(lane)) {
        if !reserved.contains(&lane.dry) && !reserved.contains(&lane.wet) {
            reserved.extend([lane.dry, lane.wet]);
            lanes.insert(lane.dry, lane);
        }
    }
    let mut edits = BTreeMap::new();
    let mut stamps = Vec::new();
    for [flank, turn] in returns {
        let [dx, dy] = flank.outward;
        // The return needs room for its outer rounding, or the default
        // front corner is a better fit in the available water.
        let (Some(outer_dry), Some(outer_wet)) =
            (offset(turn.dry, dx, dy), offset(turn.wet, dx, dy))
        else {
            continue;
        };
        let return_cells = [
            flank.dry, flank.wet, turn.dry, turn.wet, outer_dry, outer_wet,
        ];
        if usable(flank)
            && usable(turn)
            && return_cells.into_iter().all(|p| {
                !reserved.contains(&p)
                    && !ramp.edits.contains_key(&p)
                    && doc.check_cell(p).is_ok()
                    && water_available(p)
                    && doc.overlay(p).0 == 255
            })
        {
            for lane in [flank, turn] {
                reserved.extend([lane.dry, lane.wet]);
                lanes.insert(lane.dry, lane);
            }
        }
    }
    for lane in lanes.into_values() {
        let RampLane {
            dry,
            wet,
            outward: [dx, dy],
            round_end,
        } = lane;
        let straight = lane.straight_tile(scene, shore_base).unwrap();
        let corner = round_end
            .then(|| {
                [-1, 1].into_iter().find_map(|sign| {
                    let a = offset(dry, sign * dy.abs(), sign * dx.abs())?;
                    let b = offset(wet, sign * dy.abs(), sign * dx.abs())?;
                    if [a, b].into_iter().any(|p| {
                        reserved.contains(&p)
                            || ramp.edits.contains_key(&p)
                            || edits.contains_key(&p)
                            || doc.check_cell(p).is_err()
                            || !water_available(p)
                    }) {
                        return None;
                    }
                    let origin = [
                        dry[0].min(wet[0]).min(a[0]).min(b[0]),
                        dry[1].min(wet[1]).min(a[1]).min(b[1]),
                    ];
                    let dry_sub = usize::from(2 * (dry[1] - origin[1]) + dry[0] - origin[0]);
                    Some((corners[dry_sub]?, origin))
                })
            })
            .flatten();
        let (tile, origin) = corner.unwrap_or((straight, dry.min(wet)));
        edits.extend(cliff_authoring::stamp(doc, scene, tile, origin, base)?);
        stamps.push(CliffStamp { tile, origin, base });
    }
    finish_ramp_coast(doc, scene, ramp, old_shore, edits, stamps)
}

fn finish_ramp_coast(
    doc: &MapDocument,
    scene: &MapScene,
    ramp: &mut PlateauPlan,
    old_shore: BTreeSet<Cell>,
    mut edits: BTreeMap<Cell, TerrainValue>,
    stamps: Vec<CliffStamp>,
) -> Result<()> {
    let obsolete: Vec<_> = old_shore
        .into_iter()
        .filter(|at| !ramp.edits.contains_key(at) && !edits.contains_key(at))
        .collect();
    if !obsolete.is_empty() {
        let tile = plain_water_tile(scene)?;
        let raw = *scene
            .catalog
            .raw_tiles
            .get(&tile)
            .context("缺少可保存的水面编号")?;
        for at in obsolete {
            edits.insert(
                at,
                TerrainValue {
                    tile: raw,
                    sub: 0,
                    ..value(doc, scene, at)
                },
            );
        }
    }
    // Persist the real grass-to-sand LAT tiles. Restrict writes to the new
    // shore floor so slope fixups and LATs on neighboring land stay intact.
    if let Some(ids) = scene.palette.tile_ids {
        let sand: BTreeSet<_> = edits
            .iter()
            .filter_map(|(&at, &v)| (id(scene, v).map(i32::from) == Some(ids.green)).then_some(at))
            .collect();
        if !sand.is_empty() {
            let mut scratch = doc.fork();
            ramp.apply(&mut scratch)?;
            for (&at, &v) in &edits {
                scratch.set(Target::Terrain(at), Some(Value::Terrain(v)))?;
            }
            terrain_tools::recalculate(&mut scratch, scene, &sand)?;
            for at in sand {
                edits.insert(at, value(&scratch, scene, at));
            }
        }
    }
    protect(doc, scene, &edits)?;
    ramp.edits.extend(edits);
    ramp.cliffs.extend(stamps);
    // Shore edits change which cliff feet touch water. Re-select the matching
    // stock water-cliff TMPs after the final shore footprint is known.
    crate::coastal_cliffs::adapt(doc, scene, ramp)?;
    crate::cliff_variants::apply(doc, scene, ramp);
    Ok(())
}
