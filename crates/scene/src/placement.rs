//! Shared placement geometry for preview, paste, move and property editing.
use crate::map::MapScene;
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument, Record, TerrainValue};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

pub fn offsets(scene: &MapScene, section: &str, type_id: &str) -> Vec<(i16, i16)> {
    if section != "Structures" {
        return vec![(0, 0)];
    }
    let image = scene
        .catalog
        .rules
        .section_or_empty(type_id)
        .read_name("Image", 256)
        .unwrap_or(type_id);
    vera20k_core::rules::foundation::Foundation::Stock(
        scene
            .catalog
            .art
            .resolve_metadata_entry(type_id, image)
            .and_then(|art| art.foundation)
            .unwrap_or(0),
    )
    .cell_offsets()
}
pub fn footprint(scene: &MapScene, r: &Record) -> Result<Vec<Cell>> {
    let at = r.cell().context("记录坐标无效")?;
    offsets(scene, &r.section, &r.type_id())
        .into_iter()
        .map(|(dx, dy)| {
            let x = i32::from(at[0]) + i32::from(dx);
            let y = i32::from(at[1]) + i32::from(dy);
            ensure!(
                (0..i32::from(MAX_CELL_SIDE)).contains(&x)
                    && (0..i32::from(MAX_CELL_SIDE)).contains(&y),
                "对象占地超出固定网格"
            );
            Ok([x as u16, y as u16])
        })
        .collect()
}
#[derive(Clone)]
struct Occupant {
    section: String,
    sub: u8,
    high: bool,
}
fn occupant(r: &Record) -> Option<Occupant> {
    if matches!(r.section.as_str(), "Smudge" | "Waypoints") {
        return None;
    }
    let f = r.fields();
    let high = match r.section.as_str() {
        "Infantry" => f.get(11),
        "Units" => f.get(10),
        _ => None,
    }
    .is_some_and(|s| s.parse::<i32>().is_ok_and(|n| n != 0));
    Some(Occupant {
        section: r.section.clone(),
        sub: f.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        high,
    })
}
fn conflicts(a: &Occupant, b: &Occupant) -> bool {
    if a.high != b.high {
        return false;
    }
    !(a.section == "Infantry" && b.section == "Infantry" && a.sub != b.sub)
}
pub fn validate(
    doc: &MapDocument,
    scene: &MapScene,
    proposed: &[Record],
    ignored: &BTreeSet<u64>,
    allow_overlap: bool,
    terrain: &BTreeMap<Cell, TerrainValue>,
) -> Result<()> {
    let current = doc.records();
    let mut trees: BTreeSet<_> = current
        .iter()
        .filter(|(id, r)| !ignored.contains(id) && r.section == "Terrain")
        .filter_map(|(_, r)| r.cell())
        .collect();
    let mut occupied: HashMap<Cell, Vec<Occupant>> = HashMap::new();
    if !allow_overlap {
        for (id, r) in current {
            if ignored.contains(&id) {
                continue;
            }
            if let Some(o) = occupant(&r)
                && let Ok(cells) = footprint(scene, &r)
            {
                for at in cells {
                    occupied.entry(at).or_default().push(o.clone());
                }
            }
        }
    }
    for r in proposed {
        if r.section == "Terrain" {
            ensure!(
                trees.insert(r.cell().context("对象坐标无效")?),
                "每格只能保存一条地形对象记录"
            );
        }
        let cells = footprint(scene, r)?;
        let mut base = None;
        for at in cells {
            doc.check_cell(at)?;
            if r.section == "Structures" {
                let v = terrain
                    .get(&at)
                    .copied()
                    .unwrap_or_else(|| crate::terrain_tools::value(doc, scene, at));
                if let Some(level) = base {
                    ensure!(level == v.level, "建筑占地高度不一致");
                } else {
                    base = Some(v.level);
                }
            }
            if let Some(o) = occupant(r) {
                if !allow_overlap {
                    ensure!(
                        !occupied
                            .get(&at)
                            .is_some_and(|list| list.iter().any(|old| conflicts(&o, old))),
                        "{}, {} 已有对象占用；可显式开启允许重叠",
                        at[0],
                        at[1]
                    );
                }
                occupied.entry(at).or_default().push(o);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "placement_tests.rs"]
mod tests;
