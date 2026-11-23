//! Whole-bridge authoring: heads and deck share one preview and transaction.
use crate::{
    cliff_authoring::{complete_replacement, stamp},
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, TerrainValue};
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::map::{
    bridge_facts::{BridgeStampFamily, high_bridge_stamp_for_overlay},
    cell_index::MAX_CELL_SIDE,
};
use vera20k_core::rules::terrain_rules::{LandType, tmp_terrain_to_land_type};
use vera20k_core::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS;

pub(crate) fn deck_level(base: u8) -> i16 {
    i16::from(base as i8) + BRIDGE_DECK_HEIGHT_LEVELS as i16
}

/// Leave resolved high-bridge overlays in place during ground height edits.
pub(crate) fn height_editable_overlay(doc: &MapDocument, scene: &MapScene, at: Cell) -> bool {
    let id = doc.overlay(at).0;
    id == 255
        || scene
            .authored_bridge_decks
            .get(&at)
            .is_some_and(|&(saved, _)| id == saved)
}

/// Project the relevant authored stamps before overlay edits reach the scene.
/// The engine visits anchors in OverlayPack (Y, X) order. A structural slot
/// enables the deck, while the last pointer write selects its base level.
pub(crate) fn authored_deck_level(
    scene: &MapScene,
    at: Cell,
    level_at: impl Fn(Cell) -> u8,
    overlay_at: impl Fn(Cell) -> u8,
) -> Option<i16> {
    let anchor_deck = |anchor: Cell| {
        let id = overlay_at(anchor);
        // Pending terrain edits do not move an already resolved span.
        scene
            .authored_bridge_decks
            .get(&anchor)
            .filter(|&&(saved, _)| id == saved)
            .map_or_else(|| deck_level(level_at(anchor)), |&(_, level)| level)
    };
    stamped_deck_level(
        at,
        |c| high_bridge_stamp_for_overlay(overlay_at(c)),
        anchor_deck,
    )
}

/// A region transaction can cut, join or replace a span. Resolve from the
/// prospective anchors and bridgeheads, including existing cells outside it.
pub(crate) fn prospective_deck_level(
    scene: &MapScene,
    at: Cell,
    terrain_at: impl Fn(Cell) -> Option<TerrainValue>,
    overlay_at: impl Fn(Cell) -> u8,
) -> Option<i16> {
    let stamp_at = |at| {
        let id = overlay_at(at);
        let stamp = high_bridge_stamp_for_overlay(id)?;
        (scene.high_bridge_overlay_ids.contains(&id) && terrain_at(at).is_some()).then_some(stamp)
    };
    stamped_deck_level(at, stamp_at, |anchor| {
        let stamp = stamp_at(anchor).expect("stamped bridge anchor");
        let axis = usize::from(stamp.1 != 0);
        let run_end = |delta| {
            let mut end = anchor;
            while let Ok(next) = step(end, axis, delta) {
                if stamp_at(next) != Some(stamp) {
                    break;
                }
                end = next;
            }
            end
        };
        let (first, last) = (run_end(-1), run_end(1));
        let first_ground = terrain_at(first).expect("span start terrain");
        let last_ground = terrain_at(last).expect("span end terrain");
        let tile = |v: TerrainValue| {
            scene
                .palette
                .raw_ids
                .get(&v.tile)
                .copied()
                .or_else(|| u16::try_from(v.tile).ok())
        };
        let wood = |tile| {
            scene
                .palette
                .wood_bridge_rims
                .get(&tile)
                .filter(|rim| rim.axis == axis)
        };
        let concrete = scene.palette.bridge_rim.filter(|rim| rim.base >= 0);
        let middle = |relative, base: i32| (0..4).any(|n| relative == base.wrapping_add(n));
        let start_deck = step(first, axis, -1)
            .ok()
            .and_then(&terrain_at)
            .and_then(|road| {
                let start_tile = tile(first_ground)?;
                if tile(road)? != start_tile {
                    return None;
                }
                let sub = first_ground.sub;
                let wooden = wood(start_tile).is_some_and(|rim| {
                    rim.anchor_subtile == Some(sub) && rim.road_subtile == road.sub
                });
                let concrete = concrete.is_some_and(|rim| {
                    let relative = i32::from(start_tile) - rim.base + 1;
                    // The engine tests the X direction first if theater keys alias.
                    let x = (sub == 8 && rim.top_left.contains(&relative))
                        || (sub == 5 && middle(relative, rim.middle[0]));
                    if x {
                        axis == 0
                    } else {
                        axis == 1
                            && ((sub == 12 && rim.top_right.contains(&relative))
                                || (sub == 7 && middle(relative, rim.middle[1])))
                    }
                });
                (wooden || concrete).then_some(i16::from(road.level as i8))
            });
        let end_deck = step(last, axis, 1)
            .ok()
            .and_then(&terrain_at)
            .and_then(|road| {
                let tile = tile(road)?;
                let wooden = wood(tile).is_some_and(|rim| {
                    rim.anchor_subtile.is_none() && rim.road_subtile == road.sub
                });
                let concrete = concrete.is_some_and(|rim| {
                    let relative = i32::from(tile) - rim.base + 1;
                    let (sub, ends) = if axis == 0 {
                        (4, rim.bottom_right)
                    } else {
                        (2, rim.bottom_left)
                    };
                    road.sub == sub
                        && (ends.contains(&relative) || middle(relative, rim.middle[axis]))
                });
                (wooden || concrete).then_some(i16::from(road.level as i8))
            });
        match (start_deck, end_deck) {
            (Some(a), Some(b)) => a.min(b),
            (Some(deck), None) | (None, Some(deck)) => deck,
            (None, None) => deck_level(first_ground.level).min(deck_level(last_ground.level)),
        }
    })
}

fn stamped_deck_level(
    at: Cell,
    stamp_at: impl Fn(Cell) -> Option<(BridgeStampFamily, u8)>,
    anchor_deck: impl Fn(Cell) -> i16,
) -> Option<i16> {
    // ANCHOR_SELF takes precedence over a pointer from a neighbouring stamp.
    if stamp_at(at).is_some() {
        return Some(anchor_deck(at));
    }
    let mut structural = false;
    let mut last_anchor = None;
    // Inverse offsets from the target to the anchor for F1, F2, Opposite
    // and ExtraDir6. F3 and the anchor itself do not write the pointer.
    for (x, y, direction, supports_deck) in [
        (0, 1, 0, true),
        (0, 2, 0, true),
        (0, -1, 0, true),
        (1, 0, 6, true),
        (2, 0, 6, true),
        (-1, 0, 6, true),
        (-2, 0, 6, false),
    ] {
        let Ok(anchor) = cell(at, x, y) else { continue };
        if stamp_at(anchor).is_some_and(|(_, dir)| dir == direction) {
            structural |= supports_deck;
            let packed_order = (anchor[1], anchor[0]);
            last_anchor = Some(last_anchor.map_or(packed_order, |prev| packed_order.max(prev)));
        }
    }
    let (y, x) = last_anchor.filter(|_| structural)?;
    Some(anchor_deck([x, y]))
}

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
fn overlay_name(id: u8) -> String {
    match id {
        24 => "BRIDGE1".into(),
        25 => "BRIDGE2".into(),
        237 => "BRIDGEB1".into(),
        238 => "BRIDGEB2".into(),
        74..=101 => format!("LOBRDG{:02}", id - 73),
        205..=232 => format!("LOBRDB{:02}", id - 204),
        _ => String::new(),
    }
}

pub(crate) fn standard_overlay(scene: &MapScene, id: u8) -> bool {
    let expected = overlay_name(id);
    !expected.is_empty()
        && scene
            .catalog
            .overlays
            .get(usize::from(id))
            .is_some_and(|name| name.eq_ignore_ascii_case(&expected))
}

fn overlay(scene: &MapScene, id: u8, frame: u8) -> Result<()> {
    let name = scene
        .catalog
        .overlays
        .get(usize::from(id))
        .context("当前环境缺少桥面覆盖物")?;
    ensure!(
        standard_overlay(scene, id),
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

fn low_overlay(scene: &MapScene, id: u8) -> bool {
    matches!(id, 74..=101 | 205..=232) && standard_overlay(scene, id)
}

fn high_overlay(scene: &MapScene, id: u8) -> bool {
    matches!(id, 24 | 25 | 237 | 238) && standard_overlay(scene, id)
}

fn clear_crossing(doc: &MapDocument, scene: &MapScene, at: Cell) -> Result<TerrainValue> {
    let id = u16::try_from(scene.palette.tile_ids.context("缺少地形配置")?.clear)?;
    let tile = *scene.catalog.raw_tiles.get(&id).context("原地形无法识别")?;
    let sub = scene
        .palette
        .templates
        .get(&(id, 0))
        .and_then(|t| t.cells.first())
        .and_then(|s| *s)
        .context("原地形几何缺失")?;
    ensure!(sub.height == 0 && sub.slope == 0, "桥梁范围内有坡面");
    Ok(TerrainValue {
        tile,
        sub: 0,
        level: value(doc, scene, at).level,
        ice: 0,
    })
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
    if crate::coastal_cliffs::piece(scene, *id).is_some() || bridge_head(scene, *id) {
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

fn same_head_geometry(scene: &MapScene, a: u16, b: u16) -> bool {
    let (Some(a), Some(b)) = (
        scene.palette.templates.get(&(a, 0)),
        scene.palette.templates.get(&(b, 0)),
    ) else {
        return false;
    };
    a.width == b.width
        && a.height == b.height
        && a.cells.len() == b.cells.len()
        && a.cells.iter().zip(&b.cells).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => a.height == b.height && a.slope == b.slope,
            (None, None) => true,
            _ => false,
        })
}

fn head_tile(scene: &MapScene, family: u16, relative: i32) -> Result<u16> {
    let offset = u16::try_from(relative - 1)?;
    let preferred = family.checked_add(offset).context("桥头编号溢出")?;
    let reference = scene
        .palette
        .tile_ids
        .and_then(|ids| ids.cliffs.bridge_set)
        .and_then(|base| base.checked_add(offset))
        .context("当前环境缺少桥头方向配置")?;
    if same_head_geometry(scene, preferred, reference) {
        return Ok(preferred);
    }
    // NEWURBAN's wooden set orders its heads differently from BridgeSet.
    // Keep the declared piece when compatible; otherwise locate its geometry
    // within this family instead of placing a head facing the wrong bank.
    (0..16)
        .filter_map(|n| family.checked_add(n))
        .find(|&tile| same_head_geometry(scene, tile, reference))
        .context("当前环境缺少桥头方向配置")
}

/// Recover the same bank profile when later cliff edits encounter a saved head.
pub(crate) fn head_profile(scene: &MapScene, tile: u16) -> Option<(usize, i32, bool)> {
    let base = scene.palette.tile_ids?.cliffs.bridge_set?;
    let rim = scene.palette.bridge_rim?;
    for (pair, profile) in [
        (rim.top_left, (0, 0, false)),
        (rim.bottom_right, (0, 1, true)),
        (rim.top_right, (1, 0, false)),
        (rim.bottom_left, (1, 1, true)),
    ] {
        for relative in pair {
            if let Some(offset) = relative.checked_sub(1).and_then(|n| u16::try_from(n).ok())
                && let Some(reference) = base.checked_add(offset)
                && same_head_geometry(scene, tile, reference)
            {
                return Some(profile);
            }
        }
    }
    None
}

fn bridge_head(scene: &MapScene, tile: u16) -> bool {
    scene.palette.tile_ids.is_some_and(|ids| {
        [ids.cliffs.bridge_set, ids.cliffs.wood_bridge_set]
            .into_iter()
            .flatten()
            .any(|base| tile >= base && tile - base < 16)
    }) && head_profile(scene, tile).is_some()
}

fn saved_head_end(
    doc: &MapDocument,
    scene: &MapScene,
    at: Cell,
    axis: usize,
    direction: i32,
) -> Option<Cell> {
    let v = value(doc, scene, at);
    let id = *scene.palette.raw_ids.get(&v.tile)?;
    if !bridge_head(scene, id) {
        return None;
    }
    let (head_axis, _, reverse) = head_profile(scene, id)?;
    if axis != head_axis || reverse != (direction < 0) {
        return None;
    }
    let t = scene.palette.templates.get(&(id, 0))?;
    let origin = cell(
        at,
        -i32::from(u16::from(v.sub) % t.width),
        -i32::from(u16::from(v.sub) / t.width),
    )
    .ok()?;
    if axis == 0 {
        cell(origin, 1, 2).ok()
    } else {
        cell(origin, 2, 1).ok()
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
    if let Some(end) = saved_head_end(doc, scene, at, axis, direction) {
        return Ok(end);
    }
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
        if let Some(end) = saved_head_end(doc, scene, next, axis, direction) {
            return Ok(end);
        }
        if value(doc, scene, next).level != landing {
            // Align the outer cliff shoulders, not the raised centre of the
            // road. Both heads overhang the low ground by one centre cell.
            return if direction > 0 {
                step(at, axis, 1)
            } else {
                Ok(at)
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
            .checked_sub(BRIDGE_DECK_HEIGHT_LEVELS as u8)
            .context("高架桥两端至少应在第 4 层")?
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
    let mut deck = BTreeSet::new();
    let mut under_deck = BTreeSet::new();
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
            let tile = head_tile(scene, family, n)?;
            for (at, v) in stamp(doc, scene, tile, origin, base)? {
                ensure!(p.edits.insert(at, v).is_none(), "两端桥头重叠，请增加跨度");
                footprint.insert(at);
            }
            p.cliffs.push(CliffStamp { tile, origin, base });
        }
        complete_replacement(doc, scene, &mut p)?;
        footprint.extend(p.edits.keys().copied());
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
                    (under.level as i8) <= (base as i8) || side == -2 && under.level == landing,
                    "桥下地形必须与桥基等高或更低"
                );
                under_deck.insert(at);
                footprint.insert(at);
                if !p.edits.contains_key(&at) && (under.level as i8) <= (base as i8) {
                    deck.insert(at);
                }
            }
        }
    } else {
        p.overlays = low_deck(doc, scene, a, b, kind, along_x)?;
        footprint.extend(p.overlays.keys().copied());
        deck.extend(p.overlays.keys().copied());
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
    // Only cells sharing an OverlayPack slot need replacement. The high
    // bridge's wider structural row must keep the low deck and ground intact.
    let crossings: BTreeSet<_> = deck
        .iter()
        .copied()
        .filter(|&at| {
            if kind.high() {
                p.overlays.contains_key(&at) && low_overlay(scene, doc.overlay(at).0)
            } else {
                high_overlay(scene, doc.overlay(at).0)
            }
        })
        .collect();
    for &at in &footprint {
        doc.check_cell(at)?;
        let id = doc.overlay(at).0;
        ensure!(
            id == 255 || standard_overlay(scene, id),
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
        let old_head =
            bridge_head(scene, *id) && p.edits.get(&at).is_some_and(|new| new.level == v.level);
        let tile = i32::from(*id);
        let morphable = scene.palette.morphable.contains(id);
        // A suspended span can cross intact terrain, including a cliff face
        // below the bridge. Bridgeheads and overlay crossings replace terrain.
        let keep_ground = kind.high() && deck.contains(&at) && !crossings.contains(&at);
        let keep_cliff = keep_ground && land_cliff;
        let keep_slope = keep_ground
            && t.slope != 0
            && (morphable
                || ids.ramp_base >= 0 && (ids.ramp_base..ids.ramp_base + 20).contains(&tile)
                || ids.ramp_smooth >= 0 && (ids.ramp_smooth..ids.ramp_smooth + 12).contains(&tile));
        let ordinary = land(scene, v)
            || morphable
            || tile == ids.clear
            || ids.is_water_shore_or_waterfall(tile)
            || keep_slope;
        ensure!(
            ordinary
                || old_head
                || keep_cliff
                || land_cliff && (p.edits.contains_key(&at) || v.level == base),
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
        ensure!(
            t.slope == 0 || old_head || keep_slope || keep_cliff,
            "桥梁范围内有坡面"
        );
        if kind.high() {
            ensure!(
                old_head
                    || v.level == base
                    || v.level == landing
                    || under_deck.contains(&at) && (v.level as i8) < (base as i8),
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
    for at in crossings {
        p.edits.insert(at, clear_crossing(doc, scene, at)?);
        // A new high anchor already replaces the low overlay in this slot.
        if !kind.high() {
            p.overlays.remove(&at);
        }
    }
    Ok(p)
}

#[cfg(test)]
#[path = "bridge_tools_tests.rs"]
mod tests;
