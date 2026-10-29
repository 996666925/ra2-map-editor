//! Authored blank maps: full Size diamond, clear TMP, empty overlays and starts.
use crate::raw_ini::{lcw_encode, pack_lines};
use anyhow::{Context, Result, ensure};
use vera20k_core::util::lzo;

pub const MIN_MAP_DIMENSION: u16 = 20;
/// VERA20k admits Size headers only when width + height fits its cell stride.
pub const MAX_MAP_SPAN: u16 = vera20k_core::map::cell_index::CELL_ROW_STRIDE as u16;
pub const MAX_MAP_DIMENSION: u16 = MAX_MAP_SPAN - MIN_MAP_DIMENSION;
pub const MAX_PLAYERS: u8 = 30;
pub const THEATERS: [(&str, &str); 6] = [
    ("TEMPERATE", "温带"),
    ("SNOW", "雪地"),
    ("URBAN", "城市"),
    ("NEWURBAN", "新城市"),
    ("DESERT", "沙漠"),
    ("LUNAR", "月球"),
];

#[derive(Clone, Debug)]
pub struct NewMapOptions {
    pub title: String,
    pub theater: String,
    pub width: u16,
    pub height: u16,
    pub level: u8,
    pub players: u8,
}
impl Default for NewMapOptions {
    fn default() -> Self {
        Self {
            title: "New Map".into(),
            theater: "TEMPERATE".into(),
            width: 80,
            height: 80,
            level: 0,
            players: 2,
        }
    }
}
impl NewMapOptions {
    pub fn validate(&self) -> Result<()> {
        let title = self.title.trim();
        ensure!(
            THEATERS.iter().any(|(id, _)| *id == self.theater),
            "未知地图环境"
        );
        ensure!(
            !title.is_empty() && title.len() <= 120,
            "地图名称不能为空，最多 120 字节"
        );
        ensure!(
            !title.chars().any(|c| c.is_control() || c == ';'),
            "地图名称不能包含控制字符或分号"
        );
        ensure!(
            (MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION).contains(&self.width)
                && (MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION).contains(&self.height),
            "地图宽度和高度须在 {MIN_MAP_DIMENSION}～{MAX_MAP_DIMENSION} 之间（含边界）"
        );
        ensure!(
            u32::from(self.width) + u32::from(self.height) <= u32::from(MAX_MAP_SPAN),
            "VERA20k 要求地图宽度＋高度不超过 {MAX_MAP_SPAN}"
        );
        ensure!(self.level <= 14, "初始高度须在 0～14 之间");
        ensure!(
            (2..=MAX_PLAYERS).contains(&self.players),
            "玩家起点数量须在 2～{MAX_PLAYERS} 之间"
        );
        Ok(())
    }
    /// Native Size order begins at (1, width), followed by increasing x+y.
    /// Explicit records keep creation independent of runtime fill randomness.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let width = u32::from(self.width);
        let height = u32::from(self.height);
        let mut terrain = Vec::with_capacity((height * (2 * width - 1) * 11 + 4) as usize);
        for sum in width + 1..=width + 2 * height {
            for x in 0..=sum {
                let y = sum - x;
                if x.abs_diff(y) >= width {
                    continue;
                }
                terrain.extend_from_slice(&(x as u16).to_le_bytes());
                terrain.extend_from_slice(&(y as u16).to_le_bytes());
                terrain.extend_from_slice(&0i32.to_le_bytes());
                terrain.extend_from_slice(&[0, self.level, 0]);
            }
        }
        terrain.extend_from_slice(&[0; 4]);
        let mut out = format!(
            "; Created by YR Map Editor\r\n\
             [Basic]\r\nName={}\r\nNewINIFormat=4\r\nMultiplayerOnly=yes\r\nMaxPlayers={}\r\n\
             [Map]\r\nTheater={}\r\nSize=0,0,{width},{height}\r\n\
             LocalSize=2,4,{},{}\r\nLevel={}\r\nFill=Clear\r\n\
             [Lighting]\r\nAmbient=1.000000\r\nRed=1.000000\r\nGreen=1.000000\r\nBlue=1.000000\r\n\
             [IsoMapPack5]\r\n",
            self.title.trim(),
            self.players,
            self.theater,
            width - 4,
            height - 10,
            self.level
        )
        .into_bytes();
        out.extend(pack_lines(&lzo::compress_chunks(&terrain, 8192), b"\r\n"));
        for (section, value) in [("OverlayPack", 255), ("OverlayDataPack", 0)] {
            out.extend_from_slice(format!("[{section}]\r\n").as_bytes());
            out.extend(pack_lines(&lcw_encode(&vec![value; 512 * 512]), b"\r\n"));
        }
        out.extend_from_slice(b"[Waypoints]\r\n");
        // Distribute starts around an inset rectangle in projected map space.
        // Two players face diagonally, then add the remaining corners/edges.
        let positions = [
            (1, 1),
            (3, 3),
            (3, 1),
            (1, 3),
            (2, 1),
            (2, 3),
            (1, 2),
            (3, 2),
        ];
        // Membership uses x+y against twice LocalSize's top/height.
        // Keep starts inside both the geometric and elevated playfield.
        let low = 8 + u32::from(self.level);
        let high = 2 * height - 10;
        let mut starts: Vec<_> = positions
            .iter()
            .take(self.players as usize)
            .map(|&(u, v)| (2 + (width - 4) * u / 4, low + (high - low) * v / 4))
            .collect();
        // Preserve the first eight starts. Add well-spaced points by maximizing
        // distance to the closest existing start in projected map space.
        // Sampling strictly inside the bounds also works for 20x20 at level 14.
        let candidates: Vec<_> = (0..=32)
            .flat_map(|u| {
                (0..=32).map(move |v| {
                    (
                        3 + (width - 6) * u / 32,
                        low + 1 + (high - low - 1) * v / 32,
                    )
                })
            })
            .collect();
        while starts.len() < usize::from(self.players) {
            let next = candidates
                .iter()
                .filter(|point| !starts.contains(point))
                .max_by_key(|&&(q, r)| {
                    starts
                        .iter()
                        .map(|&(sq, sr)| 4 * q.abs_diff(sq).pow(2) + r.abs_diff(sr).pow(2))
                        .min()
                        .unwrap_or(0)
                })
                .context("可玩区域无法容纳所需的不同玩家起点")?;
            starts.push(*next);
        }
        for (index, (q, r)) in starts.into_iter().enumerate() {
            let x = q + r.div_ceil(2);
            let y = width + r / 2 - q;
            out.extend_from_slice(format!("{index}={}\r\n", y * 1000 + x).as_bytes());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::MapDocument, source::SourceMap};
    use std::collections::HashSet;
    use vera20k_core::map::playfield::{PlayfieldBounds, size_diamond_contains};

    #[test]
    fn blank_maps_cover_the_exact_diamond_and_have_distinct_playable_starts() {
        for (width, height, level, players) in [
            (20, 20, 0, 30),
            (20, 20, 14, 30),
            (20, 492, 14, 30),
            (492, 20, 14, 30),
            (81, 43, 7, 3),
            (256, 256, 14, 30),
            (255, 257, 0, 8),
        ] {
            let options = NewMapOptions {
                width,
                height,
                level,
                players,
                ..Default::default()
            };
            let bytes = options.bytes().unwrap();
            let (source, map) =
                SourceMap::parse("Untitled.map".into(), "new".into(), bytes.clone()).unwrap();
            assert_eq!(
                (map.header.width, map.header.height),
                (width.into(), height.into())
            );
            assert_eq!(map.header.theater, "TEMPERATE");
            assert_eq!(
                map.ini.section_or_empty("Basic").read_int("MaxPlayers", 0),
                i32::from(players)
            );
            assert_eq!(
                vera20k_core::map::waypoints::skirmish_player_capacity(&map.ini),
                i32::from(players)
            );
            let bounds = PlayfieldBounds::from_map_header(&map.header);
            assert_eq!(
                (
                    bounds.off_fc,
                    bounds.off_100,
                    bounds.off_104,
                    bounds.off_108
                ),
                (2, 4, i32::from(width) - 4, i32::from(height) - 10)
            );
            let cells: HashSet<_> = map.cells.iter().map(|c| (c.rx, c.ry)).collect();
            assert_eq!(
                cells.len(),
                usize::from(height) * (2 * usize::from(width) - 1)
            );
            assert!(
                map.cells
                    .iter()
                    .all(|c| c.tile_index == 0 && c.sub_tile == 0 && c.z == level)
            );
            for y in 0..=width + height {
                for x in 0..=width + height {
                    assert_eq!(
                        cells.contains(&(x, y)),
                        size_diamond_contains(width.into(), height.into(), (x as i16, y as i16))
                    );
                }
            }
            let doc = MapDocument::new(source.edit_base.unwrap());
            let starts: Vec<_> = doc.records().into_values().collect();
            assert_eq!(starts.len(), players as usize);
            for index in 0..players {
                assert!(starts.iter().any(|r| r.key == index.to_string()));
            }
            let unique: HashSet<_> = starts.iter().map(|r| r.cell().unwrap()).collect();
            assert_eq!(unique.len(), starts.len());
            assert!(starts.iter().all(|r| {
                let [x, y] = r.cell().unwrap();
                r.section == "Waypoints"
                    && cells.contains(&(x, y))
                    && bounds.contains_geometry_packed(x.into(), y.into())
                    && bounds.contains_height_aware_packed(x.into(), y.into(), level as i8, 0)
            }));
            assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
            for cell in &map.cells {
                assert_eq!(doc.overlay([cell.rx, cell.ry]), (255, 0));
                assert!(doc.terrain([cell.rx, cell.ry]).is_some());
            }
        }
    }
    #[test]
    fn invalid_creation_options_fail_before_allocating_or_injecting_ini() {
        let options = NewMapOptions::default();
        for title in [
            "",
            "  ",
            "map\n[Map]\nSize=0,0,9999,9999",
            "map\0",
            "name;comment",
        ] {
            assert!(
                NewMapOptions {
                    title: title.into(),
                    ..options.clone()
                }
                .bytes()
                .is_err()
            );
        }
        for (width, height) in [
            (0, 80),
            (19, 80),
            (493, 20),
            (20, 493),
            (256, 257),
            (65535, 65535),
        ] {
            assert!(
                NewMapOptions {
                    width,
                    height,
                    ..options.clone()
                }
                .bytes()
                .is_err()
            );
        }
        assert!(
            NewMapOptions {
                level: 15,
                ..options.clone()
            }
            .bytes()
            .is_err()
        );
        for players in [0, 1, 31, 255] {
            assert!(
                NewMapOptions {
                    players,
                    ..options.clone()
                }
                .bytes()
                .is_err()
            );
        }
    }
}
