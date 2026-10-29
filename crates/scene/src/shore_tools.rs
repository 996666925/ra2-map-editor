//! Local shoreline reconstruction from complete TMP water/land patterns.
//! No random-generator erosion, partial stamps, or changes outside the plan.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(crate) fn offset(at: Cell, dx: i32, dy: i32) -> Option<Cell> {
    let (x, y) = (i32::from(at[0]) + dx, i32::from(at[1]) + dy);
    ((0..512).contains(&x) && (0..512).contains(&y)).then_some([x as u16, y as u16])
}
pub(crate) fn id(scene: &MapScene, v: TerrainValue) -> Option<u16> {
    scene.palette.raw_ids.get(&v.tile).copied()
}
fn shore(scene: &MapScene, tile: u16) -> bool {
    scene
        .palette
        .tile_ids
        .is_some_and(|i| i.shore >= 0 && (i.shore..i.shore + 40).contains(&i32::from(tile)))
}
/// Shore beach cells are the wet half of the authored shoreline, even though
/// the engine lets amphibious/ground units walk on those cells.
pub fn wet(scene: &MapScene, v: TerrainValue) -> bool {
    let Some(i) = scene.palette.tile_ids else {
        return false;
    };
    id(scene, v).is_some_and(|tile| {
        i.water_base >= 0 && (i.water_base..i.water_base + 14).contains(&i32::from(tile))
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
pub(crate) fn ground(scene: &MapScene, v: TerrainValue) -> bool {
    id(scene, v).is_some_and(|tile| {
        scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
            t.width == 1
                && t.height == 1
                && t.cells
                    .first()
                    .and_then(|s| *s)
                    .is_some_and(|s| s.height == 0 && s.slope == 0)
        }) && (scene
            .palette
            .tile_ids
            .is_some_and(|ids| i32::from(tile) == ids.clear)
            || scene.palette.morphable.contains(&tile))
    })
}
pub(crate) fn protect(
    doc: &MapDocument,
    scene: &MapScene,
    edits: &BTreeMap<Cell, TerrainValue>,
) -> Result<()> {
    for (&at, &v) in edits {
        doc.check_cell(at)?;
        ensure!(v.level <= 14, "地形高度超过 14 层");
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
        ensure!(
            steps <= 50_000,
            "岸线转角过密，无法构造完整模板；请加宽水道或分段绘制"
        );
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
                anyhow::bail!(
                    "岸线过窄、斜角相接或资源缺失，无法完整拼接；请让水道和陆地至少宽 2 格并加宽急转角"
                );
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

/// `paint`: Some(true) adds water, Some(false) fills land, None repairs only.
/// Existing shores in the neighbourhood are first decoded back into wet/dry
/// cells, so extending, cutting, and filling remove obsolete corners as well.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    selection: BTreeSet<Cell>,
    paint: Option<bool>,
) -> Result<PlateauPlan> {
    ensure!(!selection.is_empty(), "请先选择水域范围");
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    ensure!(
        ids.water_base >= 0 && ids.shore >= 0,
        "当前环境没有水面与海岸模板"
    );
    let mut zone = selection.clone();
    for &p in &selection {
        doc.check_cell(p)?;
        for dx in -3..=3 {
            for dy in -3..=3 {
                if let Some(n) = offset(p, dx, dy)
                    && doc.check_cell(n).is_ok()
                {
                    zone.insert(n);
                }
            }
        }
    }
    // Include every cell of old templates crossing the local repair border.
    let mut examined = BTreeSet::new();
    loop {
        let pending: Vec<_> = zone.difference(&examined).copied().collect();
        if pending.is_empty() {
            break;
        }
        for at in pending {
            examined.insert(at);
            let v = value(doc, scene, at);
            let Some(tile) = id(scene, v).filter(|&t| shore(scene, t)) else {
                continue;
            };
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
            for (p, expected) in cliff_authoring::stamp(doc, scene, tile, origin, base)? {
                ensure!(
                    value(doc, scene, p) == expected,
                    "{},{} 的海岸模板不完整，请先恢复该模板",
                    p[0],
                    p[1]
                );
                zone.insert(p);
            }
        }
    }
    let mut logical = BTreeMap::new();
    let mut edits = BTreeMap::new();
    let water_id = (ids.water_base..ids.water_base + 14)
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
        .context("缺少 1×1 纯水面模板")? as u16;
    let water = *scene
        .catalog
        .raw_tiles
        .get(&water_id)
        .context("缺少可保存的水面编号")?;
    cliff_authoring::stamp(doc, scene, water_id, *selection.first().unwrap(), 0)?;
    for &at in &zone {
        let old = value(doc, scene, at);
        let is_shore = id(scene, old).is_some_and(|t| shore(scene, t));
        let selected = selection.contains(&at) && paint.is_some();
        if selected {
            ensure!(
                is_shore || wet(scene, old) || ground(scene, old),
                "{},{} 不是可自动拼接的平地或水面，请避开道路、悬崖和坡道",
                at[0],
                at[1]
            );
        }
        let w = if selected {
            paint.unwrap()
        } else {
            wet(scene, old)
        };
        logical.insert(at, (w, old.level));
        if is_shore || selected {
            let mut v = if w {
                TerrainValue {
                    tile: water,
                    sub: 0,
                    ..old
                }
            } else {
                cliff_authoring::clear(scene, old, old.level)?
            };
            if w {
                v.ice = 0;
            }
            edits.insert(at, v);
        }
    }
    let mut edges = BTreeSet::new();
    for (&at, &(w, level)) in &logical {
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
            if let Some(n) = offset(at, dx, dy)
                && doc.check_cell(n).is_ok()
            {
                let (nw, nl) = logical.get(&n).copied().unwrap_or_else(|| {
                    let v = value(doc, scene, n);
                    (wet(scene, v), v.level)
                });
                if w != nw
                    && (edits.contains_key(&at)
                        || edits.contains_key(&n)
                        || selection.contains(&at)
                        || selection.contains(&n))
                {
                    let dry = if w { n } else { at };
                    if crate::coastal_cliffs::barrier(scene, value(doc, scene, dry)) {
                        continue;
                    }
                    ensure!(level == nl, "水陆边界高度不同，请使用悬崖或先平整地形");
                    edges.insert(edge(at, n));
                }
            }
        }
    }
    let edge_list: Vec<_> = edges.iter().copied().collect();
    let mut incident: HashMap<Cell, Vec<usize>> = HashMap::new();
    for (i, &(a, b)) in edges.iter().enumerate() {
        incident.entry(a).or_default().push(i);
        incident.entry(b).or_default().push(i);
    }
    let mut origins = BTreeSet::new();
    for &(a, b) in &edges {
        for p in [a, b] {
            for dx in -2..=0 {
                for dy in -2..=0 {
                    if let Some(p) = offset(p, dx, dy)
                        && zone.contains(&p)
                    {
                        origins.insert(p);
                    }
                }
            }
        }
    }
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    // Only the standard 40 joining pieces. The last two are decorative beach
    // scenes, not repeatable coast geometry.
    for tile in ids.shore..ids.shore + 40 {
        let tile = tile as u16;
        let Some(t) = scene.palette.templates.get(&(tile, 0)) else {
            continue;
        };
        if t.cells.iter().enumerate().any(|(sub, s)| {
            s.is_some()
                && !scene
                    .palette
                    .images
                    .contains_key(&vera20k_core::map::theater::TileKey {
                        tile_id: tile,
                        sub_tile: sub as u8,
                        variant: 0,
                    })
        }) {
            continue;
        }
        let pattern: Vec<_> = t
            .cells
            .iter()
            .map(|s| s.map(|s| (matches!(s.terrain, 9 | 10), s.height, s.slope)))
            .collect();
        if !seen.insert((t.width, t.height, pattern)) {
            continue;
        }
        for &at in &origins {
            let Some(&(w, base)) = logical.get(&at) else {
                continue;
            };
            if t.cells
                .first()
                .and_then(|s| *s)
                .is_none_or(|s| matches!(s.terrain, 9 | 10) != w)
            {
                continue;
            }
            let Ok(cells) = cliff_authoring::stamp(doc, scene, tile, at, base) else {
                continue;
            };
            if !cells.iter().all(|(p, v)| {
                let Some(&(wet, level)) = logical.get(p) else {
                    return false;
                };
                let s = t.cells[usize::from(v.sub)].unwrap();
                let old = value(doc, scene, *p);
                wet == matches!(s.terrain, 9 | 10)
                    && level == v.level
                    && s.slope == 0
                    && (ground(scene, old)
                        || self::wet(scene, old)
                        || id(scene, old).is_some_and(|t| shore(scene, t)))
            }) {
                continue;
            }
            let potential: BTreeSet<_> = cells
                .keys()
                .flat_map(|p| incident.get(p).into_iter().flatten().copied())
                .collect();
            let covered: Vec<_> = potential
                .into_iter()
                .filter(|&e| {
                    let (a, b) = edge_list[e];
                    cells.contains_key(&a) && cells.contains_key(&b)
                })
                .collect();
            if covered.is_empty()
                || cells.keys().any(|p| {
                    incident
                        .get(p)
                        .is_some_and(|es| es.iter().any(|e| !covered.contains(e)))
                })
            {
                continue;
            }
            candidates.push(Candidate {
                stamp: CliffStamp {
                    tile,
                    origin: at,
                    base,
                },
                cells: cells.into_iter().collect(),
                edges: covered,
            });
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
    let mut by_edge = vec![Vec::new(); edges.len()];
    for (c, candidate) in candidates.iter().enumerate() {
        for &e in &candidate.edges {
            by_edge[e].push(c);
        }
    }
    let chosen = solve(&candidates, &by_edge)?;
    let mut cliffs = Vec::new();
    for c in chosen {
        edits.extend(candidates[c].cells.iter().copied());
        cliffs.push(candidates[c].stamp.clone());
    }
    cliffs.extend(crate::coastal_cliffs::gather(doc, scene, &zone)?);
    let mut result = PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs,
        interior: selection,
        base: 0,
    };
    if scene
        .palette
        .tile_ids
        .is_some_and(|i| i.cliffs.cliff_set.is_some())
    {
        crate::coastal_cliffs::adapt(doc, scene, &mut result)?;
    }
    result.edits.retain(|&at, v| *v != value(doc, scene, at));
    protect(doc, scene, &result.edits)?;
    Ok(result)
}
