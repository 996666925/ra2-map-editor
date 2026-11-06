//! Editor-only Marble Madness materials. Never replace the map's tile IDs or TMP geometry.
use crate::assets::DecodedImage;
use std::path::Path;
use vera20k_core::{
    assets::{mix_archive::MixArchive, tmp_file::TmpFile},
    map::theater::TheaterData,
    rules::ini_parser::IniFile,
};

impl crate::catalog::TerrainPalette {
    pub fn framework_height_tile(&self, key: vera20k_core::map::theater::TileKey) -> bool {
        !self.framework_native.contains(&key.tile_id)
            && self
                .groups
                .get(&key.tile_id)
                .is_some_and(|g| g.category == crate::terrain_library::TerrainCategory::Ground)
            && self
                .templates
                .get(&(key.tile_id, key.variant))
                .is_some_and(|t| {
                    t.width == 1
                        && t.height == 1
                        && t.cells
                            .first()
                            .and_then(|s| *s)
                            .is_some_and(|s| s.slope == 0)
                })
    }
    pub fn framework_rgba(
        &self,
        key: vera20k_core::map::theater::TileKey,
        level: u8,
    ) -> Option<std::borrow::Cow<'_, [u8]>> {
        let image = self.framework_images.get(&key)?;
        Some(
            if self.framework_height_tile(key)
                && let Some(height) = self.framework_heights.get(usize::from(level))
            {
                std::borrow::Cow::Owned(height_material(image, height))
            } else {
                std::borrow::Cow::Borrowed(&image.rgba)
            },
        )
    }
}

pub(crate) struct Source {
    archive: MixArchive,
    ini: IniFile,
}
impl Source {
    pub fn open(directory: &Path, theater: &TheaterData) -> Option<Self> {
        let archive = [
            directory.join("marble.mix"),
            directory.join("FinalAlert2/marble.mix"),
            directory.join("FinalAlert2YR/marble.mix"),
        ]
        .into_iter()
        .find_map(|path| MixArchive::load(&path).ok())?;
        Some(Self {
            archive,
            ini: IniFile::from_bytes(&theater.ini_data).ok()?,
        })
    }

    pub fn template(
        &self,
        theater: &TheaterData,
        tile: u16,
        original: &TmpFile,
    ) -> Option<TmpFile> {
        let set = theater.lookup.tileset_index(tile)?;
        let from = self.ini.section(&format!("TileSet{set:04}"))?;
        let mapped = from.read_int("MarbleMadness", -1);
        if mapped < 0 {
            return None;
        }
        let to = self.ini.section(&format!("TileSet{mapped:04}"))?;
        let index = tile.checked_sub(theater.lookup.bounds().get(usize::from(set))?.start)? + 1;
        if i32::from(index) > to.read_int("TilesInSet", 0) {
            return None;
        }
        let filename = to.read_name("FileName", 128)?;
        let bytes = self
            .archive
            .get_by_name(&format!("{filename}{index:02}.{}", theater.extension))
            .or_else(|| {
                self.archive
                    .get_by_name(&format!("{filename}{index:02}.tem"))
            })?;
        let native = TmpFile::from_bytes(bytes).ok()?;
        if native.template_width != original.template_width
            || native.template_height != original.template_height
            || native.tiles.len() != original.tiles.len()
            || native
                .tiles
                .iter()
                .zip(&original.tiles)
                .any(|(a, b)| match (a, b) {
                    (Some(a), Some(b)) => a.height != b.height || a.ramp_type != b.ramp_type,
                    (None, None) => false,
                    _ => true,
                })
        {
            return None;
        }
        Some(native)
    }

    pub fn heights(&self, theater: &TheaterData) -> Vec<DecodedImage> {
        let set = self
            .ini
            .section_or_empty("General")
            .read_int("HeightBase", -1);
        let Some(section) = self.ini.section(&format!("TileSet{set:04}")) else {
            return Vec::new();
        };
        let Some(name) = section.read_name("FileName", 128) else {
            return Vec::new();
        };
        (1..=section.read_int("TilesInSet", 0).min(128))
            .map_while(|index| {
                let filename = format!("{name}{index:02}.{}", theater.extension);
                let bytes = self
                    .archive
                    .get_by_name(&filename)
                    .or_else(|| self.archive.get_by_name(&format!("{name}{index:02}.tem")))?;
                let tmp = TmpFile::from_bytes(bytes).ok()?;
                let cell = tmp.tiles.first()?.as_ref()?;
                Some(DecodedImage {
                    indices: Vec::new(),
                    rgba: tmp.tile_to_rgba(0, &theater.iso_palette).ok()?,
                    info: crate::assets::AssetInfo {
                        name: filename,
                        source: "marble.mix".into(),
                        palette: format!("iso{}.pal", theater.extension),
                        palette_source: String::new(),
                        index: 0,
                        count: 1,
                        offset: [cell.offset_x, cell.offset_y],
                        width: cell.pixel_width,
                        height: cell.pixel_height,
                    },
                })
            })
            .collect()
    }
}

pub fn height_material(original: &DecodedImage, height: &DecodedImage) -> Vec<u8> {
    let mut rgba = original.rgba.clone();
    for y in 0..original.info.height as i32 {
        for x in 0..original.info.width as i32 {
            let p = (y as usize * original.info.width as usize + x as usize) * 4;
            if rgba[p + 3] == 0 {
                continue;
            }
            let nx = x + original.info.offset[0] - height.info.offset[0];
            let ny = y + original.info.offset[1] - height.info.offset[1];
            if nx >= 0 && ny >= 0 && nx < height.info.width as i32 && ny < height.info.height as i32
            {
                let q = (ny as usize * height.info.width as usize + nx as usize) * 4;
                if height.rgba[q + 3] != 0 {
                    rgba[p..p + 4].copy_from_slice(&height.rgba[q..q + 4]);
                }
            }
        }
    }
    rgba
}

/// Keep height colours and numbers available without Marble MIX and above its height range.
pub(crate) fn height_tiles(mut native: Vec<DecodedImage>) -> Vec<DecodedImage> {
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    for level in native.len()..128 {
        let mut rgba = vec![0; 60 * 30 * 4];
        let t = (level as f32 / 127.).sqrt();
        let color = [
            (105. + 90. * t) as u8,
            (155. + 55. * t) as u8,
            (213. - 75. * t) as u8,
            255,
        ];
        for y in 0..30 {
            for x in 0..60 {
                let radius = (x as f32 - 29.5).abs() / 30. + (y as f32 - 14.5).abs() / 15.;
                if radius > 1. {
                    continue;
                }
                let p = (y * 60 + x) * 4;
                rgba[p..p + 4].copy_from_slice(if radius > 0.9 {
                    &[50, 43, 67, 255]
                } else {
                    &color
                });
            }
        }
        let label = level.to_string();
        let left = 30 - label.len() as i32 * 4;
        for (i, digit) in label.bytes().enumerate() {
            for (y, bits) in DIGITS[usize::from(digit - b'0')].iter().enumerate() {
                for x in 0..3 {
                    if bits & (1 << (2 - x)) == 0 {
                        continue;
                    }
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let p =
                                ((10 + y * 2 + dy) * 60 + (left as usize + i * 8 + x * 2 + dx)) * 4;
                            rgba[p..p + 4].copy_from_slice(&[35, 28, 47, 255]);
                        }
                    }
                }
            }
        }
        native.push(DecodedImage {
            indices: Vec::new(),
            rgba,
            info: crate::assets::AssetInfo {
                name: format!("framework-height-{level}"),
                source: "generated".into(),
                palette: String::new(),
                palette_source: String::new(),
                index: level,
                count: 128,
                offset: [0, 0],
                width: 60,
                height: 30,
            },
        });
    }
    native
}

/// A clean geometric fallback follows the native alpha/Z planes, including cliff faces.
pub fn image(
    original: &DecodedImage,
    tmp: &TmpFile,
    sub: usize,
    native: Option<&TmpFile>,
    theater: &TheaterData,
) -> DecodedImage {
    let cell = tmp.tiles[sub].as_ref().unwrap();
    let mut out = original.clone();
    let native = native.and_then(|tmp| {
        Some((
            tmp.tiles.get(sub)?.as_ref()?,
            tmp.tile_to_rgba(sub, &theater.iso_palette).ok()?,
        ))
    });
    let width = original.info.width as i32;
    let height = original.info.height as i32;
    let occupied = |x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < width
            && y < height
            && original.rgba[((y * width + x) * 4 + 3) as usize] != 0
    };
    for y in 0..height {
        for x in 0..width {
            let p = (y * width + x) as usize;
            if original.rgba[p * 4 + 3] == 0 {
                continue;
            }
            let lx = x + original.info.offset[0];
            let ly = y + original.info.offset[1];
            let z = ly - i32::from(cell.depth[p]);
            let edge = [[-1, 0], [1, 0], [0, -1], [0, 1]]
                .iter()
                .any(|[dx, dy]| !occupied(x + dx, y + dy));
            let crease = x > 0 && cell.depth[p].abs_diff(cell.depth[p - 1]) > 3
                || y > 0 && cell.depth[p].abs_diff(cell.depth[p - width as usize]) > 3;
            let border = ((lx as f32 - 29.5).abs() / 30. + (ly as f32 - 14.5).abs() / 15. - 1.)
                .abs()
                < 0.065;
            let shade = (z / 8).clamp(-5, 5);
            let color = if edge || crease || border {
                [50, 43, 67, 255]
            } else if vera20k_core::rules::terrain_rules::tmp_terrain_to_land_type(
                cell.terrain_type,
            ) == vera20k_core::rules::terrain_rules::LandType::Water
            {
                [111, 166, 216, 255]
            } else {
                [
                    (224 - shade * 10).clamp(80, 250) as u8,
                    (178 - shade * 12).clamp(70, 240) as u8,
                    (205 - shade * 8).clamp(90, 250) as u8,
                    255,
                ]
            };
            out.rgba[p * 4..p * 4 + 4].copy_from_slice(&color);
            if let Some((n, rgba)) = &native {
                let nx = lx - n.offset_x;
                let ny = ly - n.offset_y;
                if nx >= 0 && ny >= 0 && nx < n.pixel_width as i32 && ny < n.pixel_height as i32 {
                    let q = (ny as usize * n.pixel_width as usize + nx as usize) * 4;
                    if rgba[q + 3] != 0 {
                        out.rgba[p * 4..p * 4 + 4].copy_from_slice(&rgba[q..q + 4]);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_and_native_materials_preserve_alpha_offsets_and_map_geometry() {
        let fixture = crate::test_support::Fixture::with_cliffs();
        let mut install = fixture.install();
        install.catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
        let palette = install.terrain_palette();
        assert!(palette.framework_native.is_empty());
        assert_eq!(palette.images.len(), palette.framework_images.len());
        for (key, original) in &palette.images {
            let other = &palette.framework_images[key];
            assert_eq!(original.info.offset, other.info.offset);
            assert_eq!(
                (original.info.width, original.info.height),
                (other.info.width, other.info.height)
            );
            assert!(
                original
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(other.rgba.as_chunks::<4>().0.iter())
                    .all(|(a, b)| a[3] == b[3])
            );
        }
        let bytes = install.manager.get_ref("cliff01.tem").unwrap();
        let tmp = TmpFile::from_bytes(bytes).unwrap();
        let mut mix = vec![0; 4];
        mix.extend(1u16.to_le_bytes());
        mix.extend((bytes.len() as u32).to_le_bytes());
        mix.extend(vera20k_core::assets::mix_hash::mix_hash("MCLIFF01.tem").to_le_bytes());
        mix.extend(0u32.to_le_bytes());
        mix.extend((bytes.len() as u32).to_le_bytes());
        mix.extend(bytes);
        let source = Source {
            archive: MixArchive::from_bytes(mix).unwrap(),
            ini: IniFile::from_bytes(
                b"[TileSet0001]\nMarbleMadness=2\n[TileSet0002]\nFileName=MCLIFF\nTilesInSet=40\n",
            )
            .unwrap(),
        };
        let tile = install.theater.lookup.bounds()[1].start;
        assert!(source.template(&install.theater, tile, &tmp).is_some());
        let wrong = TmpFile::from_bytes(install.manager.get_ref("cliff02.tem").unwrap()).unwrap();
        assert!(source.template(&install.theater, tile, &wrong).is_none());
    }
}
