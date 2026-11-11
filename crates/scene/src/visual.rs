//! Immutable static scene shared by GPU preview, pixel picking and publication.
use crate::map::MapScene;
use anyhow::{Result, ensure};
use ra2_map_assets::assets::{DecodedImage, Install};
use ra2_map_document::document::MapDocument;
use ra2_map_voxel::native_z::{self, ZGradient};
use std::{collections::HashMap, sync::Arc};
use vera20k_core::{
    assets::shp_file::ShpFile,
    map::lighting,
    rules::{color_scheme, house_colors, ini_parser::IniFile},
};

/// World-space sprite anchor shared by committed objects, placement ghosts
/// and selection bounds. Terrain elevation and bridges are applied once.
pub fn object_anchor(
    record: &ra2_map_document::document::Record,
    appearance: &ra2_map_assets::map_sprites::ObjectSprite,
    world_z: i32,
) -> [f32; 2] {
    let at = record.cell().expect("object anchor requires a cell");
    let fields = record.fields();
    let (sub_x, sub_y) = if record.section == "Infantry" {
        vera20k_core::util::lepton::subcell_offset(
            fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        )
    } else {
        (128, 128)
    };
    let (x, y) = vera20k_core::util::lepton::absolute_leptons_to_screen(
        i32::from(at[0]) * 256 + sub_x,
        i32::from(at[1]) * 256 + sub_y,
        world_z,
    );
    let correction = if record.section == "Structures" {
        ra2_map_voxel::building_terrain_correction(world_z / 104)
    } else {
        0
    };
    [
        x + appearance.anchor[0],
        y + appearance.anchor[1] - correction as f32,
    ]
}

pub fn object_on_bridge(record: &ra2_map_document::document::Record) -> bool {
    let fields = record.fields();
    match record.section.as_str() {
        "Infantry" => fields.get(11),
        "Units" => fields.get(10),
        _ => None,
    }
    .is_some_and(|s| s.parse::<i32>().is_ok_and(|v| v != 0))
}

/// Apply SHP crop/VXL canvas offsets and snap in world pixels before zooming.
pub fn sprite_position(point: [f32; 2], image: &DecodedImage) -> egui::Pos2 {
    egui::pos2(
        (point[0] + image.info.offset[0] as f32).round(),
        (point[1] + image.info.offset[1] as f32).round(),
    )
}

#[derive(Clone, Copy, Debug)]
pub struct ViewOptions {
    pub overlays: bool,
    pub objects: bool,
    pub lighting: bool,
    pub remap: bool,
    pub framework: bool,
}
impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            overlays: true,
            objects: true,
            lighting: true,
            remap: true,
            framework: false,
        }
    }
}
#[derive(Clone)]
pub struct Surface {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub original: Vec<u8>,
    /// Alternate terrain material with the same alpha and depth admission.
    pub framework: Vec<u8>,
    pub depth: Vec<u16>,
    /// Native packed RGB565 selector; zero keeps ordinary RGBA drawing.
    pub blend: u8,
}
impl Surface {
    /// Match the GPU's screen-pixel coverage filter for magnified framework art.
    fn framework_sample(
        &self,
        point: egui::Pos2,
        footprint: egui::Vec2,
        nearest: &[u8],
    ) -> [u8; 4] {
        let uv = point - egui::vec2(0.5, 0.5);
        let lo = uv.floor();
        let weight = ((uv - lo - egui::vec2(0.5, 0.5)) / footprint + egui::vec2(0.5, 0.5))
            .clamp(egui::Vec2::ZERO, egui::Vec2::splat(1.));
        let pixel = |dx: i32, dy: i32| {
            let x = (lo.x as i32 + dx).clamp(0, self.size[0] as i32 - 1) as usize;
            let y = (lo.y as i32 + dy).clamp(0, self.size[1] as i32 - 1) as usize;
            let p = (y * self.size[0] as usize + x) * 4;
            let rgba = &self.framework[p..p + 4];
            if rgba[3] == 255 { rgba } else { nearest }
        };
        let [a, b, c, d] = [pixel(0, 0), pixel(1, 0), pixel(0, 1), pixel(1, 1)];
        std::array::from_fn(|i| {
            let top = f32::from(a[i]) * (1. - weight.x) + f32::from(b[i]) * weight.x;
            let bottom = f32::from(c[i]) * (1. - weight.x) + f32::from(d[i]) * weight.x;
            (top * (1. - weight.y) + bottom * weight.y).round() as u8
        })
    }
}
#[derive(Clone, PartialEq)]
pub struct Draw {
    pub surface: usize,
    pub position: [f32; 2],
    pub base: i32,
    pub tint: [f32; 3],
    pub layer: u32,
    pub writes: bool,
    pub owner: Option<u64>,
}
#[derive(Clone)]
struct CachedRecord {
    record: ra2_map_document::document::Record,
    passes: [std::ops::Range<usize>; 3],
}
#[derive(Clone, Default)]
pub struct VisualScene {
    pub surfaces: Vec<Arc<Surface>>,
    pub draws: Vec<Draw>,
    pub bounds: Option<egui::Rect>,
    pub owners: HashMap<u64, (String, String)>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    surface_cache: HashMap<String, usize>,
    terrain_cache: HashMap<(vera20k_core::map::theater::TileKey, u8), usize>,
    light: Option<Arc<lighting::CellLightGrid>>,
    pub(crate) profile: Option<ra2_map_document::lighting::Profile>,
    static_draws: usize,
    // Body and turret own separate pass ranges so other records can sort
    // between them without invalidating incremental reuse.
    records: HashMap<(u64, bool), CachedRecord>,
    pub dirty_chunks: usize,
    pub reused_surfaces: usize,
}
/// Terrain changes retain materials but rebuild heights, lighting and draws.
pub(crate) enum SceneReuse<'a> {
    Records(&'a VisualScene),
    Terrain(&'a VisualScene),
}
impl VisualScene {
    pub fn build(
        scene: &MapScene,
        install: &Install,
        resolved: &vera20k_core::map::resolved_terrain::ResolvedTerrainGrid,
    ) -> Self {
        Self::build_with_profile(
            scene,
            install,
            resolved,
            ra2_map_document::lighting::Profile::Normal,
        )
    }
    pub fn build_with_profile(
        scene: &MapScene,
        install: &Install,
        resolved: &vera20k_core::map::resolved_terrain::ResolvedTerrainGrid,
        profile: ra2_map_document::lighting::Profile,
    ) -> Self {
        Self::build_cached(scene, install, resolved, profile, None, &|| false)
            .expect("uncancelled scene build")
    }
    /// Reuse requires identical visual INI, resources and lighting profile.
    /// Terrain reuse rebuilds lighting and all draws, including unchanged records.
    pub(crate) fn build_cached(
        scene: &MapScene,
        install: &Install,
        resolved: &vera20k_core::map::resolved_terrain::ResolvedTerrainGrid,
        profile: ra2_map_document::lighting::Profile,
        previous: Option<SceneReuse<'_>>,
        cancelled: &dyn Fn() -> bool,
    ) -> anyhow::Result<Self> {
        crate::work::checkpoint(cancelled)?;
        let terrain_changed = matches!(previous, Some(SceneReuse::Terrain(_)));
        let previous = previous.map(|reuse| match reuse {
            SceneReuse::Records(old) | SceneReuse::Terrain(old) => old,
        });
        // Bound retained materials. A changed resource/catalog owner never calls
        // this with an old cache; unused historical materials are periodically dropped.
        let previous =
            previous.filter(|old| old.surfaces.len() < 4096 && old.profile == Some(profile));
        let mut out = Self {
            profile: Some(profile),
            ..Self::default()
        };
        if let Some(old) = previous {
            out.surfaces = old.surfaces.clone();
            out.surface_cache = old.surface_cache.clone();
            out.terrain_cache = old.terrain_cache.clone();
            out.reused_surfaces = out.surfaces.len();
        }
        let ini = IniFile::from_bytes(scene.source.bytes()).unwrap_or_else(|_| IniFile::empty());
        let light = previous
            .filter(|_| !terrain_changed)
            .and_then(|s| s.light.clone())
            .unwrap_or_else(|| {
                Arc::new(
                    lighting::build_cell_light_grid_from_heights_and_units_with_detail(
                        scene.cells.iter().map(|c| ((c.rx, c.ry), c.z)),
                        profile.units(&ini),
                        2,
                    ),
                )
            });
        out.light = Some(light.clone());
        let schemes = color_scheme::parse_color_schemes(&scene.catalog.rules);
        let mut tiles = std::mem::take(&mut out.terrain_cache);
        if let Some(old) = previous.filter(|_| !terrain_changed) {
            out.draws.extend_from_slice(&old.draws[..old.static_draws]);
        }
        if previous.is_none() || terrain_changed {
            for (cell_index, cell) in scene.cells.iter().enumerate() {
                if cell_index % 256 == 0 {
                    crate::work::checkpoint(cancelled)?;
                }
                let Some(image) = scene.image(cell) else {
                    continue;
                };
                let key = MapScene::key(cell);
                let height_tile = scene.palette.framework_height_tile(key);
                let surface = *tiles
                    .entry((key, if height_tile { cell.z } else { 0 }))
                    .or_insert_with(|| {
                        let id = out.surfaces.len();
                        let framework = scene
                            .palette
                            .framework_rgba(key, cell.z)
                            .map_or_else(Vec::new, |rgba| rgba.into_owned());
                        out.surfaces.push(Arc::new(Surface {
                            size: [image.width, image.height],
                            rgba: image.rgba.clone(),
                            original: Vec::new(),
                            framework,
                            depth: image.depth.iter().map(|&n| 32768 + u16::from(n)).collect(),
                            blend: 0,
                        }));
                        id
                    });
                out.draws.push(Draw {
                    surface,
                    position: [
                        cell.screen_x + image.offset_x as f32,
                        cell.screen_y + image.offset_y as f32,
                    ],
                    base: -(i32::from(cell.rx) + i32::from(cell.ry)) * 15 - 30,
                    tint: light.terrain_tile_tint_at((cell.rx, cell.ry)),
                    layer: 0,
                    writes: true,
                    owner: None,
                });
            }
        }
        let Ok(base) = &scene.source.edit_base else {
            out.index_from(previous);
            return Ok(out);
        };
        let doc = MapDocument::new(base.clone());
        let cell_map: HashMap<_, _> = scene.cells.iter().map(|c| ([c.rx, c.ry], c)).collect();
        out.terrain_cache = tiles;
        let mut cache = std::mem::take(&mut out.surface_cache);
        if previous.is_none() || terrain_changed {
            for cell in &scene.cells {
                let (id, state) = doc.overlay([cell.rx, cell.ry]);
                let Some(name) = scene
                    .catalog
                    .overlays
                    .get(usize::from(id))
                    .filter(|_| id != 255)
                else {
                    continue;
                };
                let bridge = vera20k_core::rules::overlay_types::is_high_bridge_index(id)
                    && ["BRIDGE1", "BRIDGEB1", "BRIDGE2", "BRIDGEB2"]
                        .iter()
                        .any(|n| name.eq_ignore_ascii_case(n));
                let frame = if bridge && (state == 0 || state == 9) {
                    state
                        + [0, 1, 2, 3, 3, 2, 1, 0, 2, 3, 0, 1, 1, 0, 3, 2]
                            [usize::from((cell.ry & 3) * 4 + (cell.rx & 3))]
                } else {
                    state
                };
                let Some(image) = scene.sprites.overlay_frames.get(&(name.clone(), frame)) else {
                    continue;
                };
                let flags = scene.catalog.rules.section_or_empty(name);
                let y_bias = if bridge {
                    if (9..=17).contains(&state) {
                        -31.
                    } else {
                        -16.
                    }
                } else if flags.read_bool("Tiberium", false)
                    || flags.read_bool("Wall", false)
                    || flags.read_bool("Crate", false)
                {
                    -12.
                } else {
                    0.
                };
                out.add_sprite(
                    &mut cache,
                    image,
                    [cell.screen_x + 30., cell.screen_y + 15. + y_bias],
                    [cell.rx, cell.ry],
                    cell.z.saturating_add(if bridge { 4 } else { 0 }),
                    1,
                    None,
                    light.overlay_tint_at((cell.rx, cell.ry)),
                    None,
                    if bridge {
                        DepthStyle::Bridge
                    } else {
                        DepthStyle::Flat
                    },
                );
            }
        }
        out.static_draws = out.draws.len();
        let shape = install
            .manager
            .get_ref("buildngz.sha")
            .and_then(|b| ShpFile::from_bytes(b).ok());
        let records = doc.records();
        out.owners = records
            .iter()
            .map(|(&id, r)| (id, (r.section.clone(), r.key.clone())))
            .collect();
        let mut objects: Vec<_> = records
            .iter()
            .filter(|(_, r)| r.section != "Waypoints")
            .flat_map(|(&id, r)| {
                let turret = scene
                    .sprites
                    .objects
                    .get(&(r.section.clone(), r.type_id()))
                    .is_some_and(|sprite| sprite.shp_turret.is_some());
                std::iter::once((id, r, false)).chain(turret.then_some((id, r, true)))
            })
            .collect();
        objects.sort_by_cached_key(|(id, r, turret)| {
            let key = object_sort_key(r, &scene.catalog.rules);
            let key = if *turret {
                let sprite = &scene.sprites.objects[&(r.section.clone(), r.type_id())];
                let turret = sprite.shp_turret.as_ref().unwrap();
                let at = r.cell().unwrap();
                (i32::from(at[0]) + i32::from(at[1]))
                    .wrapping_mul(256)
                    .wrapping_add(turret.world_offset[0])
                    .wrapping_add(turret.world_offset[1])
                    .wrapping_add(turret.y_sort)
            } else {
                key
            };
            (r.section != "Smudge", key, *id, *turret)
        });
        // Flat smudges, shadows, then opaque bodies, all sharing terrain Z.
        for pass in 0..3 {
            for (object_index, &(id, r, turret_part)) in objects.iter().enumerate() {
                if object_index % 128 == 0 {
                    crate::work::checkpoint(cancelled)?;
                }
                let cache_id = (id, turret_part);
                let start = out.draws.len();
                if let Some(cached) = previous
                    .filter(|_| !terrain_changed)
                    .and_then(|old| old.records.get(&cache_id))
                    .filter(|cached| &cached.record == r)
                {
                    out.draws
                        .extend_from_slice(&previous.unwrap().draws[cached.passes[pass].clone()]);
                } else {
                    (|| {
                        let Some(at) = r.cell() else { return };
                        let Some(cell) = cell_map.get(&at) else {
                            return;
                        };
                        let Some(appearance) =
                            scene.sprites.objects.get(&(r.section.clone(), r.type_id()))
                        else {
                            return;
                        };
                        if (pass == 0) != (r.section == "Smudge") && pass != 1 {
                            return;
                        };
                        let f = r.fields();
                        let high = object_on_bridge(r);
                        let slope = resolved.cell(at[0], at[1]).map_or(0, |c| c.slope_type);
                        let world_z = vera20k_core::util::lepton::ground_height_leptons(
                            cell.z,
                            slope,
                            i32::from(at[0]) * 256 + 128,
                            i32::from(at[1]) * 256 + 128,
                        )
                        .unwrap_or(i32::from(cell.z) * 104)
                            + if high { 416 } else { 0 };
                        let level = cell.z.saturating_add(if high { 4 } else { 0 });
                        let point = object_anchor(r, appearance, world_z);
                        if pass == 1 {
                            if turret_part {
                                return;
                            }
                            if let Some(shadow) = &appearance.shadow {
                                out.add_sprite(
                                    &mut cache,
                                    shadow,
                                    [point[0], point[1] + if high { 60. } else { 0. }],
                                    at,
                                    cell.z,
                                    2,
                                    None,
                                    [1.; 3],
                                    None,
                                    DepthStyle::Flat,
                                );
                            }
                            return;
                        }
                        let facing = f
                            .get(if r.section == "Infantry" { 7 } else { 5 })
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        let image = if turret_part {
                            appearance.shp_turret.as_ref().unwrap().image(facing)
                        } else {
                            appearance.shp_turret.as_ref().map_or_else(
                                || {
                                    appearance.voxel_turret.as_ref().map_or_else(
                                        || appearance.image(facing),
                                        |turret| &turret.body,
                                    )
                                },
                                |turret| &turret.body,
                            )
                        };
                        let owner = f.first().map(String::as_str).unwrap_or("Neutral");
                        let color =
                            resolve_house_color(&ini, &scene.catalog.rules, &schemes, owner);
                        let ramp = color.map(|n| house_colors::build_scheme_ramp(schemes[n].hsv));
                        let type_id = r.type_id();
                        let rules_image = scene
                            .catalog
                            .rules
                            .section_or_empty(&type_id)
                            .read_name("Image", 256)
                            .unwrap_or("")
                            .to_owned();
                        let art = scene.catalog.art.resolve_metadata_entry(
                            &type_id,
                            if rules_image.is_empty() {
                                &type_id
                            } else {
                                &rules_image
                            },
                        );
                        let (point, style) = if turret_part {
                            let turret = appearance.shp_turret.as_ref().unwrap();
                            // Replace the preview's raw pixel offset with the
                            // native world projection, independent of body ART.
                            let (x, y) = vera20k_core::util::lepton::absolute_leptons_to_screen(
                                i32::from(at[0]) * 256 + turret.world_offset[0],
                                i32::from(at[1]) * 256 + turret.world_offset[1],
                                world_z,
                            );
                            let correction =
                                ra2_map_voxel::building_terrain_correction(i32::from(cell.z));
                            let lift =
                                vera20k_core::util::native_x87::adjust_for_z_standard(world_z)
                                    + correction;
                            (
                                [
                                    x - turret.pixel_offset[0] as f32,
                                    y - turret.pixel_offset[1] as f32 - correction as f32,
                                ],
                                DepthStyle::Animation {
                                    adjust: turret
                                        .z_adjust
                                        .wrapping_add(turret.y_draw_offset)
                                        .wrapping_sub(if turret.flat { 3 } else { 2 })
                                        .wrapping_sub(lift),
                                    flat: turret.flat,
                                    blend: turret.blend[usize::from(
                                        ra2_map_voxel::vxl_raster::voxel_facing_step(facing),
                                    )],
                                },
                            )
                        } else {
                            let style = if r.section == "Structures" {
                                let foundation =
                                    vera20k_core::rules::foundation::Foundation::Stock(
                                        art.and_then(|a| a.foundation).unwrap_or(0),
                                    )
                                    .dimensions();
                                DepthStyle::Building {
                                    shape: shape.as_ref(),
                                    foundation,
                                    move_point: art.map_or((0, 0), |a| a.z_shape_point_move),
                                    adjust: art.map_or(0, |a| a.normal_z_adjust),
                                }
                            } else if r.section == "Smudge" {
                                DepthStyle::Flat
                            } else if matches!(r.section.as_str(), "Units" | "Infantry") {
                                let rules = scene.catalog.rules.section_or_empty(&type_id);
                                let (adjust, split) = ra2_map_voxel::static_foot_depth(
                                    resolved,
                                    at,
                                    high,
                                    facing,
                                    world_z,
                                    scene.palette.tile_ids.map_or(-1, |ids| ids.water_bridge),
                                    [
                                        rules.read_int("ZFudgeCliff", 10),
                                        rules.read_int("ZFudgeColumn", 5),
                                        rules.read_int("ZFudgeTunnel", 10),
                                        rules.read_int("ZFudgeBridge", 0),
                                    ],
                                    r.section == "Units"
                                        && rules.read_bool("TooBigToFitUnderBridge", false),
                                );
                                DepthStyle::Foot {
                                    adjust,
                                    split: split
                                        && image.info.name.to_ascii_lowercase().ends_with(".vxl"),
                                }
                            } else {
                                DepthStyle::Vertical
                            };
                            (point, style)
                        };
                        let tint = if turret_part
                            && appearance.shp_turret.as_ref().unwrap().normal_light
                        {
                            [1.; 3]
                        } else if r.section == "Smudge" {
                            light.terrain_tile_tint_at((at[0], at[1]))
                        } else if r.section == "Terrain" {
                            light.terrain_object_tint_for_type(
                                (at[0], at[1]),
                                scene
                                    .catalog
                                    .rules
                                    .section_or_empty(&r.type_id())
                                    .read_bool("SpawnsTiberium", false),
                            )
                        } else {
                            light.building_body_tint_at((at[0], at[1]))
                        };
                        out.add_sprite(
                            &mut cache,
                            image,
                            point,
                            at,
                            level,
                            2,
                            Some(id),
                            tint,
                            if turret_part && !appearance.shp_turret.as_ref().unwrap().remap {
                                None
                            } else {
                                ramp.as_ref()
                            },
                            style,
                        );
                        if !turret_part && let Some(turret) = &appearance.voxel_turret {
                            let lift =
                                vera20k_core::util::native_x87::adjust_for_z_standard(world_z)
                                    + ra2_map_voxel::building_terrain_correction(i32::from(cell.z));
                            for part in &turret.parts {
                                let image = &part[usize::from(
                                    ra2_map_voxel::vxl_raster::voxel_facing_step(facing),
                                )];
                                out.add_sprite(
                                    &mut cache,
                                    image,
                                    point,
                                    at,
                                    level,
                                    2,
                                    Some(id),
                                    tint,
                                    ramp.as_ref(),
                                    DepthStyle::Animation {
                                        adjust: turret.z_adjust.wrapping_sub(lift),
                                        flat: false,
                                        blend: 0,
                                    },
                                );
                            }
                        }
                        if turret_part {
                            let turret = appearance.shp_turret.as_ref().unwrap();
                            let step =
                                usize::from(ra2_map_voxel::vxl_raster::voxel_facing_step(facing));
                            if let Some(shadow) = &turret.shadows[step] {
                                let lift =
                                    vera20k_core::util::native_x87::adjust_for_z_standard(world_z)
                                        + ra2_map_voxel::building_terrain_correction(i32::from(
                                            cell.z,
                                        ));
                                out.add_sprite(
                                    &mut cache,
                                    shadow,
                                    point,
                                    at,
                                    level,
                                    2,
                                    None,
                                    [1.; 3],
                                    None,
                                    DepthStyle::Animation {
                                        adjust: -2 - lift,
                                        flat: true,
                                        blend: 1,
                                    },
                                );
                            }
                        }
                    })();
                }
                out.records
                    .entry(cache_id)
                    .or_insert_with(|| CachedRecord {
                        record: r.clone(),
                        passes: std::array::from_fn(|_| 0..0),
                    })
                    .passes[pass] = start..out.draws.len();
            }
        }
        out.surface_cache = cache;
        out.index_from(previous);
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    fn add_sprite(
        &mut self,
        cache: &mut HashMap<String, usize>,
        image: &Arc<DecodedImage>,
        point: [f32; 2],
        at: [u16; 2],
        level: u8,
        layer: u32,
        owner: Option<u64>,
        tint: [f32; 3],
        ramp: Option<&[vera20k_core::assets::pal_file::Color; 16]>,
        style: DepthStyle<'_>,
    ) {
        let point_on_canvas = sprite_position(point, image);
        let position = [point_on_canvas.x, point_on_canvas.y];
        let row = (i32::from(at[0]) + i32::from(at[1])) * 15;
        let top = position[1] as i32 - row;
        let style_key = match style {
            DepthStyle::Flat => (0, (0, 0), (0, 0), 0),
            DepthStyle::Bridge => (3, (0, 0), (0, 0), 0),
            DepthStyle::Vertical => (1, (0, 0), (0, 0), 0),
            DepthStyle::Foot { adjust, split } => (4, (0, 0), (i32::from(split), 0), adjust),
            DepthStyle::Animation {
                adjust,
                flat,
                blend,
            } => (5, (0, 0), (i32::from(flat), i32::from(blend)), adjust),
            DepthStyle::Building {
                foundation,
                move_point,
                adjust,
                ..
            } => (2, foundation, move_point, adjust),
        };
        let key = format!(
            "{}:{top}:{level}:{style_key:?}:{:?}:{:?}",
            Arc::as_ptr(image) as usize,
            ramp.map(|r| r.map(|c| [c.r, c.g, c.b])),
            [point[0] - position[0], point[1] - position[1]]
        );
        let surface = *cache.entry(key).or_insert_with(|| {
            let mut rgba = image.rgba.clone();
            if let Some(ramp) = ramp {
                for (p, &index) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(&image.indices) {
                    if (16..32).contains(&index) {
                        let c = ramp[usize::from(index - 16)];
                        p[..3].copy_from_slice(&[c.r, c.g, c.b]);
                    }
                }
            }
            let height = image.info.height as i32;
            let mut depth = Vec::with_capacity((image.info.width * image.info.height) as usize);
            let gradient = if matches!(
                style,
                DepthStyle::Flat | DepthStyle::Bridge | DepthStyle::Animation { flat: true, .. }
            ) {
                ZGradient::Flat
            } else {
                ZGradient::Vertical
            };
            let adjust = if let DepthStyle::Foot { adjust, .. }
            | DepthStyle::Animation { adjust, .. } = style
            {
                adjust
            } else {
                native_z::ground_anchored_z_adjust(
                    i32::from(level),
                    match style {
                        DepthStyle::Building { adjust, .. } => adjust - 2,
                        _ => -2,
                    },
                )
            };
            let shape = if let DepthStyle::Building {
                shape: Some(shape),
                foundation,
                move_point,
                ..
            } = style
            {
                if foundation.0 <= 7 {
                    Some((
                        shape,
                        native_z::zshape_origin(
                            (point[0].round() as i32, point[1].round() as i32 - row),
                            foundation,
                            move_point,
                        ),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            for y in 0..height {
                for x in 0..image.info.width as i32 {
                    let z = if let Some((shape, origin)) = shape {
                        let sx = position[0] as i32 + x - origin.0;
                        let sy = top + y - origin.1;
                        let byte = shape
                            .frames
                            .first()
                            .and_then(|frame| {
                                let fx = sx - i32::from(frame.frame_x);
                                let fy = sy - i32::from(frame.frame_y);
                                if fx < 0
                                    || fy < 0
                                    || fx >= i32::from(frame.frame_width)
                                    || fy >= i32::from(frame.frame_height)
                                {
                                    None
                                } else {
                                    frame
                                        .pixels
                                        .get((fy * i32::from(frame.frame_width) + fx) as usize)
                                        .copied()
                                }
                            })
                            .unwrap_or(0);
                        32768 - height - top + 1 + adjust - i32::from(native_z::zshape_remap(byte))
                    } else {
                        if let DepthStyle::Foot { split: true, .. } = style {
                            let packed = native_z::pack_voxel_z_gradient(ZGradient::Vertical, true);
                            let region = native_z::voxel_depth_region(
                                [top, height],
                                packed,
                                adjust,
                                top + y,
                                [top, top + height],
                            )
                            .unwrap();
                            native_z::sprite_row_z(
                                region.gradient,
                                region.top,
                                region.height,
                                region.z_adjust,
                                top + y - region.top,
                            )
                        } else {
                            native_z::sprite_row_z(gradient, top, height, adjust, y)
                        }
                    };
                    depth.push(z.clamp(0, 65535) as u16);
                }
            }
            let id = self.surfaces.len();
            self.surfaces.push(Arc::new(Surface {
                size: [image.info.width, image.info.height],
                rgba,
                original: image.rgba.clone(),
                framework: Vec::new(),
                depth,
                blend: if let DepthStyle::Animation { blend, .. } = style {
                    blend
                } else {
                    0
                },
            }));
            id
        });
        self.draws.push(Draw {
            surface,
            position,
            base: -row,
            tint,
            layer,
            writes: matches!(style, DepthStyle::Building { .. } | DepthStyle::Bridge),
            owner,
        });
    }
    fn index_from(&mut self, previous: Option<&Self>) {
        let Some(old) = previous.filter(|old| old.draws.len() == self.draws.len()) else {
            self.dirty_chunks = self.draws.len().div_ceil(256);
            self.index();
            return;
        };
        self.buckets = old.buckets.clone();
        let mut chunks = std::collections::BTreeSet::new();
        let rect = |s: &Self, d: &Draw| {
            egui::Rect::from_min_size(
                d.position.into(),
                s.surfaces[d.surface].size.map(|v| v as f32).into(),
            )
        };
        for i in 0..self.draws.len() {
            if self.draws[i] == old.draws[i] {
                continue;
            }
            chunks.insert(i / 256);
            let a = rect(old, &old.draws[i]);
            let b = rect(self, &self.draws[i]);
            if a == b {
                continue;
            }
            for y in (a.top() / 128.).floor() as i32..=(a.bottom() / 128.).floor() as i32 {
                for x in (a.left() / 128.).floor() as i32..=(a.right() / 128.).floor() as i32 {
                    if let Some(ids) = self.buckets.get_mut(&(x, y)) {
                        ids.retain(|&id| id != i);
                    }
                }
            }
            for y in (b.top() / 128.).floor() as i32..=(b.bottom() / 128.).floor() as i32 {
                for x in (b.left() / 128.).floor() as i32..=(b.right() / 128.).floor() as i32 {
                    let ids = self.buckets.entry((x, y)).or_default();
                    if let Err(at) = ids.binary_search(&i) {
                        ids.insert(at, i);
                    }
                }
            }
        }
        self.dirty_chunks = chunks.len();
        let b = self
            .draws
            .iter()
            .fold(egui::Rect::NOTHING, |b, d| b.union(rect(self, d)));
        self.bounds = b.is_finite().then_some(b);
    }
    fn index(&mut self) {
        let mut bounds = egui::Rect::NOTHING;
        for (i, draw) in self.draws.iter().enumerate() {
            let s = &self.surfaces[draw.surface];
            let rect = egui::Rect::from_min_size(
                draw.position.into(),
                egui::vec2(s.size[0] as f32, s.size[1] as f32),
            );
            bounds = bounds.union(rect);
            for y in (rect.top() / 128.).floor() as i32..=(rect.bottom() / 128.).floor() as i32 {
                for x in (rect.left() / 128.).floor() as i32..=(rect.right() / 128.).floor() as i32
                {
                    self.buckets.entry((x, y)).or_default().push(i);
                }
            }
        }
        self.bounds = bounds.is_finite().then_some(bounds);
    }
    pub fn sample(&self, point: egui::Pos2, options: ViewOptions) -> ([u8; 4], Option<u64>) {
        let mut color = [11, 15, 20, 255];
        let mut depth = 65535;
        let mut owner = None;
        if let Some(draws) = self.buckets.get(&(
            (point.x / 128.).floor() as i32,
            (point.y / 128.).floor() as i32,
        )) {
            for &id in draws {
                let d = &self.draws[id];
                let s = &self.surfaces[d.surface];
                let x = (point.x - d.position[0]).floor() as i32;
                let y = (point.y - d.position[1]).floor() as i32;
                if x < 0 || y < 0 || x >= s.size[0] as i32 || y >= s.size[1] as i32 {
                    continue;
                };
                let p = (y as u32 * s.size[0] + x as u32) as usize;
                if !d.visible(options) {
                    continue;
                }
                let rgba = if options.framework && !s.framework.is_empty() {
                    &s.framework
                } else if !options.remap && !s.original.is_empty() {
                    &s.original
                } else {
                    &s.rgba
                };
                let z = (d.base + i32::from(s.depth[p])).clamp(0, 65535) as u16;
                if rgba[p * 4 + 3] == 0 || if d.layer == 0 { z > depth } else { z >= depth } {
                    continue;
                }
                blend_surface(
                    &mut color,
                    &rgba[p * 4..p * 4 + 4],
                    if options.lighting && !(options.framework && d.layer == 0) {
                        d.tint
                    } else {
                        [1.; 3]
                    },
                    s.blend,
                );
                if d.writes {
                    depth = z
                }
                if d.owner.is_some() {
                    owner = d.owner;
                }
            }
        }
        (color, owner)
    }
    pub fn raster(
        &self,
        rect: egui::Rect,
        size: [u32; 2],
        options: ViewOptions,
    ) -> Result<Vec<u8>> {
        self.raster_controlled(rect, size, options, &|| false)
    }
    pub fn raster_controlled(
        &self,
        rect: egui::Rect,
        size: [u32; 2],
        options: ViewOptions,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>> {
        crate::work::checkpoint(cancelled)?;
        ensure!(
            size[0] > 0
                && size[1] > 0
                && u64::from(size[0]) * u64::from(size[1]) <= 64 * 1024 * 1024,
            "导出图像超过 6400 万像素预算，请降低尺寸"
        );
        let mut pixels = [11, 15, 20, 255].repeat((size[0] * size[1]) as usize);
        let mut depth = vec![65535u16; (size[0] * size[1]) as usize];
        let scale = egui::vec2(
            size[0] as f32 / rect.width(),
            size[1] as f32 / rect.height(),
        );
        for (index, d) in self.draws.iter().enumerate() {
            if index % 128 == 0 {
                crate::work::checkpoint(cancelled)?;
            }
            if !d.visible(options) {
                continue;
            }
            let s = &self.surfaces[d.surface];
            let rgba = if options.framework && !s.framework.is_empty() {
                &s.framework
            } else if !options.remap && !s.original.is_empty() {
                &s.original
            } else {
                &s.rgba
            };
            let left = ((d.position[0] - rect.left()) * scale.x - 0.5)
                .ceil()
                .max(0.) as u32;
            let top = ((d.position[1] - rect.top()) * scale.y - 0.5)
                .ceil()
                .max(0.) as u32;
            let right = (((d.position[0] + s.size[0] as f32 - rect.left()) * scale.x - 0.5)
                .ceil()
                .max(0.) as u32)
                .min(size[0]);
            let bottom = (((d.position[1] + s.size[1] as f32 - rect.top()) * scale.y - 0.5)
                .ceil()
                .max(0.) as u32)
                .min(size[1]);
            for y in top..bottom {
                if y % 64 == 0 {
                    crate::work::checkpoint(cancelled)?;
                }
                let sy = ((y as f32 + 0.5) / scale.y + rect.top() - d.position[1]).floor() as usize;
                for x in left..right {
                    let sx =
                        ((x as f32 + 0.5) / scale.x + rect.left() - d.position[0]).floor() as usize;
                    let src = sy * s.size[0] as usize + sx;
                    if src >= s.depth.len() {
                        continue;
                    }
                    let dst = (y * size[0] + x) as usize;
                    let z = (d.base + i32::from(s.depth[src])).clamp(0, 65535) as u16;
                    if rgba[src * 4 + 3] == 0
                        || if d.layer == 0 {
                            z > depth[dst]
                        } else {
                            z >= depth[dst]
                        }
                    {
                        continue;
                    }
                    let nearest = &rgba[src * 4..src * 4 + 4];
                    let filtered;
                    let color = if options.framework
                        && d.layer == 0
                        && !s.framework.is_empty()
                        && nearest[3] == 255
                        && scale.x > 1.
                        && scale.y > 1.
                    {
                        filtered = s.framework_sample(
                            egui::pos2(
                                (x as f32 + 0.5) / scale.x + rect.left() - d.position[0],
                                (y as f32 + 0.5) / scale.y + rect.top() - d.position[1],
                            ),
                            egui::vec2(1. / scale.x, 1. / scale.y),
                            nearest,
                        );
                        &filtered
                    } else {
                        nearest
                    };
                    blend_surface(
                        &mut pixels[dst * 4..dst * 4 + 4],
                        color,
                        if options.lighting && !(options.framework && d.layer == 0) {
                            d.tint
                        } else {
                            [1.; 3]
                        },
                        s.blend,
                    );
                    if d.writes {
                        depth[dst] = z;
                    }
                }
            }
        }
        Ok(pixels)
    }
}
impl Draw {
    pub fn visible(&self, o: ViewOptions) -> bool {
        self.layer == 0 || self.layer == 1 && o.overlays || self.layer == 2 && o.objects
    }
}
fn blend(dst: &mut [u8], src: &[u8], tint: [f32; 3]) {
    let a = f32::from(src[3]) / 255.;
    for c in 0..3 {
        dst[c] = (f32::from(src[c]) * tint[c])
            .clamp(0., 255.)
            .mul_add(a, f32::from(dst[c]) * (1. - a))
            .round() as u8;
    }
}
fn blend_surface(dst: &mut [u8], src: &[u8], tint: [f32; 3], selector: u8) {
    if selector == 0 {
        blend(dst, src, tint);
        return;
    }
    let source =
        std::array::from_fn(|i| (f32::from(src[i]) * tint[i]).clamp(0., 255.).round() as u8);
    let rgb = ra2_map_voxel::packed_blend([dst[0], dst[1], dst[2]], source, selector);
    dst[..3].copy_from_slice(&rgb);
}
#[derive(Clone, Copy)]
enum DepthStyle<'a> {
    Flat,
    Bridge,
    Vertical,
    Animation {
        adjust: i32,
        flat: bool,
        blend: u8,
    },
    Foot {
        adjust: i32,
        split: bool,
    },
    Building {
        shape: Option<&'a ShpFile>,
        foundation: (u16, u16),
        move_point: (i32, i32),
        adjust: i32,
    },
}

/// The engine's Ground GetYSort keys use absolute leptons, independently of
/// ART draw offsets, height and pixel Z. Building render coordinates precede
/// their voxel/gate adjustment; infantry keeps its authored subcell.
fn object_sort_key(record: &ra2_map_document::document::Record, rules: &IniFile) -> i32 {
    let Some(at) = record.cell() else { return 0 };
    let fields = record.fields();
    let (sub_x, sub_y) = if record.section == "Infantry" {
        vera20k_core::util::lepton::subcell_offset(
            fields
                .get(5)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
        )
    } else {
        (128, 128)
    };
    let location = vera20k_core::sim::components::DriveCoord {
        x: i32::from(at[0]) * 256 + sub_x,
        y: i32::from(at[1]) * 256 + sub_y,
        z: 0,
    };
    let (location, adjust) = if record.section == "Structures" {
        let section = rules.section_or_empty(&record.type_id());
        vera20k_core::sim::movement::ground_pose::building_render_order_parts(
            location,
            section.read_bool("TurretAnimIsVoxel", false),
            section.read_bool("Gate", false),
        )
    } else {
        (location, 0)
    };
    location.x.wrapping_add(location.y).wrapping_add(adjust)
}
pub fn resolve_house_color(
    ini: &IniFile,
    rules: &IniFile,
    schemes: &[color_scheme::ColorSchemeEntry],
    owner: &str,
) -> Option<usize> {
    if owner.eq_ignore_ascii_case("Neutral") || owner.eq_ignore_ascii_case("Special") {
        return None;
    }
    let house = ini.section_or_empty(owner);
    let country = house.read_name("Country", 128).unwrap_or(owner);
    house
        .read_name("Color", 128)
        .or_else(|| rules.section_or_empty(country).read_name("Color", 128))
        .and_then(|name| color_scheme::scheme_entry_by_name(schemes, name))
}

#[cfg(test)]
#[path = "visual_tests.rs"]
mod tests;
