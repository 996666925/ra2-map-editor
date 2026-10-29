//! Worker-side SHP frames and static voxel previews for the material library.
use crate::{
    assets::{AssetInfo, DecodedImage, Install},
    catalog::Catalog,
};
use anyhow::{Context, Result, ensure};
use std::{collections::HashMap, sync::Arc};
use vera20k_core::{
    assets::{pal_file::Palette, shp_file::ShpFile},
    rules::art_data::{self, EffectImageKind},
};

#[derive(Default)]
pub struct SpritePalette {
    pub entries: HashMap<(String, String), SpritePreview>,
    pub overlay_frames: HashMap<(String, u8), Arc<DecodedImage>>,
    pub objects: HashMap<(String, String), crate::map_sprites::ObjectSprite>,
}
pub struct SpritePreview {
    pub image: Option<Arc<DecodedImage>>,
    pub detail: String,
    pub voxel: bool,
}
impl SpritePalette {
    pub fn get(&self, section: &str, id: &str) -> Option<&SpritePreview> {
        self.entries.get(&(section.to_owned(), id.to_owned()))
    }
    pub fn load(install: &Install, catalog: &Catalog) -> Self {
        let mut palette = Self::default();
        for (section, ids) in &catalog.object_types {
            for id in ids {
                palette.entries.insert(
                    (section.clone(), id.clone()),
                    preview(install, catalog, section, id),
                );
            }
        }
        for id in &catalog.overlays {
            let entry = preview(install, catalog, "Overlay", id);
            if let Some(image) = &entry.image {
                for frame in 0..image.info.count.min(256) {
                    if let Ok(decoded) = decode_shp_frame(
                        install,
                        &image.info.name,
                        overlay_palette(install, catalog, id),
                        frame,
                    ) {
                        palette
                            .overlay_frames
                            .insert((id.clone(), frame as u8), Arc::new(decoded));
                    }
                }
            }
            palette
                .entries
                .insert(("Overlay".into(), id.clone()), entry);
        }
        palette.objects = crate::map_sprites::load(install, catalog, &palette);
        palette
    }
}
fn preview(install: &Install, catalog: &Catalog, section: &str, id: &str) -> SpritePreview {
    let rules_image = catalog
        .rules
        .section_or_empty(id)
        .read_name("Image", 256)
        .unwrap_or(id);
    let image = if section == "Animations" {
        catalog.art.resolve_anim_image_id(id)
    } else {
        catalog.art.resolve_effective_image_id(id, rules_image)
    };
    let entry = catalog.art.get(&image);
    let voxel = entry.is_some_and(|e| e.voxel);
    let candidates = if matches!(section, "Terrain" | "Smudge") {
        // Terrain/Smudge InitTheater loads the environment shape first. A
        // same-named SHP may be legacy art with an entirely different palette.
        vec![
            format!("{id}.{}", install.theater.extension),
            format!("{image}.{}", install.theater.extension),
            format!("{image}.shp"),
        ]
    } else if section == "Structures" {
        art_data::building_body_shp_candidates(
            Some(&catalog.art),
            rules_image,
            &image,
            install.theater.extension,
            &install.theater_name,
        )
    } else if section == "Animations" {
        art_data::effect_shp_candidates(
            EffectImageKind::Animation,
            Some(&catalog.art),
            id,
            &image,
            install.theater.extension,
            &install.theater_name,
        )
    } else {
        art_data::object_shp_candidates(
            Some(&catalog.art),
            &image,
            install.theater.extension,
            &install.theater_name,
        )
    };
    let loaded = if voxel {
        voxel_image(install, &image)
    } else {
        let selected = candidates
            .iter()
            .find(|name| install.manager.contains(name));
        selected
            .context(format!("未找到图像：{}", candidates.join(" / ")))
            .and_then(|name| {
                let mut custom_palette = entry
                    .and_then(|_| catalog.art.techno_palette(&image))
                    .and_then(|name| {
                        let file = if name.to_ascii_lowercase().ends_with(".pal") {
                            name.to_owned()
                        } else {
                            format!("{name}{}.pal", install.theater.extension)
                        };
                        install
                            .manager
                            .get_ref(&file)
                            .and_then(|bytes| Palette::from_bytes(bytes).ok())
                    });
                if section == "Animations" && custom_palette.is_none() {
                    custom_palette = install
                        .manager
                        .get_ref("anim.pal")
                        .and_then(|b| Palette::from_bytes(b).ok());
                }
                let pal = custom_palette.as_ref().unwrap_or(match section {
                    "Terrain"
                        if catalog
                            .rules
                            .section_or_empty(id)
                            .read_bool("SpawnsTiberium", false) =>
                    {
                        &install.theater.unit_palette
                    }
                    "Terrain" => &install.theater.iso_palette,
                    "Smudge" => &install.theater.iso_palette,
                    "Overlay" => overlay_palette(install, catalog, id),
                    _ => &install.theater.unit_palette,
                });
                decode_shp(install, name, pal)
            })
    };
    match loaded {
        Ok(image) => SpritePreview {
            detail: format!(
                "{} · 帧 {} / {}{}",
                image.info.name,
                image.info.index,
                image.info.count,
                if voxel { " · 静态体素预览" } else { "" }
            ),
            image: Some(Arc::new(image)),
            voxel,
        },
        Err(error) => SpritePreview {
            image: None,
            detail: format!("{error:#}"),
            voxel,
        },
    }
}
fn overlay_palette<'a>(install: &'a Install, catalog: &Catalog, id: &str) -> &'a Palette {
    let rules = catalog.rules.section_or_empty(id);
    if rules.read_bool("Tiberium", false) {
        &install.theater.tiberium_palette
    } else if rules.read_bool("Wall", false)
        || rules.read_bool("IsVeins", false)
        || rules.read_bool("IsVeinholeMonster", false)
    {
        &install.theater.unit_palette
    } else {
        &install.theater.iso_palette
    }
}
fn decode_shp(install: &Install, name: &str, palette: &Palette) -> Result<DecodedImage> {
    let source = install
        .manager
        .resolve_ref(name)
        .context("图像资源不可读取")?;
    let bytes = source.bytes;
    let count = usize::from(ShpFile::frame_count_from_bytes(bytes)?);
    for index in 0..count {
        let indices = ShpFile::decode_frame(bytes, index)?;
        if !indices.iter().any(|&i| i != 0) {
            continue;
        }
        return decode_shp_frame(install, name, palette, index);
    }
    anyhow::bail!("{name} 没有可见帧")
}
pub(crate) fn decode_shp_frame(
    install: &Install,
    name: &str,
    palette: &Palette,
    index: usize,
) -> Result<DecodedImage> {
    let source = install
        .manager
        .resolve_ref(name)
        .context("图像资源不可读取")?;
    let bytes = source.bytes;
    let indices = ShpFile::decode_frame(bytes, index)?;
    let word = |offset| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let count = usize::from(ShpFile::frame_count_from_bytes(bytes)?);
    let offset = 8 + index * 24;
    let width = u32::from(word(offset + 4));
    let height = u32::from(word(offset + 6));
    ensure!(
        width > 0 && height > 0 && width * height <= 4 * 1024 * 1024,
        "SHP 帧尺寸无效"
    );
    let rgba = indices
        .iter()
        .flat_map(|&i| {
            let c = palette.color(i);
            [c.r, c.g, c.b, if i == 0 { 0 } else { 255 }]
        })
        .collect();
    Ok(DecodedImage {
        indices,
        info: AssetInfo {
            name: name.into(),
            source: source.source_archive.into(),
            palette: "theater/art".into(),
            palette_source: String::new(),
            index,
            count,
            offset: [
                i32::from(word(offset)) - i32::from(word(2)) / 2,
                i32::from(word(offset + 2)) - i32::from(word(4)) / 2,
            ],
            width,
            height,
        },
        rgba,
    })
}
fn voxel_image(install: &Install, image: &str) -> Result<DecodedImage> {
    crate::map_sprites::voxel_frames(install, image, false, false)
        .map(|frames| (*frames[0]).clone())
}
