//! VERA -SPAWN Battle options: one local player and up to 29 AI opponents.
use anyhow::{Context, Result, ensure};
use vera20k_core::map::map_file::MapFile;
pub const MAX_PLAYTEST_PLAYERS: u8 = ra2_map_document::new_map::MAX_PLAYERS;
pub const PLAYER_COLOR_COUNT: u8 = 8;

/// Explicit extended rosters may be sparse. Without a declaration only a
/// contiguous extended roster distinguishes starts from mission waypoints.
pub fn player_start_indices(declared: Option<u8>, starts: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let starts: std::collections::BTreeSet<_> = starts
        .into_iter()
        .filter(|&n| n < MAX_PLAYTEST_PLAYERS)
        .collect();
    let contiguous = (0..MAX_PLAYTEST_PLAYERS)
        .take_while(|n| starts.contains(n))
        .count() as u8;
    let limit = declared
        .filter(|n| (9..=MAX_PLAYTEST_PLAYERS).contains(n))
        .unwrap_or(contiguous.max(8));
    starts.into_iter().filter(|&n| n < limit).collect()
}

pub fn map_player_starts(map: &MapFile) -> Vec<u8> {
    let (Ok(width), Ok(height)) = (
        i32::try_from(map.header.width),
        i32::try_from(map.header.height),
    ) else {
        return Vec::new();
    };
    let side = map.header.cell_grid().side();
    player_start_indices(
        map.ini
            .section_or_empty("Basic")
            .read_name("MaxPlayers", 16)
            .and_then(|n| n.parse().ok()),
        map.waypoints
            .values()
            .filter(|w| {
                w.rx < side
                    && w.ry < side
                    && [w.rx, w.ry] != [0, 0]
                    && vera20k_core::map::playfield::size_diamond_contains(
                        width,
                        height,
                        (w.rx as i16, w.ry as i16),
                    )
            })
            .filter_map(|w| u8::try_from(w.index).ok()),
    )
}
#[derive(Clone, Debug)]
pub struct PlayOptions {
    /// Window-system monitor name; None lets VERA follow the system.
    pub display: Option<String>,
    pub full_screen: bool,
    /// Existing VERA profile scale: 0 for Auto, otherwise 100 and up in steps of 50.
    pub zoom: u32,
    pub mission: bool,
    pub mission_difficulty: u8,
    pub side: u8,
    pub color: u8,
    pub start: u8,
    pub ai: u8,
    pub ai_side: u8,
    pub difficulty: u8,
    pub credits: u32,
    pub speed: u8,
    pub units: u8,
    pub tech: u8,
    pub superweapons: bool,
    pub crates: bool,
    pub short_game: bool,
    pub seed: i32,
}
impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            display: None,
            full_screen: false,
            zoom: 100,
            mission: false,
            mission_difficulty: 1,
            side: 0,
            color: 0,
            start: 0,
            ai: 0,
            ai_side: 9,
            difficulty: 1,
            credits: 10000,
            speed: 4,
            units: 0,
            tech: 10,
            superweapons: true,
            crates: false,
            short_game: false,
            seed: 12345,
        }
    }
}
impl PlayOptions {
    pub(crate) fn write_settings(&self, directory: &std::path::Path) -> Result<()> {
        ensure!(
            self.zoom == 0 || ((100..=10_000).contains(&self.zoom) && self.zoom.is_multiple_of(50)),
            "试玩游戏缩放档位无效"
        );
        if let Some(display) = &self.display {
            ensure!(
                !display.is_empty() && !display.contains(['\r', '\n', '\0']),
                "试玩显示器设备名无效"
            );
        }
        let settings = format!(
            "[VERA]\r\nFullScreen={}\r\nDisplay={}\r\nZoom={}\r\n",
            if self.full_screen { "Yes" } else { "No" },
            self.display.as_deref().unwrap_or_default(),
            self.zoom,
        );
        std::fs::create_dir_all(directory)?;
        std::fs::write(directory.join("VERA.INI"), settings)?;
        Ok(())
    }

    pub fn spawn_ini(&self, map: &MapFile) -> Result<String> {
        if self.mission {
            ensure!(
                self.mission_difficulty <= 2 && self.speed <= 6,
                "任务难度或速度超出范围"
            );
            let player = map
                .ini
                .section_or_empty("Basic")
                .read_name("Player", 64)
                .context("任务缺少 [Basic] Player")?;
            let roster = vera20k_core::map::houses::parse_house_roster(&map.ini, &[], None);
            ensure!(
                roster.houses.iter().any(|h| h.name == player),
                "任务 Player 未在 [Houses] 中定义：{player}"
            );
            return Ok(format!(
                "[Settings]\nScenario=spawnmap.ini\nIsSinglePlayer=Yes\nCampaignID=-1\nSide=0\nDifficultyModeHuman={}\nDifficultyModeComputer={}\nGameSpeed={}\nBuildOffAlly=No\n",
                self.mission_difficulty,
                2 - self.mission_difficulty,
                self.speed
            ));
        }
        ensure!(
            self.side <= 9
                && self.ai_side <= 9
                && self.color < PLAYER_COLOR_COUNT
                && self.start < MAX_PLAYTEST_PLAYERS
                && self.ai < MAX_PLAYTEST_PLAYERS
                && self.difficulty <= 2
                && self.speed <= 6
                && self.tech <= 10
                && self.credits <= i32::MAX as u32,
            "试玩参数超出 VERA -SPAWN 支持范围"
        );
        let starts = map_player_starts(map);
        ensure!(
            starts.contains(&self.start),
            "试玩起点航点 {} 未定义为有效玩家起点（0～29）",
            self.start
        );
        let available: Vec<_> = starts.into_iter().filter(|&n| n != self.start).collect();
        ensure!(
            available.len() >= usize::from(self.ai),
            "地图有效玩家起点不足以容纳 {} 个 AI（总人数最多 30 人）",
            self.ai
        );
        let yes = |v| if v { "Yes" } else { "No" };
        let mut out = format!(
            "[Settings]\nScenario=spawnmap.ini\nIsSinglePlayer=No\nGameMode=1\nSpecialHouseIsAlly=No\nPlayerCount=1\nAIPlayers={}\nSide={}\nColor={}\nGameSpeed={}\nSeed={}\nCredits={}\nUnitCount={}\nTechLevel={}\nBases=Yes\nSuperweapons={}\nCrates={}\nShortGame={}\n",
            self.ai,
            self.side,
            self.color,
            self.speed,
            self.seed,
            self.credits,
            self.units,
            self.tech,
            yes(self.superweapons),
            yes(self.crates),
            yes(self.short_game)
        );
        out.push_str(&format!("[SpawnLocations]\nMulti1={}\n", self.start));
        for (i, start) in available.iter().take(usize::from(self.ai)).enumerate() {
            out.push_str(&format!("Multi{}={}\n", i + 2, start));
        }
        for (section, value) in [
            ("HouseHandicaps", self.difficulty),
            ("HouseCountries", self.ai_side),
        ] {
            out.push_str(&format!("[{section}]\n"));
            for i in 0..self.ai {
                out.push_str(&format!("Multi{}={}\n", i + 2, value));
            }
        }
        out.push_str("[HouseColors]\n");
        // Keep up to eight participants distinct; larger sessions reuse the
        // stock eight color priorities, without assuming Mod color indices.
        let colors: Vec<_> = (0..PLAYER_COLOR_COUNT)
            .filter(|&n| n != self.color)
            .chain(std::iter::once(self.color))
            .collect();
        for (i, color) in colors.iter().cycle().take(usize::from(self.ai)).enumerate() {
            out.push_str(&format!("Multi{}={}\n", i + 2, color));
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thirty_player_spawn_writes_every_slot_and_reuses_only_stock_colors() {
        let bytes = ra2_map_document::new_map::NewMapOptions {
            players: MAX_PLAYTEST_PLAYERS,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        for color in 0..PLAYER_COLOR_COUNT {
            let options = PlayOptions {
                ai: 29,
                start: 29,
                color,
                ai_side: 8,
                difficulty: 0,
                ..Default::default()
            };
            let ini = vera20k_core::rules::ini_parser::IniFile::from_str(
                &options.spawn_ini(&map).unwrap(),
            );
            let settings = ini.section_or_empty("Settings");
            assert_eq!(settings.read_int("PlayerCount", -1), 1);
            assert_eq!(settings.read_int("AIPlayers", -1), 29);
            assert_eq!(settings.read_int("Color", -1), i32::from(color));
            let locations = ini.section_or_empty("SpawnLocations");
            assert_eq!(locations.read_int("Multi1", -1), 29);
            let mut first_eight_colors = std::collections::BTreeSet::from([i32::from(color)]);
            for slot in 2..=30 {
                let key = format!("Multi{slot}");
                assert_eq!(locations.read_int(&key, -1), slot - 2);
                assert_eq!(ini.section_or_empty("HouseCountries").read_int(&key, -1), 8);
                assert_eq!(ini.section_or_empty("HouseHandicaps").read_int(&key, -1), 0);
                let ai_color = ini.section_or_empty("HouseColors").read_int(&key, -1);
                assert!((0..i32::from(PLAYER_COLOR_COUNT)).contains(&ai_color));
                if slot <= 8 {
                    assert!(first_eight_colors.insert(ai_color));
                }
            }
            for section in [
                "SpawnLocations",
                "HouseCountries",
                "HouseHandicaps",
                "HouseColors",
            ] {
                assert!(!ini.section_or_empty(section).is_present("Multi31"));
            }
        }
        for options in [
            PlayOptions {
                ai: 30,
                ..Default::default()
            },
            PlayOptions {
                start: 30,
                ..Default::default()
            },
        ] {
            assert!(options.spawn_ini(&map).is_err());
        }
    }

    #[test]
    fn large_maps_keep_player_starts_beyond_the_retail_grid() {
        let bytes = ra2_map_document::new_map::NewMapOptions {
            width: 20,
            height: 600,
            players: 30,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let mut map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        assert!(map.waypoints.values().any(|w| w.rx >= 512 || w.ry >= 512));
        assert_eq!(map_player_starts(&map), (0..30).collect::<Vec<_>>());
        assert!(
            PlayOptions {
                ai: 29,
                ..Default::default()
            }
            .spawn_ini(&map)
            .is_ok()
        );
        map.waypoints.get_mut(&29).unwrap().rx = 1024;
        assert_eq!(map_player_starts(&map), (0..29).collect::<Vec<_>>());
    }

    #[test]
    fn sparse_extended_starts_reject_missing_and_outside_waypoints() {
        let bytes = ra2_map_document::new_map::NewMapOptions {
            players: 30,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let mut map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        map.waypoints.retain(|&n, _| n == 0 || n == 29);
        let mut mission = map.waypoints[&0];
        mission.index = 30;
        map.waypoints.insert(30, mission);
        assert_eq!(map_player_starts(&map), vec![0, 29]);
        let options = PlayOptions {
            start: 29,
            ai: 1,
            ..Default::default()
        };
        assert!(options.spawn_ini(&map).is_ok());
        assert!(
            PlayOptions {
                ai: 2,
                ..options.clone()
            }
            .spawn_ini(&map)
            .is_err()
        );
        assert!(
            PlayOptions {
                start: 28,
                ..options.clone()
            }
            .spawn_ini(&map)
            .is_err()
        );
        map.waypoints.get_mut(&29).unwrap().rx = 511;
        map.waypoints.get_mut(&29).unwrap().ry = 511;
        assert_eq!(map_player_starts(&map), vec![0]);
        assert!(options.spawn_ini(&map).is_err());
    }

    #[test]
    fn legacy_maps_require_contiguous_extended_starts() {
        assert_eq!(
            player_start_indices(None, [0, 2, 7, 8, 29, 30]),
            vec![0, 2, 7]
        );
        assert_eq!(
            player_start_indices(None, 0..=30),
            (0..30).collect::<Vec<_>>()
        );
        assert_eq!(player_start_indices(Some(30), [29, 0, 29, 30]), vec![0, 29]);
        assert_eq!(player_start_indices(Some(12), [0, 11, 12, 29]), vec![0, 11]);
    }

    #[test]
    fn mission_uses_map_house_and_complementary_campaign_difficulty() {
        let bytes = ra2_map_document::new_map::NewMapOptions::default()
            .bytes()
            .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let o = PlayOptions {
            mission: true,
            mission_difficulty: 2,
            ..Default::default()
        };
        assert!(o.spawn_ini(&map).is_err());
        let base =
            ra2_map_document::document::Baseline::parse(std::sync::Arc::from(bytes), &map).unwrap();
        let mut doc = ra2_map_document::document::MapDocument::new(base);
        ra2_map_document::mission::seed(&mut doc).unwrap();
        doc.commit("mission");
        let map =
            vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
        let ini = vera20k_core::rules::ini_parser::IniFile::from_str(&o.spawn_ini(&map).unwrap());
        let s = ini.section_or_empty("Settings");
        assert!(s.read_bool("IsSinglePlayer", false));
        assert_eq!(s.read_int("DifficultyModeComputer", -1), 0);
        assert!(!s.is_present("AIPlayers"));
    }
    #[test]
    fn configured_ai_slots_have_distinct_starts_colors_and_reject_missing_starts() {
        let bytes = ra2_map_document::new_map::NewMapOptions {
            players: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        let mut o = PlayOptions {
            ai: 3,
            ai_side: 8,
            difficulty: 0,
            start: 2,
            color: 4,
            ..Default::default()
        };
        let text = o.spawn_ini(&map).unwrap();
        let ini = vera20k_core::rules::ini_parser::IniFile::from_str(&text);
        assert_eq!(
            ini.section_or_empty("Settings").read_int("AIPlayers", -1),
            3
        );
        for n in 2..=4 {
            assert_eq!(
                ini.section_or_empty("HouseCountries")
                    .read_int(&format!("Multi{n}"), -1),
                8
            );
            assert_eq!(
                ini.section_or_empty("HouseHandicaps")
                    .read_int(&format!("Multi{n}"), -1),
                0
            );
        }
        let colors: std::collections::BTreeSet<_> = (2..=4)
            .map(|n| {
                ini.section_or_empty("HouseColors")
                    .read_int(&format!("Multi{n}"), -1)
            })
            .collect();
        assert_eq!(colors.len(), 3);
        assert!(!colors.contains(&4));
        let starts: std::collections::BTreeSet<_> = (1..=4)
            .map(|n| {
                ini.section_or_empty("SpawnLocations")
                    .read_int(&format!("Multi{n}"), -1)
            })
            .collect();
        assert_eq!(starts.len(), 4);
        o.ai = 4;
        assert!(o.spawn_ini(&map).is_err());
    }
    #[test]
    fn speed_and_ai_choices_are_written_to_spawn_settings() {
        let bytes = ra2_map_document::new_map::NewMapOptions::default()
            .bytes()
            .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        for speed in 0..=6 {
            for (ai, difficulty) in [(0, 1), (1, 2)] {
                let options = PlayOptions {
                    speed,
                    ai,
                    difficulty,
                    ai_side: 9,
                    ..Default::default()
                };
                let text = options.spawn_ini(&map).unwrap();
                let ini = vera20k_core::rules::ini_parser::IniFile::from_str(&text);
                assert_eq!(
                    ini.section_or_empty("Settings").read_int("GameSpeed", -1),
                    i32::from(speed)
                );
                assert_eq!(
                    ini.section_or_empty("Settings").read_int("AIPlayers", -1),
                    i32::from(ai)
                );
                let countries = ini.section_or_empty("HouseCountries");
                assert_eq!(countries.is_present("Multi2"), ai > 0);
                if ai > 0 {
                    assert_eq!(countries.read_int("Multi2", -1), 9);
                    assert_eq!(
                        ini.section_or_empty("HouseHandicaps")
                            .read_int("Multi2", -1),
                        2
                    );
                }
            }
        }
        assert!(
            PlayOptions {
                speed: 7,
                ..Default::default()
            }
            .spawn_ini(&map)
            .is_err()
        );
    }
}
