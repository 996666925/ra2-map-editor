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
/// Edit just the last byte of each existing IsoMapPack5 cell record.
pub fn set_ice_growth(doc: &mut MapDocument, cells: &BTreeSet<Cell>, enabled: bool) -> Result<()> {
    doc.atomic(|doc| {
        for &at in cells {
            if let Some(terrain) = doc.terrain(at)
                && (terrain.ice != 0) != enabled
            {
                doc.set(
                    Target::Terrain(at),
                    Some(Value::Terrain(TerrainValue {
                        ice: u8::from(enabled),
                        ..terrain
                    })),
                )?;
            }
        }
        Ok(())
    })
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
    #[serde(default)]
    requires_vera_ice: bool,
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
    pub fn capture_map(source: &MapDocument, layers: RegionLayers) -> Result<Self> {
        let width = source.base.header.width as i32;
        let height = source.base.header.height as i32;
        let mut cells = BTreeSet::new();
        for sum in width + 1..=width + 2 * height {
            for x in 0..=sum {
                let y = sum - x;
                if (x - y).abs() < width {
                    cells.insert([x as u16, y as u16]);
                }
            }
        }
        Self::capture(source, &cells, layers)
    }

    /// Read spatial content using the destination's resources. Mission definitions
    /// stay in the destination, so imported objects must not bind to its tags.
    pub fn import_map(source: &MapDocument, destination: &MapDocument) -> Result<Self> {
        let mut clip = Self::capture_map(
            source,
            RegionLayers {
                waypoints: true,
                cell_tags: false,
                ..Default::default()
            },
        )?;
        clip.validate_vera_ice(destination)?;
        clip.resource_profile = destination.resource_profile.clone();
        // Imported tile numbers are interpreted by the destination theater.
        clip.theater = destination.base.header.theater.clone();
        let modern = destination
            .ini_text("Basic", "NewINIFormat")
            .parse::<i32>()
            .unwrap_or(0)
            >= 4;
        let units: BTreeSet<_> = clip
            .records
            .iter()
            .filter(|(_, r)| r.section == "Units")
            .map(|(_, r)| r.key.clone())
            .collect();
        for (_, record) in &mut clip.records {
            if record.section == "Terrain" {
                let at = record.cell().context("地形对象坐标无效")?;
                record.coordinate_radix = if modern { 1000 } else { 128 };
                *record = record.moved(at)?;
            }
            let tag = match record.section.as_str() {
                "Structures" => Some(6),
                "Units" | "Aircraft" => Some(7),
                "Infantry" => Some(8),
                _ => None,
            };
            let fields = record.fields();
            if let Some(tag) = tag.filter(|&i| i < fields.len()) {
                *record = record.patched(&[(tag, "None".into())])?;
            }
            if record.section == "Units"
                && let Some(follows) = fields.get(11)
                && follows != "-1"
                && !units.contains(follows)
            {
                *record = record.patched(&[(11, "-1".into())])?;
            }
        }
        Ok(clip)
    }

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
                layers.includes(&r.section)
                    && !crate::startup_garrison::owns_waypoint(doc, r)
                    && r.cell().is_some_and(|at| cells.contains(&at))
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
        let cells: Vec<_> = cells
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
            requires_vera_ice: layers.terrain
                && doc.vera_ice_enabled()
                && cells.iter().any(|cell| {
                    cell.terrain
                        .is_some_and(|v| crate::document::VERA_ICE_TILES.contains(&v.tile))
                }),
            origin,
            cells,
            records,
            layers,
            theater: doc.base.header.theater.clone(),
        })
    }

    fn validate_vera_ice(&self, doc: &MapDocument) -> Result<()> {
        ensure!(
            !self.requires_vera_ice || doc.vera_ice_enabled(),
            "目标地图未启用 VERA 可破裂冰面，无法导入冰面或雪岸地块"
        );
        Ok(())
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
        self.validate_vera_ice(doc)?;
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
            let mut ids = if copy {
                doc.insert_records(
                    self.records
                        .iter()
                        .map(|(_, record)| record.clone())
                        .collect(),
                )?
            } else {
                Vec::with_capacity(self.records.len())
            };
            let mut unit_keys = BTreeMap::new();
            for (index, (id, r)) in self.records.iter().enumerate() {
                let new = if copy {
                    ids[index]
                } else {
                    doc.set(Target::Record(*id), Some(Value::Record(r.clone())))?;
                    *id
                };
                if r.section == "Units" {
                    unit_keys.insert(r.key.clone(), new);
                }
                if !copy {
                    ids.push(new);
                }
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
