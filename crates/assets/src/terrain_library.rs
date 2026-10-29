//! Theater-defined terrain groups and complete, depth-composited TMP previews.
use crate::{assets::DecodedImage, catalog::TerrainPalette};
use std::collections::HashMap;
use vera20k_core::{
    assets::tmp_file::TmpFile,
    map::theater::{TheaterData, TileKey},
    rules::ini_parser::IniFile,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TerrainCategory {
    Ground,
    Cliff,
    Ramp,
    Road,
    Water,
    Shore,
    Bridge,
    Other,
}
impl TerrainCategory {
    pub const ALL: [Self; 8] = [
        Self::Ground,
        Self::Cliff,
        Self::Ramp,
        Self::Road,
        Self::Water,
        Self::Shore,
        Self::Bridge,
        Self::Other,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Ground => "地表",
            Self::Cliff => "悬崖",
            Self::Ramp => "坡道",
            Self::Road => "道路",
            Self::Water => "水面",
            Self::Shore => "水岸 / 瀑布",
            Self::Bridge => "桥梁",
            Self::Other => "其他",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TerrainGroup {
    pub id: u16,
    pub name: String,
    pub category: TerrainCategory,
}

/// Numeric theater declarations take precedence over display-name heuristics.
/// Unknown/mod groups remain accessible under their original SetName.
pub fn groups(theater: &TheaterData) -> HashMap<u16, TerrainGroup> {
    use TerrainCategory::*;
    let ini = IniFile::from_bytes(&theater.ini_data).unwrap_or_else(|_| IniFile::empty());
    let general = ini.section_or_empty("General");
    let declarations = [
        (Bridge, &["BridgeSet", "WoodBridgeSet"][..]),
        (
            Ramp,
            &["CliffRamps", "RampBase", "RampSmooth", "SlopeSetPieces"],
        ),
        (Cliff, &["CliffSet", "WaterCliffs", "DestroyableCliffs"]),
        (
            Shore,
            &[
                "ShorePieces",
                "WaterCaves",
                "WaterfallEast",
                "WaterfallWest",
                "WaterfallNorth",
                "WaterfallSouth",
            ],
        ),
        (Water, &["WaterSet"]),
        (Road, &["PavedRoads", "PavedRoadEnds", "Medians"]),
        (
            Ground,
            &[
                "ClearTile",
                "RoughTile",
                "SandTile",
                "GreenTile",
                "PaveTile",
                "ClearToRoughLat",
                "ClearToSandLat",
                "ClearToGreenLat",
                "ClearToPaveLat",
                "MiscPaveTile",
            ],
        ),
    ];
    let mut out = HashMap::new();
    for id in 0..theater.lookup.bounds().len() {
        let id = id as u16;
        let name = theater.lookup.set_name(id).unwrap_or("No Name").to_owned();
        let category = declarations
            .iter()
            .find_map(|(category, keys)| {
                keys.iter()
                    .any(|key| general.read_int(key, -1) == i32::from(id))
                    .then_some(*category)
            })
            .unwrap_or_else(|| classify_name(&name));
        let bounds = theater.lookup.bounds()[usize::from(id)];
        for tile in bounds.start..bounds.start.saturating_add(bounds.count) {
            out.insert(
                tile,
                TerrainGroup {
                    id,
                    name: name.clone(),
                    category,
                },
            );
        }
    }
    out
}

fn classify_name(name: &str) -> TerrainCategory {
    use TerrainCategory::*;
    let name = name.to_ascii_lowercase();
    let contains = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if contains(&["bridge", "桥"]) {
        Bridge
    } else if contains(&["ramp", "slope", "坡"]) {
        Ramp
    } else if contains(&["shore", "beach", "waterfall", "water cave", "岸", "瀑布"]) {
        Shore
    } else if contains(&["cliff", "悬崖"]) {
        Cliff
    } else if contains(&["road", "highway", "rail", "track", "道路", "公路"]) {
        Road
    } else if contains(&["water", "水面"]) {
        Water
    } else if contains(&[
        "clear", "rough", "sand", "grass", "green", "pav", "lat", "dirt", "地表",
    ]) {
        Ground
    } else {
        Other
    }
}

/// Screen bounds are relative to template cell (0,0), with base level zero.
/// Use the same TMP Z comparison as the scene, including extra cliff planes.
pub fn template_preview(
    palette: &TerrainPalette,
    tile: u16,
    variant: u8,
    tmp: &TmpFile,
) -> Option<DecodedImage> {
    let mut parts = Vec::new();
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for (sub, cell) in tmp.tiles.iter().enumerate() {
        let Some(cell) = cell else { continue };
        let image = palette.images.get(&TileKey {
            tile_id: tile,
            sub_tile: sub as u8,
            variant,
        })?;
        let col = sub as i32 % tmp.template_width as i32;
        let row = sub as i32 / tmp.template_width as i32;
        let x = (col - row) * 30 - 30 + image.info.offset[0];
        let y = (col + row) * 15 + 15 - i32::from(cell.height) * 15 + image.info.offset[1];
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x + image.info.width as i32);
        bounds[3] = bounds[3].max(y + image.info.height as i32);
        parts.push((image, cell, x, y, -(col + row) * 15 - 30));
    }
    let first = parts.first()?.0;
    let width = u32::try_from(bounds[2] - bounds[0]).ok()?;
    let height = u32::try_from(bounds[3] - bounds[1]).ok()?;
    let count = (width as usize).checked_mul(height as usize)?;
    if count == 0 || count > 16_777_216 {
        return None;
    }
    let mut rgba = vec![0; count * 4];
    let mut depth = vec![i32::MAX; count];
    for (image, cell, left, top, base) in parts {
        for y in 0..image.info.height as usize {
            for x in 0..image.info.width as usize {
                let src = y * image.info.width as usize + x;
                if image.rgba[src * 4 + 3] == 0 {
                    continue;
                }
                let dst = (y + (top - bounds[1]) as usize) * width as usize
                    + x
                    + (left - bounds[0]) as usize;
                let z = base + i32::from(cell.depth[src]);
                if z <= depth[dst] {
                    depth[dst] = z;
                    rgba[dst * 4..dst * 4 + 4].copy_from_slice(&image.rgba[src * 4..src * 4 + 4]);
                }
            }
        }
    }
    let mut info = first.info.clone();
    info.width = width;
    info.height = height;
    info.offset = [bounds[0], bounds[1]];
    Some(DecodedImage {
        indices: Vec::new(),
        info,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetInfo;
    use vera20k_core::assets::tmp_file::TmpTile;

    #[test]
    fn numeric_theater_groups_override_names_and_preserve_mod_group_identity() {
        let fixture = crate::test_support::Fixture::new();
        std::fs::write(fixture.0.join("temperatmd.ini"), b"[General]\nClearTile=0\nCliffSet=1\nCliffRamps=2\n[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=1\n[TileSet0001]\nSetName=Custom Escarpment\nFileName=mod\nTilesInSet=3\n[TileSet0002]\nSetName=Custom Pass\nFileName=pass\nTilesInSet=2\n[TileSet0003]\nSetName=Alien Surface\nFileName=alien\nTilesInSet=1\n").unwrap();
        let install = fixture.install();
        let groups = groups(&install.theater);
        assert_eq!(groups[&1].category, TerrainCategory::Cliff);
        assert_eq!(groups[&3].id, 1);
        assert_eq!(groups[&4].category, TerrainCategory::Ramp);
        assert_eq!(groups[&6].category, TerrainCategory::Other);
        assert_eq!(groups[&6].name, "Alien Surface");
        assert_eq!(classify_name("Water Cliffs"), TerrainCategory::Cliff);
        assert_eq!(classify_name("Cliff Ramps"), TerrainCategory::Ramp);
    }

    #[test]
    fn complete_preview_respects_extra_offsets_height_holes_transparency_and_tmp_depth() {
        let tile = |height| {
            Some(TmpTile {
                height,
                terrain_type: 0,
                ramp_type: 0,
                radar_left: [0; 3],
                radar_right: [0; 3],
                pixels: vec![1, 1],
                depth: vec![60, 60],
                pixel_width: 2,
                pixel_height: 1,
                relative_extra_y: 0,
                offset_x: 0,
                offset_y: 0,
                has_damaged_data: false,
            })
        };
        let mut tmp = TmpFile {
            template_width: 3,
            template_height: 1,
            tile_width: 60,
            tile_height: 30,
            tiles: vec![tile(0), tile(1), None],
        };
        tmp.tiles[0].as_mut().unwrap().depth = vec![0, 0];
        let image = |offset, rgba| DecodedImage {
            indices: vec![],
            info: AssetInfo {
                name: "fixture".into(),
                source: String::new(),
                palette: String::new(),
                palette_source: String::new(),
                index: 0,
                count: 3,
                offset,
                width: 2,
                height: 1,
            },
            rgba,
        };
        let mut palette = TerrainPalette::default();
        palette.images.insert(
            TileKey {
                tile_id: 1,
                sub_tile: 0,
                variant: 0,
            },
            image([0, 0], vec![255, 0, 0, 255, 0, 0, 0, 0]),
        );
        palette.images.insert(
            TileKey {
                tile_id: 1,
                sub_tile: 1,
                variant: 0,
            },
            image([-30, 0], vec![0, 0, 255, 255, 0, 255, 0, 255]),
        );
        let preview = template_preview(&palette, 1, 0, &tmp).unwrap();
        assert_eq!(preview.info.offset, [-30, 15]);
        assert_eq!([preview.info.width, preview.info.height], [2, 1]);
        assert_eq!(preview.rgba, vec![255, 0, 0, 255, 0, 255, 0, 255]);
        palette.images.remove(&TileKey {
            tile_id: 1,
            sub_tile: 1,
            variant: 0,
        });
        assert!(template_preview(&palette, 1, 0, &tmp).is_none());
    }
}
