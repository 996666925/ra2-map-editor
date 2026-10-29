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
            Self::HighConcrete => "高架混凝土桥",
            Self::HighWood => "高架木桥",
            Self::LowWood => "低木桥",
            Self::LowConcrete => "低混凝土桥",
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
        (0..512).contains(&x) && (0..512).contains(&y),
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

pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    a: Cell,
    b: Cell,
    kind: BridgeKind,
) -> Result<PlateauPlan> {
    ensure!(a != b, "拖动确定桥梁跨度");
    let along_x = a[0].abs_diff(b[0]) >= a[1].abs_diff(b[1]);
    let mut b = if along_x { [b[0], a[1]] } else { [a[0], b[1]] };
    let mut a = a;
    if a[usize::from(!along_x)] > b[usize::from(!along_x)] {
        std::mem::swap(&mut a, &mut b);
    }
    doc.check_cell(a)?;
    doc.check_cell(b)?;
    let span = if along_x { b[0] - a[0] } else { b[1] - a[1] };
    ensure!(
        span >= if kind.high() { 8 } else { 3 },
        "桥梁跨度过短（高桥至少 8 格，低桥至少 3 格）"
    );
    let landing = value(doc, scene, a).level;
    ensure!(value(doc, scene, b).level == landing, "两端落地点必须等高");
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
                let id = translated.get(&old.tile).context("落地点地形未知")?;
                let slope = scene
                    .palette
                    .templates
                    .get(&(*id, 0))
                    .and_then(|t| t.cells.get(usize::from(old.sub)))
                    .and_then(|s| *s)
                    .map(|s| s.slope);
                ensure!(
                    old.level == landing
                        && slope == Some(0)
                        && (scene.palette.morphable.contains(id) || i32::from(*id) == ids.clear),
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
        let first = if kind == BridgeKind::LowWood { 74 } else { 205 };
        for i in 0..=span {
            let id = first
                + if along_x {
                    if i == 0 {
                        20
                    } else if i == span {
                        18
                    } else {
                        (i % 4) as u8
                    }
                } else if i == 0 {
                    22
                } else if i == span {
                    24
                } else {
                    9 + (i % 4) as u8
                };
            for side in -1..=1 {
                let at = if along_x {
                    cell(a, i32::from(i), side)?
                } else {
                    cell(a, side, i32::from(i))?
                };
                let frame = (side + 1) as u8;
                overlay(scene, id, frame)?;
                p.overlays.insert(at, (id, frame));
                footprint.insert(at);
            }
        }
        for side in -1..=1 {
            for at in if along_x {
                [cell(a, -1, side)?, cell(b, 1, side)?]
            } else {
                [cell(a, side, -1)?, cell(b, side, 1)?]
            } {
                doc.check_cell(at)?;
                let old = value(doc, scene, at);
                let id = translated.get(&old.tile).context("落地点地形未知")?;
                ensure!(
                    old.level == base
                        && (scene.palette.morphable.contains(id) || i32::from(*id) == ids.clear),
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
        let ordinary = scene.palette.morphable.contains(id)
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
            ensure!(!footprint.contains(&at), "桥梁范围内有对象或建筑占地");
        }
    }
    Ok(p)
}

#[cfg(test)]
#[path = "bridge_tools_tests.rs"]
mod tests;
