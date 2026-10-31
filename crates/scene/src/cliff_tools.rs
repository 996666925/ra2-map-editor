//! Local, atomic plateau authoring using the theater's standard CliffSet.
//! The orientation decision follows the game's higher-neighbor selector
//! (579620/579B70, documented by core's cliff_materialization owner). Unlike
//! generation, authoring requires complete templates and validates all writes.
use crate::{map::MapScene, terrain_tools::value};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, Target, TerrainValue, Value, cell_line};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;
use vera20k_core::map::theater::TileKey;

#[derive(Clone, Debug)]
pub struct CliffStamp {
    pub tile: u16,
    pub origin: Cell,
    pub base: u8,
}
pub struct PlateauPlan {
    pub edits: BTreeMap<Cell, TerrainValue>,
    pub overlays: BTreeMap<Cell, (u8, u8)>,
    pub cliffs: Vec<CliffStamp>,
    pub interior: BTreeSet<Cell>,
    pub base: u8,
}
impl PlateauPlan {
    pub fn apply(&self, doc: &mut MapDocument) -> Result<Vec<Cell>> {
        for &at in self.edits.keys().chain(self.overlays.keys()) {
            doc.check_cell(at)?;
        }
        let mut changed = Vec::new();
        for (&at, &v) in &self.edits {
            if doc.terrain(at) != Some(v) {
                doc.set(Target::Terrain(at), Some(Value::Terrain(v)))?;
                changed.push(at);
            }
        }
        for (&at, &(id, frame)) in &self.overlays {
            if doc.overlay(at) != (id, frame) {
                doc.set(Target::Overlay(at), Some(Value::Overlay(id, frame)))?;
                changed.push(at);
            }
        }
        changed.sort_unstable();
        changed.dedup();
        Ok(changed)
    }
}

pub fn rectangle(a: Cell, b: Cell) -> BTreeSet<Cell> {
    (a[0].min(b[0])..=a[0].max(b[0]))
        .flat_map(|x| (a[1].min(b[1])..=a[1].max(b[1])).map(move |y| [x, y]))
        .collect()
}

/// Close the sampled outline, then flood the exterior with four neighbors.
/// Everything enclosed, including the outline itself, becomes high ground.
pub fn enclose(path: &[Cell]) -> Result<BTreeSet<Cell>> {
    ensure!(
        path.len() >= 3,
        "拖出高台轮廓；松开时自动连接起点并填充内部"
    );
    let mut wall = BTreeSet::new();
    for (a, b) in path
        .iter()
        .zip(path.iter().cycle().skip(1))
        .take(path.len())
    {
        wall.extend(cell_line(*a, *b));
    }
    let min = [
        wall.iter().map(|p| p[0]).min().unwrap(),
        wall.iter().map(|p| p[1]).min().unwrap(),
    ];
    let max = [
        wall.iter().map(|p| p[0]).max().unwrap(),
        wall.iter().map(|p| p[1]).max().unwrap(),
    ];
    ensure!(
        max[0] > min[0] && max[1] > min[1],
        "高台轮廓需要围出至少 2×2 格的区域"
    );
    let lo = [i32::from(min[0]) - 1, i32::from(min[1]) - 1];
    let hi = [i32::from(max[0]) + 1, i32::from(max[1]) + 1];
    let mut outside = BTreeSet::new();
    let mut queue = VecDeque::from([lo]);
    while let Some(p) = queue.pop_front() {
        if p[0] < lo[0]
            || p[1] < lo[1]
            || p[0] > hi[0]
            || p[1] > hi[1]
            || wall.contains(&[p[0] as u16, p[1] as u16])
            || !outside.insert(p)
        {
            continue;
        }
        for d in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            queue.push_back([p[0] + d[0], p[1] + d[1]]);
        }
    }
    let enclosed: BTreeSet<_> = rectangle(min, max)
        .into_iter()
        .filter(|p| !outside.contains(&p.map(i32::from)))
        .collect();
    ensure!(
        enclosed.iter().any(|p| !wall.contains(p)),
        "轮廓没有围出内部区域，请扩大轮廓"
    );
    Ok(enclosed)
}

const DIRECTIONS: [(i32, i32, u8); 8] = [
    (0, -1, 0x80),
    (1, -1, 1),
    (1, 0, 2),
    (1, 1, 4),
    (0, 1, 8),
    (-1, 1, 0x10),
    (-1, 0, 0x20),
    (-1, -1, 0x40),
];
type Point = (i32, i32);
fn mask(levels: &HashMap<Point, u8>, stamped: &BTreeMap<Cell, TerrainValue>, p: Point) -> u8 {
    let target = levels.get(&p).copied().unwrap_or(0).saturating_add(4);
    DIRECTIONS
        .iter()
        .filter(|&&(dx, dy, _)| {
            let n = (p.0 + dx, p.1 + dy);
            levels.get(&n) == Some(&target) && !stamped.contains_key(&[n.0 as u16, n.1 as u16])
        })
        .fold(0, |m, &(_, _, bit)| m | bit)
}

/// Stable representatives of the game's equivalent piece triples. 1-based.
fn piece(m: u8, south: u8, east: u8, southeast: u8) -> Option<u16> {
    Some(if m & 0xA0 == 0xA0 {
        // Keep the complete corner here. Generation's narrow piece 34 can
        // share a low cell with both following faces; a document cell cannot
        // retain two templates. Piece 9 closes that corner without overlap.
        9
    } else if m & 0x82 == 0x82 {
        39
    } else if m & 0x0A == 0x0A {
        33
    } else if m & 0x28 == 0x28 {
        40
    } else if m & 2 != 0 {
        if m & 4 != 0 && m & 0x18 == 0 && south & 0x0A != 0x0A {
            35
        } else if m & 0x0C == 0 && southeast & 2 == 0 {
            1
        } else {
            38
        }
    } else if m & 0x20 != 0 {
        if m & 0x10 != 0 && m & 0x0C == 0 && south & 0x28 != 0x28 {
            15
        } else {
            18
        }
    } else if m & 8 != 0 {
        if m & 4 != 0 && m & 3 == 0 && east & 0x0A != 0x0A {
            23
        } else if m & 6 == 0 && southeast & 8 == 0 {
            21
        } else {
            26
        }
    } else if m & 0x80 != 0 {
        if m & 1 != 0 && m & 6 == 0 && east & 0x82 != 0x82 {
            5
        } else {
            8
        }
    } else if m & 1 != 0 {
        2
    } else if m & 4 != 0 {
        29
    } else if m & 0x10 != 0 {
        22
    } else if m & 0x40 != 0 {
        12
    } else {
        return None;
    })
}

pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    interior: BTreeSet<Cell>,
    base: u8,
) -> Result<PlateauPlan> {
    let mut result = geometry(doc, scene, interior, base)?;
    crate::coastal_cliffs::adapt(doc, scene, &mut result)?;
    validate(doc, scene, &result.edits, base, false)?;
    Ok(result)
}

/// Pure topology compiler, shared by new plateaus and existing-terrain edits.
pub(crate) fn geometry(
    doc: &MapDocument,
    scene: &MapScene,
    interior: BTreeSet<Cell>,
    base: u8,
) -> Result<PlateauPlan> {
    ensure!(base <= 10, "悬崖高差为 4 层，基准高度必须在 0～10 层");
    ensure!(!interior.is_empty(), "请先绘制高台区域");
    let ids = scene.palette.tile_ids.context("当前环境没有地形配置")?;
    let cliff_base = ids
        .cliffs
        .cliff_set
        .context("当前环境没有标准陆地悬崖 CliffSet，无法自动围合")?;
    let min = [
        interior.iter().map(|p| p[0]).min().unwrap(),
        interior.iter().map(|p| p[1]).min().unwrap(),
    ];
    let max = [
        interior.iter().map(|p| p[0]).max().unwrap(),
        interior.iter().map(|p| p[1]).max().unwrap(),
    ];
    ensure!(max[0] > min[0] && max[1] > min[1], "高台区域至少为 2×2 格");
    let lo = (i32::from(min[0]) - 3, i32::from(min[1]) - 3);
    let hi = (i32::from(max[0]) + 3, i32::from(max[1]) + 3);
    ensure!(
        lo.0 >= 0
            && lo.1 >= 0
            && hi.0 < i32::from(MAX_CELL_SIDE)
            && hi.1 < i32::from(MAX_CELL_SIDE),
        "高台及悬崖边界超出固定格网，请留出 3 格空间"
    );
    let mut levels = HashMap::new();
    for x in lo.0..=hi.0 {
        for y in lo.1..=hi.1 {
            levels.insert(
                (x, y),
                base + if interior.contains(&[x as u16, y as u16]) {
                    4
                } else {
                    0
                },
            );
        }
    }
    let mut coords: Vec<_> = levels.keys().copied().collect();
    coords.sort_unstable_by_key(|&(x, y)| (x + y, x));
    let mut stamped = BTreeMap::new();
    let mut cliffs = Vec::new();
    for (x, y) in coords {
        if stamped.contains_key(&[x as u16, y as u16]) {
            continue;
        }
        let m = mask(&levels, &stamped, (x, y));
        let Some(selected) = piece(
            m,
            mask(&levels, &stamped, (x, y + 1)),
            mask(&levels, &stamped, (x + 1, y)),
            mask(&levels, &stamped, (x + 1, y + 1)),
        ) else {
            continue;
        };
        // Use the narrow member of each straight family. Larger 2-cell
        // variants can consume the following concave corner before its turn.
        let mut selected = match selected {
            5 => 8,
            15 => 18,
            23 => 26,
            35 => 38,
            n => n,
        };
        // On staircase edges the preceding face may already own a corner's
        // low cell. The stock single-cell corner completes that join without
        // replacing any part of the preceding complete template.
        if selected == 9
            && let Some(t) = scene.palette.templates.get(&(cliff_base + 8, 0))
            && t.cells.iter().enumerate().any(|(sub, s)| {
                s.is_some()
                    && stamped.contains_key(&[
                        (x + sub as i32 % i32::from(t.width)) as u16,
                        (y + sub as i32 / i32::from(t.width)) as u16,
                    ])
            })
        {
            selected = 34;
        }
        let tile = cliff_base
            .checked_add(selected - 1)
            .context("悬崖编号溢出")?;
        let t = scene
            .palette
            .templates
            .get(&(tile, 0))
            .with_context(|| format!("缺少悬崖模板 #{}（第 {} 块）", tile, selected))?;
        let raw = scene
            .catalog
            .raw_tiles
            .get(&tile)
            .copied()
            .context("悬崖模板无法映射到地图编号")?;
        let anchor = levels[&(x, y)];
        let mut cells = Vec::new();
        for (sub, metadata) in t.cells.iter().enumerate() {
            let Some(metadata) = metadata else {
                continue;
            };
            ensure!(
                metadata.height == 0 || metadata.height == 4,
                "当前悬崖模板不是标准 4 层高差，无法自动围合"
            );
            let at = [
                (x + sub as i32 % i32::from(t.width)) as u16,
                (y + sub as i32 / i32::from(t.width)) as u16,
            ];
            doc.check_cell(at)
                .with_context(|| format!("悬崖延伸到地图边界 {},{}，请向内移动", at[0], at[1]))?;
            ensure!(
                !stamped.contains_key(&at),
                "轮廓过窄或拐角过密，悬崖模板在 {},{} 重叠；请加宽转角",
                at[0],
                at[1]
            );
            ensure!(
                scene.palette.images.contains_key(&TileKey {
                    tile_id: tile,
                    sub_tile: sub as u8,
                    variant: 0
                }),
                "悬崖子块资源缺失"
            );
            let level = anchor
                .checked_add(metadata.height)
                .filter(|v| *v <= 14)
                .context("生成悬崖高度超过 14 层")?;
            cells.push((
                at,
                TerrainValue {
                    tile: raw,
                    sub: sub as u8,
                    level,
                    ..value(doc, scene, at)
                },
            ));
        }
        ensure!(!cells.is_empty(), "悬崖模板没有有效子块");
        for (at, v) in cells {
            levels.insert((i32::from(at[0]), i32::from(at[1])), v.level);
            stamped.insert(at, v);
        }
        cliffs.push(CliffStamp {
            tile,
            origin: [x as u16, y as u16],
            base: anchor,
        });
    }
    ensure!(!cliffs.is_empty(), "未能生成悬崖边界");
    for &at in &interior {
        ensure!(
            levels.get(&(i32::from(at[0]), i32::from(at[1]))) == Some(&(base + 4)),
            "轮廓过窄，无法保留连续高台内部；请扩大区域"
        );
    }
    // Every height discontinuity must be covered by the complete cliff stamps.
    // This rejects an open contour rather than leaving invisible height walls.
    for (&(x, y), &level) in &levels {
        for (dx, dy) in [(1, 0), (0, 1)] {
            let next = (x + dx, y + dy);
            if let Some(&other) = levels.get(&next)
                && other != level
            {
                ensure!(
                    level.abs_diff(other) == 4
                        && (stamped.contains_key(&[x as u16, y as u16])
                            || stamped.contains_key(&[next.0 as u16, next.1 as u16])),
                    "轮廓无法闭合，请加宽狭窄区域或转角"
                );
            }
        }
    }
    let mut edits = stamped;
    for (&(x, y), &level) in &levels {
        if level == base + 4 {
            let at = [x as u16, y as u16];
            edits.entry(at).or_insert_with(|| TerrainValue {
                level,
                ..value(doc, scene, at)
            });
        }
    }
    Ok(PlateauPlan {
        edits,
        overlays: BTreeMap::new(),
        cliffs,
        interior,
        base,
    })
}

pub(crate) fn validate(
    doc: &MapDocument,
    scene: &MapScene,
    edits: &BTreeMap<Cell, TerrainValue>,
    base: u8,
    existing: bool,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("当前环境没有地形配置")?;
    let translated: HashMap<_, _> = scene
        .catalog
        .raw_tiles
        .iter()
        .map(|(&tile, &raw)| (raw, tile))
        .collect();
    for (&at, v) in edits {
        doc.check_cell(at)?;
        let old = value(doc, scene, at);
        if existing && old == *v {
            continue;
        }
        let tile = translated.get(&old.tile).copied();
        let geometry = tile
            .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
            .and_then(|t| t.cells.get(usize::from(old.sub)))
            .and_then(|s| *s);
        let morphable = tile.is_some_and(|t| {
            scene.palette.morphable.contains(&t)
                || i32::from(t) == ids.clear
                || ids.ramp_base >= 0 && (ids.ramp_base..ids.ramp_base + 20).contains(&i32::from(t))
                || ids.ramp_smooth >= 0
                    && (ids.ramp_smooth..ids.ramp_smooth + 12).contains(&i32::from(t))
        });
        let cliff =
            existing && tile.is_some_and(|t| crate::coastal_cliffs::piece(scene, t).is_some());
        let water = crate::shore_tools::wet(scene, old)
            && old.level == base
            && crate::shore_tools::id(scene, *v)
                .is_some_and(|t| crate::coastal_cliffs::piece(scene, t).is_some());
        ensure!(
            (if existing {
                (base..=base + 4).contains(&old.level)
            } else {
                old.level == base
            }) && (morphable && geometry.is_some_and(|s| existing || s.slope == 0)
                || cliff
                || water),
            "{},{} 不是同一基准高度的平坦可塑地面；请避开已有悬崖、道路、水面或坡道",
            at[0],
            at[1]
        );
        ensure!(
            doc.overlay(at).0 == 255,
            "{},{} 有覆盖物占用，未修改高台",
            at[0],
            at[1]
        );
        ensure!(v.level <= 14, "高台高度超过 14 层");
    }
    for record in doc
        .records()
        .values()
        .filter(|r| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in crate::placement::footprint(scene, record)? {
            ensure!(
                !edits.contains_key(&at) || existing && edits[&at] == value(doc, scene, at),
                "{},{} 有对象或建筑占地，未修改高台",
                at[0],
                at[1]
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cliff_tools_tests.rs"]
mod tests;
