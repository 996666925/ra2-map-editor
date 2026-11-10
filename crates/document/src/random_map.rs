//! Random-map options and serialization of the engine's generated flat tile IDs.
use crate::{
    new_map::{MAX_PLAYERS, NewMapOptions},
    raw_ini::{lcw_encode, pack_lines},
};
use anyhow::{Result, ensure};
use vera20k_core::{
    map::{
        entities::EntityCategory,
        map_file::MapFile,
        rmg::{MapGeometry, RmgOptions, emit},
    },
    util::lzo,
};

pub const LANDSCAPES: [&str; 5] = ["群岛", "大陆", "海中岛屿", "内陆湖泊", "山地"];
pub const ENVIRONMENTS: [&str; 5] = ["温带", "雪地", "城市", "沙漠", "新城市"];
pub const SIZES: [&str; 8] = ["小", "中", "大", "特大", "巨大", "超大", "极大", "最大"];

#[derive(Clone, Debug)]
pub struct RandomMapOptions {
    pub title: String,
    pub terrain: RmgOptions,
    pub ore: bool,
    pub tech_buildings: bool,
}

impl Default for RandomMapOptions {
    fn default() -> Self {
        Self {
            title: "Random Map".into(),
            terrain: RmgOptions {
                map_type: 3,
                seed: 1,
                width: 2,
                height: 2,
                water_amount: 20,
                ruggedness: 20,
                vegetation: 35,
                urban_presence: 50,
                tiberium: 60,
                tiberium_layout: 50,
                accessibility: 70,
                resources: 3,
                ..Default::default()
            },
            ore: true,
            tech_buildings: true,
        }
    }
}

impl RandomMapOptions {
    /// Restore the complete editor recipe when reopening a generated map.
    pub fn from_ini(ini: &vera20k_core::rules::ini_parser::IniFile, title: &str) -> Option<Self> {
        let section = ini.section("RandomMap")?;
        let mut options = Self::default();
        options.terrain.apply_sed(ini);
        // The source owns decoded user-facing text; native INI strings widen
        // legacy bytes instead of decoding editor-authored UTF-8.
        options.title = title.to_owned();
        options.ore = section.read_bool("EditorOre", true);
        options.tech_buildings = section.read_bool("EditorTechBuildings", true);
        options.validate().ok()?;
        Some(options)
    }
    pub fn dimensions(&self) -> (u16, u16) {
        let geometry = self.geometry();
        (geometry.map_w as u16, geometry.map_h as u16)
    }

    /// Use Vera's native geometry, including extended player counts and sizes.
    pub fn geometry(&self) -> MapGeometry {
        // VERA20k owns the player-count and extended-size geometry rules.
        MapGeometry::from_options(&self.terrain)
    }

    pub fn validate(&self) -> Result<()> {
        let t = &self.terrain;
        ensure!((0..=4).contains(&t.theater), "随机地图环境无效");
        ensure!((0..=4).contains(&t.map_type), "随机地图地貌无效");
        ensure!(
            (0..=vera20k_core::map::rmg::MAX_SIZE_OPTION).contains(&t.width)
                && (0..=vera20k_core::map::rmg::MAX_SIZE_OPTION).contains(&t.height),
            "随机地图尺寸档位无效"
        );
        ensure!(
            (2..=i32::from(MAX_PLAYERS)).contains(&t.num_players),
            "随机地图支持 2～{MAX_PLAYERS} 个玩家起点"
        );
        ensure!((0..=65535).contains(&t.seed), "随机种子须在 0～65535 之间");
        ensure!(
            [
                t.water_amount,
                t.ruggedness,
                t.vegetation,
                t.accessibility,
                t.region_size,
                t.tiberium_layout,
                t.urban_presence
            ]
            .iter()
            .all(|v| (0..=100).contains(v))
                && (1..=100).contains(&t.tiberium),
            "随机地图比例参数超出范围"
        );
        ensure!(matches!(t.resources, 1 | 3), "资源类型无效");
        let (width, height) = self.dimensions();
        NewMapOptions {
            title: self.title.clone(),
            theater: emit::theater_name(t.theater).into(),
            width,
            height,
            players: t.num_players as u8,
            ..Default::default()
        }
        .validate()
    }

    /// `raw_tile` must invert the active theater's legacy map translation.
    /// Writing the generator's flat IDs directly corrupts saved shore/cliff sets.
    pub fn map_bytes(
        &self,
        map: &MapFile,
        raw_tile: impl Fn(i32) -> Result<i32>,
    ) -> Result<Vec<u8>> {
        self.validate()?;
        let h = &map.header;
        ensure!(
            map.waypoints.len() == self.terrain.num_players as usize,
            "未能生成全部玩家起点，请减少水量、玩家数或更换种子"
        );
        let mut out = format!(
            "; Created by YR Map Editor random map generator\r\n\
             [Basic]\r\nName={}\r\nNewINIFormat=4\r\nMultiplayerOnly=yes\r\nMinPlayers=2\r\nMaxPlayers={}\r\n\
             [Map]\r\nTheater={}\r\nSize=0,0,{},{}\r\nLocalSize={},{},{},{}\r\nLevel=0\r\nFill=Clear\r\n",
            self.title.trim(), self.terrain.num_players, h.theater, h.width, h.height,
            h.local_left, h.local_top, h.local_width, h.local_height
        ).into_bytes();
        // Persist the recipe in a harmless INI section for reproducibility.
        out.extend(self.terrain.to_sed_bytes());
        out.extend_from_slice(
            format!(
                "EditorOre={}\r\nEditorTechBuildings={}\r\n",
                self.ore, self.tech_buildings
            )
            .as_bytes(),
        );
        out.extend_from_slice(b"[Lighting]\r\n");
        let lighting = map.ini.section_or_empty("Lighting");
        for key in ["Ambient", "Red", "Green", "Blue"] {
            out.extend_from_slice(
                format!("{key}={}\r\n", lighting.read_string(key, "1.000000", 64)).as_bytes(),
            );
        }
        let mut terrain = Vec::with_capacity(map.cells.len() * 11 + 4);
        for cell in &map.cells {
            terrain.extend_from_slice(&cell.rx.to_le_bytes());
            terrain.extend_from_slice(&cell.ry.to_le_bytes());
            terrain.extend_from_slice(&raw_tile(cell.tile_index)?.to_le_bytes());
            terrain.extend_from_slice(&[cell.sub_tile, cell.z, cell.legacy_ice_byte]);
        }
        terrain.extend_from_slice(&[0; 4]);
        out.extend_from_slice(b"[IsoMapPack5]\r\n");
        out.extend(pack_lines(&lzo::compress_chunks(&terrain, 8192), b"\r\n"));
        let side = usize::from(map.header.cell_grid().side());
        let mut overlay = vec![255; side * side];
        let mut density = vec![0; overlay.len()];
        for entry in &map.overlays {
            ensure!(
                usize::from(entry.rx) < side && usize::from(entry.ry) < side,
                "生成的覆盖物越界"
            );
            let at = usize::from(entry.ry) * side + usize::from(entry.rx);
            overlay[at] = entry.overlay_id;
            density[at] = entry.frame;
        }
        for (section, data) in [("OverlayPack", overlay), ("OverlayDataPack", density)] {
            out.extend_from_slice(format!("[{section}]\r\n").as_bytes());
            out.extend(pack_lines(&lcw_encode(&data), b"\r\n"));
        }
        out.extend_from_slice(b"[Waypoints]\r\n");
        for slot in 0..self.terrain.num_players as u32 {
            let start = map
                .waypoints
                .get(&slot)
                .ok_or_else(|| anyhow::anyhow!("玩家起点编号不完整"))?;
            out.extend_from_slice(
                format!(
                    "{slot}={}\r\n",
                    u32::from(start.ry) * 1000 + u32::from(start.rx)
                )
                .as_bytes(),
            );
        }
        out.extend_from_slice(b"[Terrain]\r\n");
        for tree in &map.terrain_objects {
            out.extend_from_slice(
                format!(
                    "{}={}\r\n",
                    u32::from(tree.ry) * 1000 + u32::from(tree.rx),
                    tree.name
                )
                .as_bytes(),
            );
        }
        // Urban generation emits civilian foots as well as buildings. Each
        // class has its own scenario columns (infantry facing is column 7).
        for (section, category) in [
            ("Structures", EntityCategory::Structure),
            ("Units", EntityCategory::Unit),
            ("Infantry", EntityCategory::Infantry),
            ("Aircraft", EntityCategory::Aircraft),
        ] {
            out.extend_from_slice(format!("[{section}]\r\n").as_bytes());
            for (index, entity) in map
                .entities
                .iter()
                .filter(|e| e.category == category)
                .enumerate()
            {
                let tail = match category {
                    EntityCategory::Structure => {
                        format!("{},None,1,0,1,0,0,None,None,None,0", entity.facing)
                    }
                    EntityCategory::Infantry => {
                        format!("{},None,{},None,0,-1,0,1,1", entity.sub_cell, entity.facing)
                    }
                    EntityCategory::Unit => format!("{},None,None,0,-1,0,-1,1,1", entity.facing),
                    EntityCategory::Aircraft => format!("{},None,None,0,-1,1,1", entity.facing),
                };
                out.extend_from_slice(
                    format!(
                        "{index}={},{},{},{},{},{tail}\r\n",
                        entity.owner, entity.type_id, entity.health, entity.cell_x, entity.cell_y
                    )
                    .as_bytes(),
                );
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::MapDocument, source::SourceMap};
    use vera20k_core::map::{map_file, overlay::OverlayEntry};

    #[test]
    fn generated_map_roundtrip_keeps_terrain_overlays_objects_starts_and_recipe() {
        let mut bytes = NewMapOptions::default().bytes().unwrap();
        bytes.extend_from_slice(b"[Terrain]\r\n80080=TREE01\r\n[Structures]\r\n0=Neutral,CAOILD,256,85,85,0,None,1,0,1,0,0,None,None,None,0\r\n");
        let mut map = map_file::load_from_bytes(bytes).unwrap();
        map.cells[0].tile_index = 100;
        map.cells[0].sub_tile = 2;
        map.cells[0].z = 4;
        map.cells[0].legacy_ice_byte = 0x80;
        map.overlays.push(OverlayEntry {
            rx: 80,
            ry: 81,
            overlay_id: 102,
            frame: 9,
        });
        let mut options = RandomMapOptions {
            title: "随机战场".into(),
            ..Default::default()
        };
        options.terrain.urban_enabled = true;
        options.terrain.urban_presence = 73;
        let bytes = options
            .map_bytes(&map, |tile| Ok(if tile == 100 { 97 } else { tile }))
            .unwrap();
        let (source, reopened) =
            SourceMap::parse("random.map".into(), "test".into(), bytes.clone()).unwrap();
        assert_eq!(source.title, "随机战场");
        assert_eq!(reopened.cells[0].tile_index, 97);
        assert_eq!(reopened.cells[0].sub_tile, 2);
        assert_eq!(reopened.cells[0].z, 4);
        assert_eq!(reopened.cells[0].legacy_ice_byte, 0x80);
        assert_eq!(reopened.terrain_objects[0].name, "TREE01");
        assert_eq!(reopened.entities[0].type_id, "CAOILD");
        assert_eq!(reopened.waypoints.len(), 2);
        let mut recipe = RmgOptions::default();
        recipe.apply_sed(&reopened.ini);
        assert!(recipe.urban_enabled);
        assert_eq!(recipe.urban_presence, 73);
        let restored = RandomMapOptions::from_ini(&reopened.ini, &source.title).unwrap();
        assert!(restored.terrain.urban_enabled);
        assert_eq!(restored.terrain.urban_presence, 73);
        assert_eq!(restored.terrain.seed, options.terrain.seed);
        assert_eq!(restored.ore, options.ore);
        assert_eq!(
            reopened
                .ini
                .section_or_empty("RandomMap")
                .read_int("Seed", -1),
            1
        );
        let document = MapDocument::new(source.edit_base.unwrap());
        assert_eq!(document.overlay([80, 81]), (102, 9));
        assert_eq!(document.snapshot().bytes().unwrap(), bytes);
        assert!(
            options
                .map_bytes(&map, |_| anyhow::bail!("unrepresentable tile"))
                .is_err()
        );
        map.waypoints.remove(&0);
        assert!(options.map_bytes(&map, Ok).is_err());
    }

    #[test]
    fn generated_civilians_reopen_in_their_own_sections_with_facing_and_idle_mission() {
        let mut bytes = NewMapOptions::default().bytes().unwrap();
        bytes.extend_from_slice(
            b"[Structures]\r\n0=Neutral,CAOILD,256,85,85,0\r\n\
            [Units]\r\n0=Neutral,TRUCKA,256,81,82,192,None,None,0,-1,0,-1,1,1\r\n\
            [Infantry]\r\n0=Neutral,CIV1,256,83,84,2,None,64,None,0,-1,0,1,1\r\n\
            [Aircraft]\r\n0=Neutral,ORCA,256,86,87,128,None,None,0,-1,1,1\r\n",
        );
        let map = map_file::load_from_bytes(bytes).unwrap();
        let saved = RandomMapOptions::default().map_bytes(&map, Ok).unwrap();
        let (source, reopened) =
            SourceMap::parse("town.map".into(), "test".into(), saved.clone()).unwrap();
        assert_eq!(reopened.entities.len(), map.entities.len());
        for original in &map.entities {
            let entity = reopened
                .entities
                .iter()
                .find(|e| e.type_id == original.type_id)
                .unwrap();
            assert_eq!(entity.category, original.category);
            assert_eq!(entity.owner, original.owner);
            assert_eq!(entity.health, original.health);
            assert_eq!(
                (entity.cell_x, entity.cell_y),
                (original.cell_x, original.cell_y)
            );
            assert_eq!(entity.facing, original.facing);
            assert_eq!(entity.sub_cell, original.sub_cell);
            assert_eq!(entity.mission, None);
            assert!(entity.recruitable_a && entity.recruitable_b);
        }
        let document = MapDocument::new(source.edit_base.unwrap());
        for section in ["Structures", "Units", "Infantry", "Aircraft"] {
            assert_eq!(
                document
                    .record_iter()
                    .filter(|(_, r)| r.section == section)
                    .count(),
                1
            );
        }
        assert_eq!(document.snapshot().bytes().unwrap(), saved);
    }

    #[test]
    fn validates_random_map_controls_before_generation() {
        let options = RandomMapOptions::default();
        options.validate().unwrap();
        for field in [
            "theater",
            "map_type",
            "width",
            "height",
            "num_players",
            "seed",
            "water_amount",
            "ruggedness",
            "vegetation",
            "accessibility",
            "region_size",
            "tiberium",
            "tiberium_layout",
            "urban_presence",
            "resources",
        ] {
            let mut bad = options.clone();
            let value = match field {
                "theater" => &mut bad.terrain.theater,
                "map_type" => &mut bad.terrain.map_type,
                "width" => &mut bad.terrain.width,
                "height" => &mut bad.terrain.height,
                "num_players" => &mut bad.terrain.num_players,
                "seed" => &mut bad.terrain.seed,
                "water_amount" => &mut bad.terrain.water_amount,
                "ruggedness" => &mut bad.terrain.ruggedness,
                "vegetation" => &mut bad.terrain.vegetation,
                "accessibility" => &mut bad.terrain.accessibility,
                "region_size" => &mut bad.terrain.region_size,
                "tiberium" => &mut bad.terrain.tiberium,
                "tiberium_layout" => &mut bad.terrain.tiberium_layout,
                "urban_presence" => &mut bad.terrain.urban_presence,
                _ => &mut bad.terrain.resources,
            };
            *value = -1;
            assert!(bad.validate().is_err(), "{field}");
        }
        for region_size in [0, 100, 101] {
            let mut options = options.clone();
            options.terrain.region_size = region_size;
            assert_eq!(options.validate().is_ok(), region_size <= 100);
        }
        for title in ["", "x\n[Map]", "x;comment"] {
            assert!(
                RandomMapOptions {
                    title: title.into(),
                    ..options.clone()
                }
                .validate()
                .is_err()
            );
        }
        for players in 2..=i32::from(MAX_PLAYERS) {
            for size in 0..=vera20k_core::map::rmg::MAX_SIZE_OPTION {
                let mut options = options.clone();
                options.terrain.num_players = players;
                options.terrain.width = size;
                options.terrain.height = size;
                options.validate().unwrap();
                assert_eq!(
                    options.geometry(),
                    MapGeometry::from_options(&options.terrain),
                    "editor geometry must follow the engine for {players} players and size {size}"
                );
            }
        }
        for players in [0, 1, 31, i32::MAX] {
            let mut bad = options.clone();
            bad.terrain.num_players = players;
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn extended_player_capacity_and_all_start_slots_survive_serialization() {
        let mut options = RandomMapOptions::default();
        options.terrain.num_players = i32::from(MAX_PLAYERS);
        let (width, height) = options.dimensions();
        let blank = NewMapOptions {
            width,
            height,
            players: MAX_PLAYERS,
            ..Default::default()
        };
        let map = map_file::load_from_bytes(blank.bytes().unwrap()).unwrap();
        let bytes = options.map_bytes(&map, Ok).unwrap();
        let (source, map) =
            SourceMap::parse("extended.map".into(), "test".into(), bytes.clone()).unwrap();
        assert_eq!(
            vera20k_core::map::waypoints::skirmish_player_capacity(&map.ini),
            i32::from(MAX_PLAYERS)
        );
        assert_eq!(
            map.ini
                .section_or_empty("RandomMap")
                .read_int("NumPlayers", 0),
            i32::from(MAX_PLAYERS)
        );
        assert!((0..u32::from(MAX_PLAYERS)).all(|slot| map.waypoints.contains_key(&slot)));
        let doc = MapDocument::new(source.edit_base.unwrap());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }
}
