//! Whole-bridge authoring: heads and deck share one preview and transaction.
use crate::{
    cliff_authoring::{complete_replacement, stamp},
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument};
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;
use vera20k_core::rules::terrain_rules::{LandType, tmp_terrain_to_land_type};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BridgeKind {
    #[default]
    HighConcrete,
    HighWood,
    LowWood,
    LowConcrete,
}
impl BridgeKind {
    pub const ALL: [Self; 4] = [
        Self::HighConcrete,
        Self::HighWood,
        Self::LowWood,
        Self::LowConcrete,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::HighConcrete => "悬崖间高架混凝土桥",
            Self::HighWood => "悬崖间高架木桥",
            Self::LowWood => "水面低木桥",
            Self::LowConcrete => "水面低混凝土桥",
        }
    }
    pub fn high(self) -> bool {
        matches!(self, Self::HighConcrete | Self::HighWood)
    }
}
fn cell(a: Cell, x: i32, y: i32) -> Result<Cell> {
    let x = i32::from(a[0]) + x;
    let y = i32::from(a[1]) + y;
    ensure!(
        (0..i32::from(MAX_CELL_SIDE)).contains(&x) && (0..i32::from(MAX_CELL_SIDE)).contains(&y),
        "桥梁越过固定格网"
    );
    Ok([x as u16, y as u16])
}
fn overlay(scene: &MapScene, id: u8, frame: u8) -> Result<()> {
    let name = scene
        .catalog
        .overlays
        .get(usize::from(id))
        .context("当前环境缺少桥面覆盖物")?;
    let expected = match id {
        24 => "BRIDGE1".into(),
        25 => "BRIDGE2".into(),
        237 => "BRIDGEB1".into(),
        238 => "BRIDGEB2".into(),
        74..=101 => format!("LOBRDG{:02}", id - 73),
        205..=232 => format!("LOBRDB{:02}", id - 204),
        _ => String::new(),
    };
    ensure!(
        name.eq_ignore_ascii_case(&expected),
        "覆盖物编号 {id} 已被 Mod 改写，无法安全生成标准桥梁"
    );
    // Some bridge-end cross rows are intentionally empty SHP frames. Their
    // map data is still required even though the renderer has no image.
    ensure!(
        scene
            .sprites
            .get("Overlay", name)
            .and_then(|p| p.image.as_ref())
            .is_some_and(|p| usize::from(frame) < p.info.count),
        "缺少桥面 {name} 的第 {frame} 帧"
    );
    Ok(())
}

fn land(scene: &MapScene, v: ra2_map_document::document::TerrainValue) -> bool {
    let Some(&id) = scene.palette.raw_ids.get(&v.tile) else {
        return false;
    };
    let Some(s) = scene
        .palette
        .templates
        .get(&(id, 0))
        .and_then(|t| t.cells.get(usize::from(v.sub)))
        .and_then(|s| *s)
    else {
        return false;
    };
    // Morphable describes terrain editing, not whether units can enter a bank.
    // Retail snow LATs (Icy, Grey/Clear, etc.) are flat dry ground without that
    // flag. Use their TMP land type; low bridges leave those templates intact.
    s.slope == 0
        && s.terrain < 16
        && matches!(
            tmp_terrain_to_land_type(s.terrain),
            LandType::Clear | LandType::Road | LandType::Rough | LandType::Ice
        )
}

/// A cliff face's picked subcell can be at the foot of the cliff. Recover
/// the bank height from its TMP rather than treating that foot as a landing.
fn bank_level(doc: &MapDocument, scene: &MapScene, at: Cell) -> Result<u8> {
    let v = value(doc, scene, at);
    let id = scene
        .palette
        .raw_ids
        .get(&v.tile)
        .context("落地点地形未知")?;
    if crate::coastal_cliffs::piece(scene, *id).is_some() {
        let sub = scene
            .palette
            .templates
            .get(&(*id, 0))
            .and_then(|t| t.cells.get(usize::from(v.sub)))
            .and_then(|s| *s)
            .context("悬崖子块缺失")?;
        return v
            .level
            .checked_sub(sub.height)
            .and_then(|b| b.checked_add(4))
            .context("悬崖高度无效");
    }
    Ok(v.level)
}

fn step(at: Cell, axis: usize, amount: i32) -> Result<Cell> {
    if axis == 0 {
        cell(at, amount, 0)
    } else {
        cell(at, 0, amount)
    }
}

fn high_end(
    doc: &MapDocument,
    scene: &MapScene,
    mut at: Cell,
    other: Cell,
    axis: usize,
    direction: i32,
    landing: u8,
) -> Result<Cell> {
    // Only the clicked cliff's own footprint may be searched backwards.
    let v = value(doc, scene, at);
    let id = scene
        .palette
        .raw_ids
        .get(&v.tile)
        .context("落地点地形未知")?;
    let reach = if crate::coastal_cliffs::piece(scene, *id).is_some() {
        let t = &scene.palette.templates[&(*id, 0)];
        if axis == 0 { t.width } else { t.height }
    } else {
        0
    };
    for _ in 0..reach {
        if value(doc, scene, at).level == landing {
            break;
        }
        at = step(at, axis, -direction)?;
        doc.check_cell(at)?;
    }
    ensure!(
        value(doc, scene, at).level == landing,
        "请从高地或悬崖边缘拖动高桥"
    );
    loop {
        let next = step(at, axis, direction)?;
        ensure!(
            next != other,
            "两端之间没有可跨越的低地；水面同高桥请选择低桥"
        );
        doc.check_cell(next)?;
        if value(doc, scene, next).level != landing {
            // The positive-side head's first raised centre cell is one
            // cell before its anchor (BottomRight/BottomLeft TMP profile).
            return if direction > 0 {
                Ok(at)
            } else {
                step(at, axis, 1)
            };
        }
        at = next;
    }
}

fn low_end(
    doc: &MapDocument,
    scene: &MapScene,
    mut at: Cell,
    axis: usize,
    outward: i32,
    level: u8,
) -> Result<Cell> {
    // A shore is at most a few cells thick. Never search through a whole
    // water body when an endpoint was accidentally placed in open water.
    for _ in 0..=4 {
        let behind = step(at, axis, outward)?;
        let mut dry = true;
        for side in -1..=1 {
            let p = step(behind, 1 - axis, side)?;
            doc.check_cell(p)?;
            let v = value(doc, scene, p);
            dry &= v.level == level && land(scene, v);
        }
        if dry {
            return Ok(at);
        }
        at = behind;
    }
    anyhow::bail!("低桥端点需要靠近同高岸边，请从一岸拖到另一岸")
}

fn endpoints(mut a: Cell, mut b: Cell) -> (Cell, Cell, bool) {
    let along_x = a[0].abs_diff(b[0]) >= a[1].abs_diff(b[1]);
    b = if along_x { [b[0], a[1]] } else { [a[0], b[1]] };
    if a[usize::from(!along_x)] > b[usize::from(!along_x)] {
        std::mem::swap(&mut a, &mut b);
    }
    (a, b, along_x)
}

fn low_deck(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    kind: BridgeKind,
    along_x: bool,
) -> Result<BTreeMap<Cell, (u8, u8)>> {
    let axis = usize::from(!along_x);
    let first = if kind == BridgeKind::LowWood { 74 } else { 205 };
    let mut overlays = BTreeMap::new();
    for coordinate in a[axis]..=b[axis] {
        let piece = if coordinate == a[axis] {
            if along_x { 20 } else { 22 }
        } else if coordinate == b[axis] {
            if along_x { 18 } else { 24 }
        } else {
            (if along_x { 0 } else { 9 }) + (coordinate % 4) as u8
        };
        for side in -1..=1 {
            let anchor = if along_x {
                [coordinate, a[1]]
            } else {
                [a[0], coordinate]
            };
            let at = step(anchor, 1 - axis, side)?;
            doc.check_cell(at)?;
            let frame = (side + 1) as u8;
            overlay(scene, first + piece, frame)?;
            overlays.insert(at, (first + piece, frame));
        }
    }
    Ok(overlays)
}

/// Visual draft at the dragged endpoints, including an unfinished crossing.
/// This does not validate landings, heights or occupancy; placement must use
/// `plan`. Keep the same deck rows and frames as the validated placement.
pub fn preview_low(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    kind: BridgeKind,
) -> Result<PlateauPlan> {
    ensure!(!kind.high(), "此预览仅用于水面低桥");
    ensure!(a != b, "拖动确定桥梁跨度");
    let (a, b, along_x) = endpoints(a, b);
    doc.check_cell(a)?;
    doc.check_cell(b)?;
    Ok(PlateauPlan {
        edits: BTreeMap::new(),
        overlays: low_deck(doc, scene, a, b, kind, along_x)?,
        cliffs: Vec::new(),
        interior: BTreeSet::new(),
        base: value(doc, scene, a).level,
    })
}

pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    kind: BridgeKind,
) -> Result<PlateauPlan> {
    ensure!(a != b, "拖动确定桥梁跨度");
    let (mut a, mut b, along_x) = endpoints(a, b);
    doc.check_cell(a)?;
    doc.check_cell(b)?;
    let axis = usize::from(!along_x);
    let landing = if kind.high() {
        bank_level(doc, scene, a)?
    } else {
        value(doc, scene, a).level
    };
    let end_level = if kind.high() {
        bank_level(doc, scene, b)?
    } else {
        value(doc, scene, b).level
    };
    ensure!(end_level == landing, "两端落地点必须等高");
    if kind.high() {
        let start = high_end(doc, scene, a, b, axis, 1, landing)?;
        b = high_end(doc, scene, b, a, axis, -1, landing)?;
        a = start;
    } else {
        a = low_end(doc, scene, a, axis, -1, landing)?;
        b = low_end(doc, scene, b, axis, 1, landing)?;
    }
    ensure!(a[axis] < b[axis], "桥梁两端没有有效跨度");
    let span = b[axis] - a[axis];
    ensure!(
        span >= if kind.high() { 3 } else { 2 },
        "桥梁跨度过短，无法容纳两端桥头与桥面"
    );
    let base = if kind.high() {
        landing
            .checked_sub(4)
            .context("高架桥两端至少应在第 4 层；桥下低 4 层")?
    } else {
        landing
    };
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let translated: BTreeMap<_, _> = scene
        .catalog
        .raw_tiles
        .iter()
        .map(|(&id, &raw)| (raw, id))
        .collect();
    let mut p = PlateauPlan {
        edits: BTreeMap::new(),
        overlays: BTreeMap::new(),
        cliffs: Vec::new(),
        interior: BTreeSet::new(),
        base,
    };
    let mut footprint = BTreeSet::new();
    if kind.high() {
        let family = if kind == BridgeKind::HighWood {
            ids.cliffs.wood_bridge_set
        } else {
            ids.cliffs.bridge_set
        }
        .context("当前环境没有该类型的桥头模板")?;
        let rim = scene.palette.bridge_rim.context("缺少桥头方向配置")?;
        let (first, last) = if along_x {
            (rim.top_left[0], rim.bottom_right[0])
        } else {
            (rim.top_right[0], rim.bottom_left[0])
        };
        ensure!(first > 0 && last > 0, "当前环境缺少桥头方向配置");
        let o1 = if along_x {
            cell(a, -1, -2)?
        } else {
            cell(a, -2, -1)?
        };
        let o2 = if along_x {
            cell(b, -1, -2)?
        } else {
            cell(b, -2, -1)?
        };
        for (n, origin) in [(first, o1), (last, o2)] {
            let tile = family
                .checked_add(u16::try_from(n - 1)?)
                .context("桥头编号溢出")?;
            for (at, v) in stamp(doc, scene, tile, origin, base)? {
                ensure!(p.edits.insert(at, v).is_none(), "两端桥头重叠，请增加跨度");
                footprint.insert(at);
            }
            p.cliffs.push(CliffStamp { tile, origin, base });
        }
        complete_replacement(doc, scene, &p.edits)?;
        // Landing strips outside both complete heads remain untouched and must
        // meet their full width, rather than checking just the clicked cells.
        for side in -2..=2 {
            for at in if along_x {
                [cell(a, -2, side)?, cell(b, 1, side)?]
            } else {
                [cell(a, side, -2)?, cell(b, side, 1)?]
            } {
                doc.check_cell(at)?;
                let old = value(doc, scene, at);
                ensure!(
                    old.level == landing && land(scene, old),
                    "桥头后方需要同高的平坦陆地（宽 5 格）"
                );
                footprint.insert(at);
            }
        }
        let (id, frame) = match (kind, along_x) {
            // Setter direction is transverse to the span: BRIDGE1 marks
            // N/S cross rows for an X span, BRIDGE2 W/E for a Y span.
            (BridgeKind::HighConcrete, true) => (24, 0),
            (BridgeKind::HighConcrete, false) => (25, 9),
            (BridgeKind::HighWood, true) => (237, 0),
            (BridgeKind::HighWood, false) => (238, 9),
            _ => unreachable!(),
        };
        overlay(scene, id, frame)?;
        for i in 1..span - 1 {
            let anchor = if along_x {
                cell(a, i32::from(i), 0)?
            } else {
                cell(a, 0, i32::from(i))?
            };
            p.overlays.insert(anchor, (id, frame));
            // The engine's bridge setter stamps anchor, two forward cells
            // and the opposite neighbour. Check the complete structural row.
            for side in -2..=1 {
                let at = if along_x {
                    cell(anchor, 0, side)?
                } else {
                    cell(anchor, side, 0)?
                };
                doc.check_cell(at)?;
                let under = p
                    .edits
                    .get(&at)
                    .copied()
                    .unwrap_or_else(|| value(doc, scene, at));
                ensure!(
                    under.level == base || side == -2 && under.level == landing,
                    "桥下地面必须比两端低 4 层，且桥面下不能有高度突变"
                );
                footprint.insert(at);
            }
        }
    } else {
        p.overlays = low_deck(doc, scene, a, b, kind, along_x)?;
        footprint.extend(p.overlays.keys().copied());
        for side in -1..=1 {
            for at in if along_x {
                [cell(a, -1, side)?, cell(b, 1, side)?]
            } else {
                [cell(a, side, -1)?, cell(b, side, 1)?]
            } {
                doc.check_cell(at)?;
                let old = value(doc, scene, at);
                ensure!(
                    old.level == base && land(scene, old),
                    "低桥两端需要同高陆地"
                );
                footprint.insert(at);
            }
        }
    }
    for &at in &footprint {
        doc.check_cell(at)?;
        ensure!(
            doc.overlay(at).0 == 255,
            "{},{} 已有覆盖物，未放置桥梁",
            at[0],
            at[1]
        );
        let v = value(doc, scene, at);
        let id = translated.get(&v.tile).context("原地形无法识别")?;
        let t = scene
            .palette
            .templates
            .get(&(*id, 0))
            .and_then(|t| t.cells.get(usize::from(v.sub)))
            .and_then(|s| *s)
            .context("原地形几何缺失")?;
        let land_cliff = crate::coastal_cliffs::piece(scene, *id).is_some();
        let ordinary = land(scene, v)
            || scene.palette.morphable.contains(id)
            || i32::from(*id) == ids.clear
            || ids.is_water_shore_or_waterfall(i32::from(*id));
        ensure!(
            ordinary || land_cliff && (p.edits.contains_key(&at) || v.level == base),
            "桥梁范围 {},{} 有不能替换的特殊地形 #{}（{}）",
            at[0],
            at[1],
            id,
            scene
                .palette
                .groups
                .get(id)
                .map_or("未知", |g| g.name.as_str())
        );
        ensure!(t.slope == 0, "桥梁范围内有坡面");
        if kind.high() {
            ensure!(
                v.level == base || v.level == landing,
                "桥梁范围内高度不匹配"
            );
        } else {
            ensure!(v.level == base, "低桥范围必须等高");
        }
    }
    for r in doc
        .records()
        .values()
        .filter(|r| !matches!(r.section.as_str(), "Waypoints" | "Smudge"))
    {
        for at in crate::placement::footprint(scene, r)? {
            ensure!(
                !footprint.contains(&at),
                "桥梁范围 {},{} 被 {}（{}）占用，请移开对象或调整桥位",
                at[0],
                at[1],
                scene.catalog.object_name(&r.type_id()),
                r.type_id()
            );
        }
    }
    Ok(p)
}

#[cfg(test)]
#[path = "bridge_tools_tests.rs"]
mod tests;
