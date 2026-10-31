//! Layer-aware region snapshots and atomic transforms, independent of rendering.
use crate::document::{Cell, MapDocument, Record, Target, TerrainValue, Value};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RegionLayers {
    pub terrain: bool,
    pub overlays: bool,
    pub objects: [bool; 6], // structures, units, infantry, aircraft, trees, smudges
    pub waypoints: bool,
    #[serde(default)]
    pub cell_tags: bool,
}
pub const OBJECT_SECTIONS: [&str; 6] = [
    "Structures",
    "Units",
    "Infantry",
    "Aircraft",
    "Terrain",
    "Smudge",
];
impl Default for RegionLayers {
    fn default() -> Self {
        Self {
            terrain: true,
            overlays: true,
            objects: [true; 6],
            waypoints: false,
            cell_tags: true,
        }
    }
}
impl RegionLayers {
    pub fn with_cell_tags_visible(mut self, visible: bool) -> Self {
        self.cell_tags &= visible;
        self
    }

    pub fn includes(self, section: &str) -> bool {
        if section == "Waypoints" {
            self.waypoints
        } else {
            OBJECT_SECTIONS
                .iter()
                .position(|&s| s == section)
                .is_some_and(|i| self.objects[i])
        }
    }
    pub fn visible(mut self, visibility: [bool; 3]) -> Self {
        self.overlays &= visibility[0];
        for object in &mut self.objects {
            *object &= visibility[1];
        }
        self.waypoints &= visibility[2];
        self
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RegionCell {
    pub at: Cell,
    pub terrain: Option<TerrainValue>,
    pub overlay: Option<(u8, u8)>,
    #[serde(default)]
    pub cell_tag: Option<Vec<u8>>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Clipboard {
    #[serde(default)]
    pub resource_profile: Option<String>,
    pub origin: Cell,
    pub cells: Vec<RegionCell>,
    pub records: Vec<(u64, Record)>,
    pub layers: RegionLayers,
    pub theater: String,
}
pub struct PastePlan {
    pub cell_tags: bool,
    pub cells: Vec<RegionCell>,
    pub records: Vec<(u64, Record)>,
}
impl Clipboard {
    pub fn capture(
        doc: &MapDocument,
        cells: &BTreeSet<Cell>,
        layers: RegionLayers,
    ) -> Result<Self> {
        ensure!(!cells.is_empty(), "请先框选区域");
        for &at in cells {
            doc.check_cell(at)?;
        }
        let origin = [
            cells.iter().map(|c| c[0]).min().unwrap(),
            cells.iter().map(|c| c[1]).min().unwrap(),
        ];
        let records = doc
            .records()
            .into_iter()
            .filter(|(_, r)| {
                layers.includes(&r.section) && r.cell().is_some_and(|at| cells.contains(&at))
            })
            .collect();
        let mut tags = BTreeMap::new();
        if layers.cell_tags {
            for (_, key, value) in doc.ini_entries("CellTags") {
                if let Some(at) = crate::mission::cell_at(doc, &key)
                    && cells.contains(&at)
                {
                    ensure!(
                        tags.insert(at, value).is_none(),
                        "选区含重复格子标签，先消歧后操作"
                    );
                }
            }
        }
        let cells = cells
            .iter()
            .map(|&at| RegionCell {
                at,
                terrain: layers.terrain.then(|| {
                    doc.terrain(at).unwrap_or(TerrainValue {
                        tile: 0,
                        sub: 0,
                        level: doc.base.header.level as u8,
                        ice: 0,
                    })
                }),
                overlay: layers.overlays.then(|| doc.overlay(at)),
                cell_tag: tags.get(&at).cloned(),
            })
            .collect();
        Ok(Self {
            resource_profile: doc.resource_profile.clone(),
            origin,
            cells,
            records,
            layers,
            theater: doc.base.header.theater.clone(),
        })
    }
    pub fn plan(&self, doc: &MapDocument, anchor: Cell) -> Result<PastePlan> {
        ensure!(
            self.theater.eq_ignore_ascii_case(&doc.base.header.theater),
            "剪贴板来自不同地形环境，请重新复制"
        );
        ensure!(
            self.resource_profile == doc.resource_profile,
            "剪贴板资源版本不匹配或缺少来源；请在当前 Mod 中重新复制"
        );
        let translate = |at: Cell| -> Result<Cell> {
            let [x, y] = [0, 1].map(|axis| {
                i32::from(at[axis]) + i32::from(anchor[axis]) - i32::from(self.origin[axis])
            });
            ensure!(
                (0..i32::from(MAX_CELL_SIDE)).contains(&x)
                    && (0..i32::from(MAX_CELL_SIDE)).contains(&y),
                "整个区域超出地图边界"
            );
            let at = [x as u16, y as u16];
            doc.check_cell(at)?;
            Ok(at)
        };
        let cells = self
            .cells
            .iter()
            .map(|c| {
                let at = translate(c.at)?;
                if self.layers.cell_tags && c.cell_tag.is_some() {
                    crate::mission::cell_key(doc, at)?;
                }
                Ok(RegionCell { at, ..c.clone() })
            })
            .collect::<Result<_>>()?;
        let records = self
            .records
            .iter()
            .map(|(id, r)| Ok((*id, r.moved(translate(r.cell().context("对象坐标无效")?)?)?)))
            .collect::<Result<_>>()?;
        Ok(PastePlan {
            cells,
            records,
            cell_tags: self.layers.cell_tags,
        })
    }
}
impl PastePlan {
    /// Copy gives records new identities; moving keeps keys and internal IDs.
    pub fn apply(&self, doc: &mut MapDocument, copy: bool) -> Result<Vec<u64>> {
        doc.atomic(|doc| {
            if self.cell_tags {
                replace_cell_tags(doc, &self.cells)?;
            }
            for cell in &self.cells {
                if let Some(v) = cell.terrain {
                    doc.set(Target::Terrain(cell.at), Some(Value::Terrain(v)))?;
                }
                if let Some((id, frame)) = cell.overlay {
                    doc.set(Target::Overlay(cell.at), Some(Value::Overlay(id, frame)))?;
                }
            }
            let mut ids = Vec::new();
            let mut unit_keys = BTreeMap::new();
            for (id, r) in &self.records {
                let new = if copy {
                    doc.insert_record(r.clone())?
                } else {
                    doc.set(Target::Record(*id), Some(Value::Record(r.clone())))?;
                    *id
                };
                if r.section == "Units" {
                    unit_keys.insert(r.key.clone(), new);
                }
                ids.push(new);
            }
            if copy {
                let current = doc.records();
                for id in &ids {
                    let r = &current[id];
                    if r.section == "Units"
                        && let Some(follows) = r.fields().get(11)
                        && let Some(target) = unit_keys.get(follows)
                    {
                        let changed = r.patched(&[(11, current[target].key.clone())])?;
                        doc.set(Target::Record(*id), Some(Value::Record(changed)))?;
                    }
                }
            }
            Ok(ids)
        })
    }
}
pub fn clear(doc: &mut MapDocument, clip: &Clipboard, clear_tile: i32) -> Result<()> {
    doc.atomic(|doc| {
        if clip.layers.cell_tags {
            let empty: Vec<_> = clip
                .cells
                .iter()
                .map(|cell| RegionCell {
                    cell_tag: None,
                    ..cell.clone()
                })
                .collect();
            replace_cell_tags(doc, &empty)?;
        }
        for cell in &clip.cells {
            if let Some(v) = cell.terrain {
                doc.set(
                    Target::Terrain(cell.at),
                    Some(Value::Terrain(TerrainValue {
                        tile: clear_tile,
                        sub: 0,
                        ..v
                    })),
                )?;
            }
            if cell.overlay.is_some() {
                doc.set(Target::Overlay(cell.at), Some(Value::Overlay(255, 0)))?;
            }
        }
        for (id, _) in &clip.records {
            doc.set(Target::Record(*id), None)?;
        }
        Ok(())
    })
}

/// Index physical targets once. Empty cells outside the old 128 radix need no
/// key, and noncanonical numeric keys must not leave a second tag behind.
fn replace_cell_tags(doc: &mut MapDocument, cells: &[RegionCell]) -> Result<()> {
    let selected: BTreeSet<_> = cells.iter().map(|c| c.at).collect();
    let mut existing = BTreeMap::new();
    for (target, key, _) in doc.ini_entries("CellTags") {
        if let Some(at) = crate::mission::cell_at(doc, &key)
            && selected.contains(&at)
        {
            ensure!(
                existing.insert(at, target).is_none(),
                "目的区域含重复格子标签，先消歧后操作"
            );
        }
    }
    for cell in cells {
        if let Some(target) = existing.remove(&cell.at) {
            doc.set(target, cell.cell_tag.clone().map(Value::Ini))?;
        } else if let Some(value) = &cell.cell_tag {
            let key = crate::mission::cell_key(doc, cell.at)?;
            doc.set_ini("CellTags", &key, Some(value.clone()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "region_tests.rs"]
mod tests;
