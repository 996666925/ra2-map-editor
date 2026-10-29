//! Map-sized idle appearances, prepared on the resource worker, never in the UI.
use crate::{
    assets::{AssetInfo, DecodedImage, Install},
    catalog::Catalog,
    sprites::{SpritePalette, decode_shp_frame},
};
use anyhow::{Context, Result, ensure};
use ra2_map_voxel::vxl_raster::{self, VxlRenderParams, VxlSprite};
use std::{collections::HashMap, sync::Arc};
use vera20k_core::{
    assets::{hva_file::HvaFile, pal_file::Palette, vpl_file::VplFile, vxl_file::VxlFile},
    rules::{
        art_data::BuildingAnimKind, infantry_sequence::parse_infantry_sequence_registry,
        ini_parser::IniFile,
    },
};

pub struct ObjectSprite {
    /// One image per native 32-step facing. Shared Arcs for identical SHP frames.
    pub facings: Vec<Arc<DecodedImage>>,
    /// Draw anchor relative to the authored north-west cell's diamond centre.
    pub anchor: [f32; 2],
    /// Ground mask in the same canvas/anchor coordinates as the building body.
    /// Kept separate so shadows never participate in object picking.
    pub shadow: Option<Arc<DecodedImage>>,
}
impl ObjectSprite {
    pub fn image(&self, facing: u8) -> &Arc<DecodedImage> {
        &self.facings[usize::from(vxl_raster::voxel_facing_step(facing)) % self.facings.len()]
    }
}

pub(crate) fn load(
    install: &Install,
    catalog: &Catalog,
    library: &SpritePalette,
) -> HashMap<(String, String), ObjectSprite> {
    let art_ini = install
        .manager
        .get_ref("artmd.ini")
        .and_then(|b| IniFile::from_bytes(b).ok())
        .unwrap_or_else(IniFile::empty);
    let sequences = parse_infantry_sequence_registry(&art_ini);
    let mut result = HashMap::new();
    for (section, types) in &catalog.object_types {
        if section == "Animations" {
            continue;
        }
        for id in types {
            let Some(preview) = library.get(section, id) else {
                continue;
            };
            let rules_image = catalog
                .rules
                .section_or_empty(id)
                .read_name("Image", 256)
                .unwrap_or(id);
            let image_id = catalog.art.resolve_effective_image_id(id, rules_image);
            let art = catalog.art.get(&image_id);
            let mut facings = if preview.voxel {
                match voxel_frames(
                    install,
                    &image_id,
                    catalog
                        .rules
                        .section_or_empty(id)
                        .read_bool("Turret", false),
                    true,
                ) {
                    Ok(frames) => frames,
                    Err(_) => continue,
                }
            } else if let Some(base) = &preview.image {
                let palette = object_palette(install, catalog, &image_id, section, id);
                let mut frames = HashMap::new();
                (0..32).map(|step| {
                    let facing = step * 8;
                    let frame = if section == "Infantry" {
                        art.and_then(|a| a.sequence.as_ref())
                            .and_then(|name| sequences.get(&name.to_ascii_uppercase()))
                            .and_then(|seq| seq.entries.get("READY"))
                            .and_then(|seq| usize::try_from(seq.start_frame + seq.facings * i32::from(vera20k_core::sim::animation::infantry_facing_slot(facing))).ok())
                            .unwrap_or(base.info.index)
                    } else if matches!(section.as_str(), "Units" | "Aircraft") {
                        art.map_or(base.info.index, |a| {
                            let stand = a.standing_frames.unwrap_or(u16::from(a.firing_frames.unwrap_or(0) > 0));
                            let walk = a.walk_frames.unwrap_or(12);
                            let slot = if a.shp_facings == 8 { (((u16::from(facing) + 16) / 32 + 1) & 7) as usize } else {0};
                            if stand > 0 { usize::from(walk) * usize::from(a.shp_facings) + slot * usize::from(stand) }
                            else { slot * usize::from(walk) }
                        })
                    } else { base.info.index };
                    frames.entry(frame).or_insert_with(|| {
                        decode_shp_frame(install, &base.info.name, &palette, frame).ok()
                            .filter(|im| im.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0))
                            .map(Arc::new).unwrap_or_else(|| base.clone())
                    }).clone()
                }).collect()
            } else {
                continue;
            };
            let mut anchor = [0., 0.];
            let mut shadow = None;
            if section == "Smudge" {
                anchor[1] = -15.;
            }
            if section == "Terrain"
                && catalog
                    .rules
                    .section_or_empty(id)
                    .read_bool("SpawnsTiberium", false)
            {
                anchor[1] = -16.;
            }
            if section == "Structures" {
                let palette = object_palette(install, catalog, &image_id, section, id);
                let casts_shadow = !catalog
                    .rules
                    .section_or_empty(id)
                    .read_bool("NoShadow", false);
                let mut shadows = Vec::new();
                if casts_shadow && let Some(mask) = embedded_shadow(install, &facings[0], &palette)
                {
                    shadows.push((Arc::new(mask), [0, 0]));
                }
                // BuildingClass render coordinates subtract 128 leptons on both
                // axes from Location: exactly 15 pixels up from the NW cell
                // centre. GetCoords' foundation centre is for selection/targets,
                // never for centring the SHP canvas (body, bib or shadow).
                anchor[1] = -15.;
                if let Some(art) = art {
                    // Completed building art includes static idle/active/super layers.
                    // Production/special effects require gameplay state and are excluded.
                    let mut layers = Vec::new();
                    if let Some(bib) = &art.bib_shape {
                        let image_id = catalog.art.resolve_anim_image_id(bib);
                        let candidates = vera20k_core::rules::art_data::object_shp_candidates(
                            Some(&catalog.art),
                            &image_id,
                            install.theater.extension,
                            &install.theater_name,
                        );
                        if let Some(bib) = candidates
                            .iter()
                            .find_map(|name| decode_shp_frame(install, name, &palette, 0).ok())
                        {
                            if casts_shadow
                                && let Some(mask) = embedded_shadow(install, &bib, &palette)
                            {
                                shadows.push((Arc::new(mask), [0, 0]));
                            }
                            layers.push((Arc::new(bib), [0, 0]));
                        }
                    }
                    layers.push((facings[0].clone(), [0, 0]));
                    for anim in &art.building_anims {
                        if matches!(
                            anim.kind,
                            BuildingAnimKind::Active
                                | BuildingAnimKind::Idle
                                | BuildingAnimKind::Super
                        ) && let Some(image) = library
                            .get("Animations", &anim.anim_type)
                            .and_then(|p| p.image.as_ref())
                        {
                            // Attached building art uses the parent's palette; ANIM.PAL
                            // produces wrong colours for power plants, radars and flags.
                            if let Ok(mut decoded) = decode_shp_frame(
                                install,
                                &image.info.name,
                                &palette,
                                image.info.index,
                            ) {
                                let offsets = catalog.art.draw_offsets(&anim.anim_type);
                                // AnimClass shadows are explicitly opted in by Shadow=yes;
                                // Flat animations return before the shadow draw. Unlike the
                                // body, the native shadow does not add YDrawOffset.
                                if catalog
                                    .art
                                    .anim_runtime_config(&anim.anim_type)
                                    .is_some_and(|config| config.shadow && !config.flat)
                                    && let Some(mut mask) =
                                        embedded_shadow(install, &decoded, &palette)
                                {
                                    mask.info.offset[0] += offsets.0;
                                    shadows.push((Arc::new(mask), [anim.x, anim.y]));
                                }
                                decoded.info.offset[0] += offsets.0;
                                decoded.info.offset[1] += offsets.1;
                                layers.push((Arc::new(decoded), [anim.x, anim.y]));
                            }
                        }
                    }
                    if layers.len() > 1
                        && let Some(image) = composite(&layers)
                    {
                        facings = vec![Arc::new(image)];
                    }
                }
                // Union masks so overlapping body/bib/idle layers do not create
                // seams or darken a single building's shadow multiple times.
                shadow = composite(&shadows).map(Arc::new);
            }
            if let Some(art) = art {
                anchor[0] += art.x_draw_offset as f32;
                anchor[1] += art.y_draw_offset as f32;
            }
            result.insert(
                (section.clone(), id.clone()),
                ObjectSprite {
                    facings,
                    anchor,
                    shadow,
                },
            );
        }
    }
    result
}

/// Native SHP shadow companion: body frame n + physical frame count / 2.
/// Source index zero is transparent; every other index is a shadow mask,
/// regardless of its palette colour. egui blends in gamma space, so black
/// at half opacity approximates the original destination-halving blitter.
fn embedded_shadow(
    install: &Install,
    body: &DecodedImage,
    palette: &Palette,
) -> Option<DecodedImage> {
    let half = body.info.count / 2;
    if body.info.index >= half {
        return None;
    }
    let mut shadow =
        decode_shp_frame(install, &body.info.name, palette, body.info.index + half).ok()?;
    let mut visible = false;
    for pixel in shadow.rgba.as_chunks_mut::<4>().0 {
        let alpha = if pixel[3] == 0 {
            0
        } else {
            visible = true;
            128
        };
        pixel.copy_from_slice(&[0, 0, 0, alpha]);
    }
    visible.then_some(shadow)
}

fn object_palette(
    install: &Install,
    catalog: &Catalog,
    image: &str,
    section: &str,
    id: &str,
) -> Palette {
    catalog
        .art
        .techno_palette(image)
        .and_then(|name| {
            let name = if name.to_ascii_lowercase().ends_with(".pal") {
                name.to_owned()
            } else {
                format!("{name}{}.pal", install.theater.extension)
            };
            install
                .manager
                .get_ref(&name)
                .and_then(|b| Palette::from_bytes(b).ok())
        })
        .unwrap_or_else(|| {
            if section == "Terrain" {
                if catalog
                    .rules
                    .section_or_empty(id)
                    .read_bool("SpawnsTiberium", false)
                {
                    install.theater.unit_palette.clone()
                } else {
                    install.theater.iso_palette.clone()
                }
            } else if section == "Smudge"
                || catalog.art.get(image).is_some_and(|a| a.terrain_palette)
            {
                install.theater.iso_palette.clone()
            } else {
                install.theater.unit_palette.clone()
            }
        })
}

pub(crate) fn voxel_frames(
    install: &Install,
    image: &str,
    turret: bool,
    all_facings: bool,
) -> Result<Vec<Arc<DecodedImage>>> {
    let name = format!("{image}.vxl");
    let source = install
        .manager
        .resolve_ref(&name)
        .with_context(|| format!("未找到体素 {name}"))?;
    let model = VxlFile::from_bytes(source.bytes)?;
    let hva = install
        .manager
        .get_ref(&format!("{image}.hva"))
        .and_then(|b| HvaFile::from_bytes(b).ok());
    let vpl = install
        .manager
        .get_ref("voxels.vpl")
        .and_then(|b| VplFile::from_bytes(b).ok());
    let parts: Vec<_> = if turret {
        ["tur", "barl"]
            .into_iter()
            .filter_map(|suffix| {
                let vxl =
                    VxlFile::from_bytes(install.manager.get_ref(&format!("{image}{suffix}.vxl"))?)
                        .ok()?;
                let hva = install
                    .manager
                    .get_ref(&format!("{image}{suffix}.hva"))
                    .and_then(|b| HvaFile::from_bytes(b).ok());
                Some((vxl, hva))
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut frames = Vec::new();
    for step in 0..if all_facings { 32 } else { 1 } {
        let params = VxlRenderParams {
            facing: step * 8,
            use_buffer: install.catalog.art.use_buffer(image),
            ..Default::default()
        };
        let body = vxl_raster::render_vxl(&model, hva.as_ref(), &params, vpl.as_ref());
        let mut layers = vec![body];
        let turret_offset = install
            .catalog
            .art
            .get(image)
            .map_or(0, |a| a.turret_offset);
        let (pivot_x, pivot_y) = vxl_raster::turret_pivot_screen_offset_for_slope_state(
            turret_offset,
            params.facing,
            0,
            None,
            1.,
        );
        layers.extend(parts.iter().map(|(model, hva)| {
            let mut part = vxl_raster::render_vxl(model, hva.as_ref(), &params, vpl.as_ref());
            part.offset_x += pivot_x.round();
            part.offset_y += pivot_y.round();
            part
        }));
        let sprite = merge_voxels(&layers);
        ensure!(
            sprite.palette_indices.iter().any(|i| *i != 0),
            "{name} 没有可见体素"
        );
        let rgba = sprite
            .palette_indices
            .iter()
            .flat_map(|&i| {
                let c = install.theater.unit_palette.color(i);
                [c.r, c.g, c.b, if i == 0 { 0 } else { 255 }]
            })
            .collect();
        frames.push(Arc::new(DecodedImage {
            indices: sprite.palette_indices,
            info: AssetInfo {
                name: name.clone(),
                source: source.source_archive.into(),
                palette: "unit/VPL".into(),
                palette_source: String::new(),
                index: usize::from(step),
                count: 32,
                offset: [sprite.offset_x as i32, sprite.offset_y as i32],
                width: sprite.width,
                height: sprite.height,
            },
            rgba,
        }));
    }
    Ok(frames)
}

/// Compose cropped SHP layers in their common canvas coordinate system.
fn composite(layers: &[(Arc<DecodedImage>, [i32; 2])]) -> Option<DecodedImage> {
    let min = std::array::from_fn::<_, 2, _>(|a| {
        layers
            .iter()
            .map(|(i, p)| i.info.offset[a] + p[a])
            .min()
            .unwrap_or(0)
    });
    let max = std::array::from_fn::<_, 2, _>(|a| {
        layers
            .iter()
            .map(|(i, p)| i.info.offset[a] + p[a] + [i.info.width, i.info.height][a] as i32)
            .max()
            .unwrap_or(0)
    });
    let (w, h) = ((max[0] - min[0]) as usize, (max[1] - min[1]) as usize);
    if w == 0 || h == 0 || w.checked_mul(h)? > 4 * 1024 * 1024 {
        return None;
    }
    let mut rgba = vec![0; w * h * 4];
    let mut indices = vec![0; w * h];
    for (image, at) in layers {
        for y in 0..image.info.height as usize {
            for x in 0..image.info.width as usize {
                let src = (y * image.info.width as usize + x) * 4;
                if image.rgba[src + 3] == 0 {
                    continue;
                }
                let dx = (image.info.offset[0] + at[0] - min[0]) as usize + x;
                let dy = (image.info.offset[1] + at[1] - min[1]) as usize + y;
                indices[dy * w + dx] = image.indices.get(src / 4).copied().unwrap_or(0);
                rgba[(dy * w + dx) * 4..(dy * w + dx) * 4 + 4]
                    .copy_from_slice(&image.rgba[src..src + 4]);
            }
        }
    }
    let mut info = layers.first()?.0.info.clone();
    info.offset = min;
    info.width = w as u32;
    info.height = h as u32;
    Some(DecodedImage {
        info,
        rgba,
        indices,
    })
}

/// Merge body and independently loaded gun parts in their model-centred pixel coordinates.
fn merge_voxels(layers: &[VxlSprite]) -> VxlSprite {
    let min_x = layers.iter().map(|s| s.offset_x as i32).min().unwrap_or(0);
    let min_y = layers.iter().map(|s| s.offset_y as i32).min().unwrap_or(0);
    let max_x = layers
        .iter()
        .map(|s| s.offset_x as i32 + s.width as i32)
        .max()
        .unwrap_or(1);
    let max_y = layers
        .iter()
        .map(|s| s.offset_y as i32 + s.height as i32)
        .max()
        .unwrap_or(1);
    let width = (max_x - min_x).max(1) as u32;
    let height = (max_y - min_y).max(1) as u32;
    let mut sprite = VxlSprite {
        width,
        height,
        offset_x: min_x as f32,
        offset_y: min_y as f32,
        palette_indices: vec![0; (width * height) as usize],
        depth: vec![f32::NEG_INFINITY; (width * height) as usize],
    };
    for layer in layers {
        let dx = layer.offset_x as i32 - min_x;
        let dy = layer.offset_y as i32 - min_y;
        for y in 0..layer.height as usize {
            for x in 0..layer.width as usize {
                let src = y * layer.width as usize + x;
                let dst = (y + dy as usize) * width as usize + x + dx as usize;
                if layer.palette_indices[src] != 0 && layer.depth[src] >= sprite.depth[dst] {
                    sprite.palette_indices[dst] = layer.palette_indices[src];
                    sprite.depth[dst] = layer.depth[src];
                }
            }
        }
    }
    sprite
}
