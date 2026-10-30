//! Read-only authoring choices resolved from the active retail installation.
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};
use vera20k_core::{
    assets::{asset_manager::AssetManager, csf_file::CsfFile},
    map::theater::{TheaterData, TileKey},
    rules::ini_parser::IniFile,
};
#[derive(Clone)]
pub struct TerrainBrush {
    pub key: TileKey,
    pub raw: i32,
    pub label: String,
}

#[derive(Default)]
pub struct TerrainPalette {
    pub brushes: Vec<TerrainBrush>,
    pub images: std::collections::HashMap<TileKey, crate::assets::DecodedImage>,
    /// Geometry is decoded on the resource worker, never while dragging a tool.
    pub templates: std::collections::HashMap<(u16, u8), TerrainTemplate>,
    pub tile_ids: Option<vera20k_core::map::rmg::TileIds>,
    pub morphable: std::collections::HashSet<u16>,
    pub groups: std::collections::HashMap<u16, crate::terrain_library::TerrainGroup>,
    pub previews: std::collections::HashMap<(u16, u8), crate::assets::DecodedImage>,
    pub bridge_rim: Option<vera20k_core::map::bridge_rim_tiles::HighBridgeRimTiles>,
    pub raw_ids: std::collections::HashMap<i32, u16>,
}

pub struct TerrainTemplate {
    pub width: u16,
    pub height: u16,
    pub cells: Vec<Option<TerrainSubtile>>,
}

#[derive(Clone, Copy)]
pub struct TerrainSubtile {
    pub height: u8,
    pub slope: u8,
    /// Raw TMP terrain byte, before conversion to the engine land type.
    pub terrain: u8,
}
#[derive(Clone)]
pub struct Catalog {
    pub rules: IniFile,
    /// Active installation language table; shared by map-local catalog clones.
    strings: Option<Arc<CsfFile>>,
    /// Simplified display text cached on resource load, keyed by unchanged IDs.
    object_names: BTreeMap<String, String>,
    pub art: Arc<vera20k_core::rules::art_data::ArtRegistry>,
    pub object_types: BTreeMap<String, Vec<String>>,
    pub overlays: Vec<String>,
    pub raw_tiles: BTreeMap<u16, i32>,
    pub defaults: Vec<TileKey>,
    pub ore: Option<u8>,
    pub gems: Option<u8>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            rules: IniFile::empty(),
            strings: None,
            object_names: BTreeMap::new(),
            art: Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(
                &IniFile::empty(),
            )),
            object_types: BTreeMap::new(),
            overlays: vec![],
            raw_tiles: BTreeMap::new(),
            defaults: vec![],
            ore: None,
            gems: None,
        }
    }
}
impl Catalog {
    /// Display names never replace the type IDs used by authoring operations.
    pub fn object_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.object_names
            .get(&id.to_ascii_uppercase())
            .map_or_else(|| self.raw_object_name(id), String::as_str)
    }

    fn raw_object_name<'a>(&'a self, id: &'a str) -> &'a str {
        let canonical = self
            .object_types
            .values()
            .flatten()
            .chain(self.overlays.iter());
        let rules = self.rules.section(id).or_else(|| {
            canonical
                .into_iter()
                .find(|name| name.eq_ignore_ascii_case(id))
                .and_then(|name| self.rules.section(name))
        });
        rules
            .and_then(|rules| rules.read_name("UIName", 32))
            .and_then(|label| self.strings.as_ref()?.get(label))
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                rules
                    .and_then(|rules| rules.read_name("Name", 256))
                    .filter(|name| !name.trim().is_empty())
            })
            .unwrap_or(id)
    }

    pub fn object_label<'a>(&'a self, id: &'a str) -> Cow<'a, str> {
        let name = self.object_name(id);
        if name == id {
            Cow::Borrowed(id)
        } else {
            Cow::Owned(format!("{name} · {id}"))
        }
    }

    /// Search translated names, original IDs and the rule's descriptive name.
    pub fn object_matches(&self, id: &str, filter: &str) -> bool {
        let filter = crate::chinese::simplified(filter).to_lowercase();
        filter.is_empty()
            || id.to_lowercase().contains(&filter)
            || self.object_name(id).to_lowercase().contains(&filter)
            || self
                .rules
                .section_or_empty(id)
                .read_name("Name", 256)
                .is_some_and(|name| name.to_lowercase().contains(&filter))
    }

    pub fn terrain_palette(&self, manager: &AssetManager, theater: &TheaterData) -> TerrainPalette {
        use vera20k_core::assets::tmp_file::TmpFile;
        let mut palette = TerrainPalette {
            tile_ids: Some(vera20k_core::map::rmg::TileIds::resolve(theater)),
            groups: crate::terrain_library::groups(theater),
            bridge_rim: Some(theater.high_bridge_rim_tiles),
            raw_ids: self.raw_tiles.iter().map(|(&id, &raw)| (raw, id)).collect(),
            ..Default::default()
        };
        for (&tile_id, &raw) in &self.raw_tiles {
            if theater.lookup.is_morphable(tile_id) {
                palette.morphable.insert(tile_id);
            }
            for variant in 0..theater.lookup.total_file_count(tile_id) {
                let Some(name) = theater.lookup.filename_for_variant(tile_id, variant) else {
                    continue;
                };
                let Some(bytes) = manager.get_ref(name) else {
                    continue;
                };
                let Ok(tmp) = TmpFile::from_bytes(bytes) else {
                    continue;
                };
                // Map subtiles are bytes, so larger templates cannot be authored.
                if tmp.tiles.len() > 256 {
                    continue;
                }
                palette.templates.insert(
                    (tile_id, variant),
                    TerrainTemplate {
                        width: tmp.template_width as u16,
                        height: tmp.template_height as u16,
                        cells: tmp
                            .tiles
                            .iter()
                            .map(|cell| {
                                cell.as_ref().map(|cell| TerrainSubtile {
                                    height: cell.height,
                                    slope: cell.ramp_type,
                                    terrain: cell.terrain_type,
                                })
                            })
                            .collect(),
                    },
                );
                for (index, tile) in tmp.tiles.iter().enumerate().take(256) {
                    let Some(tile) = tile else { continue };
                    let Ok(rgba) = tmp.tile_to_rgba(index, &theater.iso_palette) else {
                        continue;
                    };
                    let key = TileKey {
                        tile_id,
                        sub_tile: index as u8,
                        variant,
                    };
                    palette.brushes.push(TerrainBrush {
                        key,
                        raw,
                        label: format!(
                            "{} · {name} · {tile_id}:{index} · v{variant}",
                            palette.groups.get(&tile_id).map_or("", |g| g.name.as_str())
                        ),
                    });
                    palette.images.insert(
                        key,
                        crate::assets::DecodedImage {
                            indices: Vec::new(),
                            info: crate::assets::AssetInfo {
                                name: name.into(),
                                source: String::new(),
                                palette: format!("iso{}.pal", theater.extension),
                                palette_source: String::new(),
                                index,
                                count: tmp.tiles.len(),
                                offset: [tile.offset_x, tile.offset_y],
                                width: tile.pixel_width,
                                height: tile.pixel_height,
                            },
                            rgba,
                        },
                    );
                }
                if let Some(preview) =
                    crate::terrain_library::template_preview(&palette, tile_id, variant, &tmp)
                {
                    palette.previews.insert((tile_id, variant), preview);
                }
            }
        }
        palette
    }
    pub fn ore_family(&self, id: u8) -> bool {
        self.family(id, "TIB", 20)
    }
    pub fn gem_family(&self, id: u8) -> bool {
        self.family(id, "GEM", 12)
    }
    fn family(&self, id: u8, prefix: &str, max: u8) -> bool {
        self.overlays.get(usize::from(id)).is_some_and(|name| {
            name.get(..prefix.len())
                .is_some_and(|part| part.eq_ignore_ascii_case(prefix))
                && name
                    .get(prefix.len()..)
                    .and_then(|suffix| suffix.parse::<u8>().ok())
                    .is_some_and(|value| (1..=max).contains(&value))
        })
    }
    pub fn load(manager: &AssetManager, theater: &TheaterData) -> Arc<Self> {
        let rules = manager
            .get_ref("rulesmd.ini")
            .and_then(|b| IniFile::from_bytes(b).ok())
            .unwrap_or_else(IniFile::empty);
        let art = manager
            .get_ref("artmd.ini")
            .and_then(|b| IniFile::from_bytes(b).ok())
            .unwrap_or_else(IniFile::empty);
        let mut catalog = Self {
            rules,
            strings: CsfFile::load_active(|name| manager.get_ref(name))
                .ok()
                .map(|(table, _)| Arc::new(table)),
            art: Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(&art)),
            ..Self::default()
        };
        catalog.read_registries();
        // Save authored disk IDs, never the already-translated render ID.
        for raw in -1..theater.lookup.len() as i32 {
            let translated = theater.lookup.translate_legacy_map_tile_index(raw);
            if let Ok(tile) = u16::try_from(translated)
                && theater.lookup.filename(translated).is_some()
            {
                catalog.raw_tiles.entry(tile).or_insert(raw);
            }
        }
        for tile in [
            theater.rmg_tiles.clear_tile,
            theater.rmg_tiles.pave_tile,
            theater.rmg_tiles.rough_tile,
            theater.rmg_tiles.water_set,
        ]
        .into_iter()
        .flatten()
        {
            if let Some(name) = theater.lookup.filename(i32::from(tile))
                && manager.contains(name)
            {
                catalog.defaults.push(TileKey {
                    tile_id: tile,
                    sub_tile: 0,
                    variant: 0,
                });
            }
        }
        Arc::new(catalog)
    }
    fn read_registries(&mut self) {
        for (section, registry) in [
            ("Structures", "BuildingTypes"),
            ("Units", "VehicleTypes"),
            ("Infantry", "InfantryTypes"),
            ("Aircraft", "AircraftTypes"),
            ("Terrain", "TerrainTypes"),
            ("Smudge", "SmudgeTypes"),
            ("Animations", "Animations"),
        ] {
            self.object_types.insert(
                section.into(),
                self.rules
                    .section_or_empty(registry)
                    .registry_ids()
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            );
        }
        self.overlays = self
            .rules
            .section_or_empty("OverlayTypes")
            .registry_ids()
            .into_iter()
            .map(str::to_owned)
            .collect();
        self.ore = self
            .overlays
            .iter()
            .position(|id| id.eq_ignore_ascii_case("TIB01"))
            .and_then(|i| u8::try_from(i).ok());
        self.gems = self
            .overlays
            .iter()
            .position(|id| id.eq_ignore_ascii_case("GEM01"))
            .and_then(|i| u8::try_from(i).ok());
        self.refresh_object_names();
    }

    fn refresh_object_names(&mut self) {
        self.object_names = self
            .object_types
            .values()
            .flatten()
            .chain(self.overlays.iter())
            .map(|id| {
                (
                    id.to_ascii_uppercase(),
                    crate::chinese::simplified(self.raw_object_name(id)),
                )
            })
            .collect();
    }
    /// Merge only rule/type sections, never the map's placed [Terrain] records.
    pub fn with_map(self: &Arc<Self>, bytes: &[u8]) -> Arc<Self> {
        let Ok(map) = IniFile::from_bytes(bytes) else {
            return self.clone();
        };
        let registries = [
            "BuildingTypes",
            "VehicleTypes",
            "InfantryTypes",
            "AircraftTypes",
            "TerrainTypes",
            "SmudgeTypes",
            "OverlayTypes",
            "Animations",
        ];
        let custom = map.section_names().iter().any(|s| {
            registries.iter().any(|r| r.eq_ignore_ascii_case(s))
                || self
                    .object_types
                    .values()
                    .flatten()
                    .chain(self.overlays.iter())
                    .any(|id| id.eq_ignore_ascii_case(s))
        });
        if !custom {
            return self.clone();
        }
        let mut catalog = (**self).clone();
        catalog.rules.merge(&map);
        catalog.read_registries();
        // Runtime registries retain previously allocated types; map-local list
        // keys must not remove a stock type or renumber an existing overlay.
        for (section, ids) in &self.object_types {
            let extras = catalog.object_types.remove(section).unwrap_or_default();
            let mut merged = ids.clone();
            for id in extras {
                if !merged.iter().any(|old| old.eq_ignore_ascii_case(&id)) {
                    merged.push(id);
                }
            }
            catalog.object_types.insert(section.clone(), merged);
        }
        let extras = std::mem::take(&mut catalog.overlays);
        catalog.overlays = self.overlays.clone();
        for id in extras {
            if !catalog
                .overlays
                .iter()
                .any(|old| old.eq_ignore_ascii_case(&id))
            {
                catalog.overlays.push(id);
            }
        }
        catalog.ore = catalog
            .overlays
            .iter()
            .position(|id| id.eq_ignore_ascii_case("TIB01"))
            .and_then(|i| u8::try_from(i).ok());
        catalog.gems = catalog
            .overlays
            .iter()
            .position(|id| id.eq_ignore_ascii_case("GEM01"))
            .and_then(|i| u8::try_from(i).ok());
        catalog.refresh_object_names();
        Arc::new(catalog)
    }
    pub fn brushes(
        &self,
        theater: &TheaterData,
        keys: impl Iterator<Item = TileKey>,
    ) -> Vec<TerrainBrush> {
        let mut keys: Vec<_> = keys
            .filter(|key| {
                self.raw_tiles.contains_key(&key.tile_id)
                    && theater
                        .lookup
                        .filename_for_variant(key.tile_id, key.variant)
                        .is_some()
            })
            .collect();
        keys.sort_by_key(|key| (key.tile_id, key.sub_tile, key.variant));
        keys.dedup();
        keys.into_iter()
            .map(|key| TerrainBrush {
                raw: self.raw_tiles[&key.tile_id],
                label: format!(
                    "{} · {}:{}",
                    theater
                        .lookup
                        .filename_for_variant(key.tile_id, key.variant)
                        .unwrap_or("TMP"),
                    key.tile_id,
                    key.sub_tile
                ),
                key,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
