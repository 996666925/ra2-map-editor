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
        art_data::{self, BuildingAnimKind},
        ruleset::RuleSet,
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
    /// Retain the SHP turret separately for scene sorting and pixel depth.
    /// `facings` stays composited for thumbnails, bounds and placement ghosts.
    pub shp_turret: Option<ShpTurretSprite>,
    pub voxel_turret: Option<VoxelTurretSprite>,
}
pub struct VoxelTurretSprite {
    pub body: Arc<DecodedImage>,
    pub facings: Vec<Arc<DecodedImage>>,
    pub z_adjust: i32,
    /// Turret and barrel keep their own native crop/Z-gradient seeds.
    pub parts: Vec<Vec<Arc<DecodedImage>>>,
}
struct GunFrames {
    facings: Vec<Arc<DecodedImage>>,
    parts: Vec<Vec<Arc<DecodedImage>>>,
}
pub struct ShpTurretSprite {
    pub body: Arc<DecodedImage>,
    pub facings: Vec<Arc<DecodedImage>>,
    /// Floor phase of the native pixel projection, baked into preview frames.
    pub pixel_offset: [i32; 2],
    /// Native conversion of the RULES offsets for positioning and GetYSort.
    pub world_offset: [i32; 2],
    /// Native GetYSort adjustment, in leptons rather than screen pixels.
    pub y_sort: i32,
    pub z_adjust: i32,
    pub y_draw_offset: i32,
    pub flat: bool,
    pub remap: bool,
    pub normal_light: bool,
    pub blend: Vec<u8>,
    pub shadows: Vec<Option<Arc<DecodedImage>>>,
    /// Four native truncation phases, decoded once on the resource worker.
    pub previews: Vec<Vec<Arc<DecodedImage>>>,
}
impl ShpTurretSprite {
    pub fn image(&self, facing: u8) -> &Arc<DecodedImage> {
        &self.facings[usize::from(vxl_raster::voxel_facing_step(facing)) % self.facings.len()]
    }
}
impl ObjectSprite {
    pub fn image(&self, facing: u8) -> &Arc<DecodedImage> {
        &self.facings[usize::from(vxl_raster::voxel_facing_step(facing)) % self.facings.len()]
    }
    pub fn image_at(&self, facing: u8, at: [u16; 2]) -> &Arc<DecodedImage> {
        let Some(turret) = &self.shp_turret else {
            return self.image(facing);
        };
        let world_x = i32::from(at[0]) * 256;
        let world_y = i32::from(at[1]) * 256;
        let (x, y) = vera20k_core::util::lepton::project_absolute_lepton_xy(
            world_x + turret.world_offset[0],
            world_y + turret.world_offset[1],
        );
        let offset = [
            x - (i32::from(at[0]) - i32::from(at[1])) * 30,
            y - (i32::from(at[0]) + i32::from(at[1])) * 15,
        ];
        let phase = usize::from(offset[0] > turret.pixel_offset[0])
            + 2 * usize::from(offset[1] > turret.pixel_offset[1]);
        &turret.previews[phase][usize::from(vxl_raster::voxel_facing_step(facing))]
    }
}

pub(crate) fn load(
    install: &Install,
    catalog: &Catalog,
    library: &SpritePalette,
) -> HashMap<(String, String), ObjectSprite> {
    let mut result = HashMap::new();
    let building_rules = catalog.object_types.get("Structures").and_then(|ids| {
        ids.iter()
            .any(|id| {
                let section = catalog.rules.section_or_empty(id);
                section.read_bool("Turret", false)
                    || section.read_bool("TurretAnimIsVoxel", false)
                    || section.read_bool("BarrelAnimIsVoxel", false)
            })
            .then(|| RuleSet::from_ini(&catalog.rules).ok())
            .flatten()
            .map(|mut rules| {
                // Bind the retained physical model slots once for all buildings,
                // including VoxelBarrelFile and the engine's loading fallbacks.
                rules.bind_effect_assets(
                    &install.manager,
                    install.theater.extension,
                    &install.theater_name,
                );
                rules
            })
    });
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
            let image_id = if section == "Infantry" {
                catalog
                    .infantry_image(id)
                    .map(str::to_owned)
                    .unwrap_or_else(|| catalog.art.resolve_effective_image_id(id, rules_image))
            } else {
                catalog.art.resolve_effective_image_id(id, rules_image)
            };
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
                (0..32)
                    .map(|step| {
                        let facing = step * 8;
                        let frame = if section == "Infantry" {
                            catalog
                                .infantry_frame(id, facing)
                                .unwrap_or(base.info.index)
                        } else if matches!(section.as_str(), "Units" | "Aircraft") {
                            art.map_or(base.info.index, |a| {
                                let stand = a
                                    .standing_frames
                                    .unwrap_or(u16::from(a.firing_frames.unwrap_or(0) > 0));
                                let walk = a.walk_frames.unwrap_or(12);
                                let slot = if a.shp_facings == 8 {
                                    (((u16::from(facing) + 16) / 32 + 1) & 7) as usize
                                } else {
                                    0
                                };
                                if stand > 0 {
                                    usize::from(walk) * usize::from(a.shp_facings)
                                        + slot * usize::from(stand)
                                } else {
                                    slot * usize::from(walk)
                                }
                            })
                        } else {
                            base.info.index
                        };
                        frames
                            .entry(frame)
                            .or_insert_with(|| {
                                decode_shp_frame(install, &base.info.name, &palette, frame)
                                    .ok()
                                    .filter(|im| {
                                        im.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
                                    })
                                    .map(Arc::new)
                                    .unwrap_or_else(|| base.clone())
                            })
                            .clone()
                    })
                    .collect()
            } else {
                continue;
            };
            let mut anchor = [0., 0.];
            let mut shadow = None;
            let mut shp_turret = None;
            let mut voxel_turret = None;
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
                if let Some(guns) = building_rules.as_ref().and_then(|rules| {
                    building_gun_frames(install, catalog, rules, id, &image_id)
                        .map(|guns| {
                            voxel_turret = Some(VoxelTurretSprite {
                                body: facings[0].clone(),
                                facings: guns.facings.clone(),
                                z_adjust: rules.object(id).unwrap().turret_anim_z_adjust,
                                parts: guns.parts,
                            });
                            guns.facings
                        })
                        .or_else(|| {
                            shp_turret = building_shp_turret(
                                install,
                                catalog,
                                rules,
                                id,
                                &palette,
                                facings[0].clone(),
                            );
                            shp_turret.as_ref().map(|turret| turret.facings.clone())
                        })
                }) {
                    let body = facings[0].clone();
                    let offset = if shp_turret.is_some() {
                        art.map_or([0, 0], |art| [-art.x_draw_offset, -art.y_draw_offset])
                    } else {
                        [0, 0]
                    };
                    facings = guns
                        .into_iter()
                        .map(|gun| {
                            composite(&[(body.clone(), [0, 0]), (gun, offset)])
                                .map(Arc::new)
                                .unwrap_or_else(|| body.clone())
                        })
                        .collect();
                    if let Some(turret) = &mut shp_turret {
                        let offset =
                            art.map_or([0, 0], |art| [-art.x_draw_offset, -art.y_draw_offset]);
                        turret.previews = (0..4)
                            .map(|phase| {
                                turret
                                    .facings
                                    .iter()
                                    .map(|gun| {
                                        let shift = [
                                            offset[0] + (phase & 1),
                                            offset[1] + ((phase >> 1) & 1),
                                        ];
                                        composite(&[(body.clone(), [0, 0]), (gun.clone(), shift)])
                                            .map(Arc::new)
                                            .unwrap_or_else(|| body.clone())
                                    })
                                    .collect()
                            })
                            .collect();
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
                    shp_turret,
                    voxel_turret,
                },
            );
        }
    }
    result
}

/// Slot 9 is read from RULES, separately from ART's idle/active animations.
/// Use the same facing table as Simulation::aim_building_turret_anim; the
/// healthy turret is held at that frame rather than played as an animation.
fn building_shp_turret(
    install: &Install,
    catalog: &Catalog,
    rules: &RuleSet,
    id: &str,
    palette: &Palette,
    body: Arc<DecodedImage>,
) -> Option<ShpTurretSprite> {
    let object = rules.object(id)?;
    if !object.has_turret {
        return None;
    }
    let config = art_data::shp_turret_anim_config(object)?;
    if !rules.anim_type_names.contains(&config.anim_type) {
        return None;
    }
    let image = catalog.art.resolve_anim_image_id(&config.anim_type);
    let candidates = art_data::effect_shp_candidates(
        art_data::EffectImageKind::Animation,
        Some(&catalog.art),
        &config.anim_type,
        &image,
        install.theater.extension,
        &install.theater_name,
    );
    let name = candidates
        .iter()
        .find(|name| install.manager.contains(name))?;
    let offsets = catalog.art.draw_offsets(&config.anim_type);
    let runtime = catalog.art.anim_runtime_config(&config.anim_type)?;
    let selected_palette = if runtime.should_use_cell_drawer {
        palette.clone()
    } else if runtime.alt_palette {
        install.theater.unit_palette.clone()
    } else {
        Palette::from_bytes(install.manager.get_ref("anim.pal")?).ok()?
    };
    let (dx, dy) = vera20k_core::util::pixel_conversion::PixelConversionBounds::default()
        .offset_to_leptons(config.x, config.y);
    // Positive authored map coordinates use the native integer projection of
    // the retained offset, so ghosts and committed draws share its rounding.
    let project = |n: i32| n.div_euclid(256);
    let pixel_offset = [project((dx - dy) * 30), project((dx + dy) * 15)];
    let mut shadows = Vec::new();
    let mut blend = Vec::new();
    let facings = vera20k_core::util::direction_tables::DRAGON_FRAME_TABLE
        .iter()
        .map(|&frame| {
            let physical = usize::try_from(frame.wrapping_add(runtime.start)).ok()?;
            let mut image = decode_shp_frame(install, name, &selected_palette, physical).ok()?;
            let shadow = (runtime.shadow && !runtime.flat)
                .then(|| embedded_shadow(install, &image, &selected_palette))
                .flatten()
                .map(|mut shadow| {
                    shadow.info.offset[0] += pixel_offset[0] + offsets.0;
                    shadow.info.offset[1] += pixel_offset[1];
                    Arc::new(shadow)
                });
            shadows.push(shadow);
            let flags = vera20k_core::sim::anim_class::anim_translucency_selection(
                vera20k_core::sim::anim_class::AnimTranslucencyInput {
                    base_flags: 0x1600,
                    forced_translucent: false,
                    double_thick: runtime.double_thick,
                    translucency_detail_level: runtime.translucency_detail_level,
                    game_detail_level: 2,
                    translucent_ramp: runtime.translucent,
                    current_frame: frame,
                    end_frame: runtime.loaded_end(image.info.count as i32),
                    explicit_translucency: runtime.translucency,
                    instance_ramp: 0,
                },
            );
            blend.push((flags.flags & 6) as u8);
            image.info.offset[0] += pixel_offset[0] + offsets.0;
            image.info.offset[1] += pixel_offset[1] + offsets.1;
            Some(Arc::new(image))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ShpTurretSprite {
        body,
        facings,
        pixel_offset,
        world_offset: [dx, dy],
        y_sort: config.y_sort,
        z_adjust: config.z_adjust,
        y_draw_offset: offsets.1,
        flat: runtime.flat,
        remap: runtime.should_use_cell_drawer,
        normal_light: runtime.use_normal_light,
        blend,
        shadows,
        previews: Vec::new(),
    })
}

/// BuildingType45FA90 selects separate B8/C0 models; Building43DA80 draws
/// them at TurretAnimX/Y relative to the same anchor as the SHP body.
fn building_gun_frames(
    install: &Install,
    catalog: &Catalog,
    rules: &RuleSet,
    id: &str,
    image: &str,
) -> Option<GunFrames> {
    let model = rules.loaded_voxel_model(id)?;
    let object = rules.object(id)?;
    let vpl = install
        .manager
        .get_ref("voxels.vpl")
        .and_then(|b| VplFile::from_bytes(b).ok());
    let mut renderer = ra2_map_voxel::Renderer::new(vpl.as_ref());
    let params: Vec<_> = (0..32)
        .map(|step| VxlRenderParams {
            facing: step * 8,
            use_buffer: catalog.art.use_buffer(image),
            ..Default::default()
        })
        .collect();
    let turret_offset = catalog.art.get(image).map_or(0, |art| art.turret_offset);
    let mut layers = vec![Vec::new(); 32];
    let has_turret = model.auxiliary().is_some();
    let mut info = None;
    let mut parts = Vec::new();
    for (part, model_part) in [model.auxiliary(), model.barrel()].into_iter().enumerate() {
        let Some(model_part) = model_part else {
            continue;
        };
        let (name, _) = model_part.identity();
        let filename = format!("{name}.vxl");
        let Some(source) = install.manager.resolve_ref(&filename) else {
            continue;
        };
        info.get_or_insert_with(|| AssetInfo {
            name: filename,
            source: source.source_archive.into(),
            palette: "unit/VPL".into(),
            palette_source: String::new(),
            index: 0,
            count: 32,
            offset: [0, 0],
            width: 0,
            height: 0,
        });
        let mut frames = Vec::new();
        for ((mut sprite, params), layers) in renderer
            .render_batch(model_part.vxl(), model_part.hva(), &params, true)
            .into_iter()
            .zip(&params)
            .zip(&mut layers)
        {
            let offsets = if has_turret {
                vxl_raster::building_gun_screen_offsets(turret_offset, params.facing, 0, [0.; 2])
                    [part]
            } else {
                [0.; 2]
            };
            sprite.offset_x += object.turret_anim_x as f32 + offsets[0].round();
            sprite.offset_y += object.turret_anim_y as f32 + offsets[1].round();
            let rgba = sprite
                .palette_indices
                .iter()
                .flat_map(|&i| {
                    let color = install.theater.unit_palette.color(i);
                    [color.r, color.g, color.b, if i == 0 { 0 } else { 255 }]
                })
                .collect();
            frames.push(Arc::new(DecodedImage {
                info: AssetInfo {
                    index: frames.len(),
                    offset: [sprite.offset_x as i32, sprite.offset_y as i32],
                    width: sprite.width,
                    height: sprite.height,
                    ..info.as_ref().unwrap().clone()
                },
                indices: sprite.palette_indices.clone(),
                rgba,
            }));
            layers.push(sprite);
        }
        parts.push(frames);
    }
    let info = info?;
    Some(GunFrames {
        facings: layers
            .into_iter()
            .enumerate()
            .map(|(step, layers)| {
                let sprite = merge_voxels(&layers);
                let rgba = sprite
                    .palette_indices
                    .iter()
                    .flat_map(|&i| {
                        let color = install.theater.unit_palette.color(i);
                        [color.r, color.g, color.b, if i == 0 { 0 } else { 255 }]
                    })
                    .collect();
                Arc::new(DecodedImage {
                    info: AssetInfo {
                        index: step,
                        offset: [sprite.offset_x as i32, sprite.offset_y as i32],
                        width: sprite.width,
                        height: sprite.height,
                        ..info.clone()
                    },
                    indices: sprite.palette_indices,
                    rgba,
                })
            })
            .collect(),
        parts,
    })
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
    let mut renderer = ra2_map_voxel::Renderer::new(vpl.as_ref());
    let params: Vec<_> = (0..if all_facings { 32 } else { 1 })
        .map(|step| VxlRenderParams {
            facing: step * 8,
            use_buffer: install.catalog.art.use_buffer(image),
            ..Default::default()
        })
        .collect();
    let mut layers: Vec<_> = renderer
        .render_batch(&model, hva.as_ref(), &params, !parts.is_empty())
        .into_iter()
        .map(|body| vec![body])
        .collect();
    let turret_offset = install
        .catalog
        .art
        .get(image)
        .map_or(0, |a| a.turret_offset);
    for (model, hva) in &parts {
        let sprites = renderer.render_batch(model, hva.as_ref(), &params, true);
        for ((mut part, params), layers) in sprites.into_iter().zip(&params).zip(&mut layers) {
            let (pivot_x, pivot_y) = vxl_raster::turret_pivot_screen_offset_for_slope_state(
                turret_offset,
                params.facing,
                0,
                None,
                1.,
            );
            part.offset_x += pivot_x.round();
            part.offset_y += pivot_y.round();
            layers.push(part);
        }
    }
    for (step, layers) in layers.into_iter().enumerate() {
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
                index: step,
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
