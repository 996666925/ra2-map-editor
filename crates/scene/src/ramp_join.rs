//! Recover complete entrances from saved TMP cells and rebuild touching runs.
//! No editor-only topology is needed after saving, reloading or undoing.
use super::*;
use ra2_map_document::document::{Target, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Straight {
        origin: Cell,
        direction: u8,
        extra: i32,
    },
    Corner {
        center: Cell,
        outward: [i32; 2],
        extra: [i32; 2],
    },
}
impl Shape {
    fn geometry(self, doc: &MapDocument, scene: &MapScene, base: u8) -> Result<PlateauPlan> {
        match self {
            Self::Straight {
                origin,
                direction,
                extra,
            } => straight_geometry(doc, scene, origin, base, direction, extra),
            Self::Corner {
                center,
                outward,
                extra,
            } => corner_geometry(doc, scene, center, outward, extra, base),
        }
    }
    fn union(self, other: Self) -> Option<Self> {
        match (self, other) {
            (
                Self::Straight {
                    origin: a,
                    direction,
                    extra: ea,
                },
                Self::Straight {
                    origin: b,
                    direction: db,
                    extra: eb,
                },
            ) => {
                let axis = usize::from(!direction.is_multiple_of(2));
                if direction != db || a[1 - axis] != b[1 - axis] {
                    return None;
                }
                let lo = a[axis].min(b[axis]);
                let hi = (i32::from(a[axis]) + ea).max(i32::from(b[axis]) + eb);
                let mut origin = a;
                origin[axis] = lo;
                Some(Self::Straight {
                    origin,
                    direction,
                    extra: hi - i32::from(lo),
                })
            }
            (
                Self::Corner {
                    center,
                    outward,
                    extra: a,
                },
                Self::Corner {
                    center: cb,
                    outward: ob,
                    extra: b,
                },
            ) => (center == cb && outward == ob).then_some(Self::Corner {
                center,
                outward,
                extra: [a[0].max(b[0]), a[1].max(b[1])],
            }),
            (Self::Straight { .. }, Self::Corner { .. }) => other.union(self),
            (
                Self::Corner {
                    center,
                    outward,
                    mut extra,
                },
                Self::Straight {
                    origin,
                    direction,
                    extra: es,
                },
            ) => {
                let axis = usize::from(!direction.is_multiple_of(2));
                let normal = 1 - axis;
                let expected = if normal == 0 {
                    if outward[0] > 0 { 1 } else { 3 }
                } else if outward[1] > 0 {
                    0
                } else {
                    2
                };
                let line = i32::from(center[normal]) + if outward[normal] < 0 { -3 } else { 0 };
                if direction != expected || i32::from(origin[normal]) != line {
                    return None;
                }
                let extent = if outward[axis] > 0 {
                    i32::from(center[axis]) - 3 - i32::from(origin[axis])
                } else {
                    i32::from(origin[axis]) + 2 + es - i32::from(center[axis])
                };
                extra[axis] = extra[axis].max(extent);
                Some(Self::Corner {
                    center,
                    outward,
                    extra,
                })
            }
        }
    }
}

#[derive(Clone, Copy)]
struct End {
    index: u16,
    origin: Cell,
    base: u8,
}
fn end_at(doc: &MapDocument, scene: &MapScene, at: Cell) -> Option<End> {
    let ids = scene.palette.tile_ids?;
    let first = u16::try_from(ids.slope_set_pieces)
        .ok()
        .or(ids.cliffs.cliff_ramps)?;
    let v = value(doc, scene, at);
    let id = tile(scene, v.tile)?;
    let index = id.checked_sub(first).filter(|&n| n < 10)?;
    let t = scene.palette.templates.get(&(id, 0))?;
    let sub = usize::from(v.sub);
    let s = t.cells.get(sub)?.as_ref()?;
    Some(End {
        index,
        origin: offset(
            at,
            -(sub as i32 % i32::from(t.width)),
            -(sub as i32 / i32::from(t.width)),
        )?,
        base: v.level.checked_sub(s.height)?,
    })
}
fn body_at(doc: &MapDocument, scene: &MapScene, at: Cell) -> Option<(u16, u8)> {
    let ids = scene.palette.tile_ids?;
    if ids.ramp_base < 0 {
        return None;
    }
    let v = value(doc, scene, at);
    let shape = i32::from(tile(scene, v.tile)?) - ids.ramp_base + 1;
    (v.sub == 0 && (1..=8).contains(&shape)).then_some((shape as u16, v.level))
}

pub(super) fn infer_base(doc: &MapDocument, scene: &MapScene, start: Cell) -> Option<u8> {
    if let Some(e) = end_at(doc, scene, start) {
        return Some(e.base);
    }
    body_at(doc, scene, start)?;
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(at) = queue.pop_front() {
        if !seen.insert(at) || doc.check_cell(at).is_err() {
            continue;
        }
        if let Some(e) = end_at(doc, scene, at) {
            return Some(e.base);
        }
        if body_at(doc, scene, at).is_none() {
            continue;
        }
        for (x, y) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if let Some(n) = offset(at, x, y) {
                queue.push_back(n);
            }
        }
    }
    None
}

fn recover(doc: &MapDocument, scene: &MapScene, plan: &PlateauPlan, base: u8) -> Vec<Shape> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    for &at in plan.edits.keys() {
        for x in -1..=1 {
            for y in -1..=1 {
                if let Some(p) = offset(at, x, y) {
                    queue.push_back(p);
                }
            }
        }
    }
    let mut ends = BTreeMap::new();
    let mut centers = Vec::new();
    while let Some(at) = queue.pop_front() {
        if !seen.insert(at) || doc.check_cell(at).is_err() {
            continue;
        }
        if let Some(e) = end_at(doc, scene, at).filter(|e| e.base == base) {
            ends.insert((e.index, e.origin), e);
        } else if let Some((shape, level)) =
            body_at(doc, scene, at).filter(|&(_, h)| (base..base + 4).contains(&h))
        {
            if shape >= 5 && level == base + 3 {
                centers.push((at, shape));
            }
        } else {
            continue;
        }
        for x in -1..=1 {
            for y in -1..=1 {
                if let Some(n) = offset(at, x, y) {
                    queue.push_back(n);
                }
            }
        }
    }
    let mut shapes = Vec::new();
    let mut accept = |shape: Shape| {
        if let Ok(p) = shape.geometry(doc, scene, base)
            && p.edits.iter().all(|(&at, &v)| value(doc, scene, at) == v)
            && !shapes.contains(&shape)
        {
            shapes.push(shape);
        }
    };
    for a in ends.values() {
        let (direction, right, axis) = match a.index {
            0 => (0, 1, 0),
            3 => (1, 2, 1),
            4 => (2, 5, 0),
            7 => (3, 8, 1),
            _ => continue,
        };
        for b in ends
            .values()
            .filter(|b| b.index == right && b.origin[1 - axis] == a.origin[1 - axis])
        {
            let extra = i32::from(b.origin[axis]) - i32::from(a.origin[axis]) - 3;
            if extra >= 0 {
                accept(Shape::Straight {
                    origin: a.origin,
                    direction,
                    extra,
                });
            }
        }
    }
    for (center, slope) in centers {
        let (outward, xend, yend, xoffset, yoffset) = match slope {
            5 => ([-1, -1], 5, 8, [1, -3], [-3, 1]),
            6 => ([1, -1], 4, 2, [-3, -3], [0, 1]),
            7 => ([1, 1], 0, 3, [-3, 0], [0, -3]),
            _ => ([-1, 1], 1, 7, [1, 0], [-3, -3]),
        };
        for x in ends.values().filter(|e| {
            e.index == xend && i32::from(e.origin[1]) == i32::from(center[1]) + xoffset[1]
        }) {
            let ex = (i32::from(x.origin[0]) - i32::from(center[0]) - xoffset[0]) * -outward[0];
            if ex < 0 {
                continue;
            }
            for y in ends.values().filter(|e| {
                e.index == yend && i32::from(e.origin[0]) == i32::from(center[0]) + yoffset[0]
            }) {
                let ey = (i32::from(y.origin[1]) - i32::from(center[1]) - yoffset[1]) * -outward[1];
                if ey >= 0 {
                    accept(Shape::Corner {
                        center,
                        outward,
                        extra: [ex, ey],
                    });
                }
            }
        }
    }
    shapes
}

pub(super) fn finish(
    doc: &MapDocument,
    scene: &MapScene,
    mut shape: Shape,
    base: u8,
    start: Cell,
) -> Result<PlateauPlan> {
    let mut p = shape.geometry(doc, scene, base)?;
    ensure!(
        p.edits.contains_key(&start),
        "请从悬崖边缘或已有坡道入口拖动"
    );
    let old = recover(doc, scene, &p, base);
    let mut included = BTreeSet::new();
    loop {
        let mut changed = false;
        for (i, &other) in old.iter().enumerate() {
            if included.contains(&i) {
                continue;
            }
            let q = other.geometry(doc, scene, base)?;
            let touching = q.edits.keys().any(|at| {
                (-1..=1).any(|x| {
                    (-1..=1).any(|y| offset(*at, x, y).is_some_and(|n| p.edits.contains_key(&n)))
                })
            });
            if !touching {
                continue;
            }
            let Some(union) = shape.union(other) else {
                ensure!(
                    !q.edits.keys().any(|at| p.edits.contains_key(at)),
                    "坡向不兼容，不能切断已有坡道"
                );
                continue;
            };
            let next = union.geometry(doc, scene, base)?;
            ensure!(
                q.edits.keys().all(|at| next.edits.contains_key(at)),
                "坡道方向不兼容，无法完整衔接已有入口"
            );
            shape = union;
            p = next;
            included.insert(i);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    let mut touched = !included.is_empty();
    for (&at, &v) in &p.edits {
        let id = tile(scene, v.tile).context("坡道编号无效")?;
        let s = scene.palette.templates[&(id, 0)].cells[usize::from(v.sub)].unwrap();
        if s.slope == 0 {
            ensure!(value(doc, scene, at).level == v.level, "坡道两侧高度不匹配");
        }
        touched |= cliff(scene, value(doc, scene, at), at).is_some();
    }
    ensure!(touched, "请放置在悬崖边界或已有坡道入口");
    complete_replacement(doc, scene, &p.edits)?;
    // Admit only complete same-height endpoint TMPs for replacement here;
    // generic plateau authoring still protects existing entrances.
    let mut validation = doc.fork();
    for &at in p.edits.keys() {
        if let Some(e) = end_at(doc, scene, at) {
            ensure!(e.base == base, "已有坡道基准高度不匹配");
            let old = value(doc, scene, at);
            if old == p.edits[&at] {
                continue;
            }
            validation.set(
                Target::Terrain(at),
                Some(Value::Terrain(clear(scene, old, old.level)?)),
            )?;
        }
    }
    cliff_tools::validate(&validation, scene, &p.edits, base, true)?;
    Ok(p)
}
