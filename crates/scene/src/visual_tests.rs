use super::*;

fn turret_scene(
    fixture: &ra2_map_assets::test_support::Fixture,
    y_sort: i32,
    z_adjust: i32,
    level: u8,
    neighbor: Option<(&str, &str)>,
) -> (Install, crate::map::MapScene, u64) {
    let rules_path = fixture.0.join("rulesmd.ini");
    let rules = std::fs::read_to_string(&rules_path)
        .unwrap()
        .replace("TurretAnimYSort=512", &format!("TurretAnimYSort={y_sort}"))
        .replace(
            "TurretAnimZAdjust=-30",
            &format!("TurretAnimZAdjust={z_adjust}"),
        );
    std::fs::write(rules_path, rules).unwrap();
    let bytes = ra2_map_document::new_map::NewMapOptions {
        width: if level > 20 { 40 } else { 20 },
        height: if level > 20 { 40 } else { 20 },
        level,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let (source, _) =
        ra2_map_document::source::SourceMap::parse("turret.map".into(), "test".into(), bytes)
            .unwrap();
    let mut doc = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
    let at = if level > 20 { [40, 40] } else { [20, 20] };
    doc.add_record("Structures", at, "Neutral", "GUN").unwrap();
    if let Some((section, kind)) = neighbor {
        doc.add_record(section, at, "Neutral", kind).unwrap();
    }
    doc.commit("place");
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "turret.map",
        "test".into(),
        doc.snapshot().bytes().unwrap(),
        std::time::Instant::now(),
    )
    .unwrap();
    let reopened = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    let id = reopened
        .records()
        .into_iter()
        .find(|(_, record)| record.type_id() == "GUN")
        .unwrap()
        .0;
    let sprite = &scene.sprites.objects[&("Structures".into(), "GUN".into())];
    assert!(sprite.shp_turret.is_some() || sprite.voxel_turret.is_some());
    (install, scene, id)
}

fn unlit() -> ViewOptions {
    ViewOptions {
        lighting: false,
        remap: false,
        ..Default::default()
    }
}

#[test]
fn packed_turret_blends_match_the_pinned_native_blitter_controls() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../vendor/vera20k/tools/procedural_drawing_oracle/translucent_blitter_a.json"
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus["cases"].as_array().unwrap() {
        if row["profile"] != "plain1"
            || row["route"] != "shp"
            || row["displacement"] != 0
            || row["index"] == 0
            || row["prior_depths"][0].as_u64().unwrap_or(0) <= 4096
        {
            continue;
        }
        let selector = row["selector_bits"].as_u64().unwrap() as u8;
        if ![2, 4, 6].contains(&selector) {
            continue;
        }
        let rgb = std::array::from_fn(|i| row["rgb"][i].as_u64().unwrap() as u8);
        let destination =
            ra2_map_voxel::preview_unpack(row["prior_colors"][0].as_u64().unwrap() as u16);
        let mixed = ra2_map_voxel::packed_blend(destination, rgb, selector);
        assert_eq!(
            u64::from(ra2_map_voxel::preview_pack(mixed)),
            row["colors"][0].as_u64().unwrap()
        );
        checked += 1;
    }
    assert!(
        checked >= 9,
        "must cover the three transparency stages and background colours"
    );
}

#[test]
fn voxel_turret_keeps_independent_z_seed_for_every_facing_and_height() {
    for level in [0, 4, 36] {
        let mut previous = None;
        for adjust in [-60, 60] {
            let fixture = ra2_map_assets::test_support::Fixture::with_voxel_turret();
            let (_, scene, id) = turret_scene(&fixture, 512, adjust, level, None);
            let draws: Vec<_> = scene
                .visual
                .draws
                .iter()
                .filter(|d| d.owner == Some(id))
                .collect();
            assert_eq!(draws.len(), 2);
            assert!(draws[0].writes);
            assert!(!draws[1].writes);
            let image = &scene.visual.surfaces[draws[1].surface];
            let row = if level > 20 { 1200 } else { 600 };
            let top = draws[1].position[1] as i32 - row;
            let expected = native_z::sprite_row_z(
                ZGradient::Vertical,
                top,
                image.size[1] as i32,
                adjust - i32::from(level) * 15,
                0,
            );
            assert_eq!(image.depth[0] as i32, expected);
            if let Some(depth) = previous {
                assert_ne!(image.depth, depth);
            }
            previous = Some(image.depth.clone());
            let appearance = &scene.sprites.objects[&("Structures".into(), "GUN".into())];
            assert_eq!(appearance.voxel_turret.as_ref().unwrap().facings.len(), 32);
        }
    }
}

#[test]
fn turret_preview_pixel_and_bounds_match_native_projection_on_both_sides_of_map() {
    for offset in [[1, -10], [-1, -10], [7, -10], [-13, 11]] {
        let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
        let path = fixture.0.join("rulesmd.ini");
        let rules = std::fs::read_to_string(&path).unwrap().replace(
            "TurretAnimYSort=512",
            &format!(
                "TurretAnimX={}\nTurretAnimY={}\nTurretAnimYSort=512",
                offset[0], offset[1]
            ),
        );
        std::fs::write(path, rules).unwrap();
        let path = fixture.0.join("artmd.ini");
        std::fs::write(
            &path,
            std::fs::read_to_string(&path)
                .unwrap()
                .replace("NormalZAdjust=60", "NormalZAdjust=60\nXDrawOffset=50"),
        )
        .unwrap();
        let install = fixture.install();
        let catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
        let sprites = ra2_map_assets::sprites::SpritePalette::load(&install, &catalog);
        let appearance = &sprites.objects[&("Structures".into(), "GUN".into())];
        let turret = appearance.shp_turret.as_ref().unwrap();
        for at in [[20, 20], [21, 20], [20, 21]] {
            let image = appearance.image_at(0, at);
            let p = image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .position(|p| p == &[200, 0, 0, 255])
                .unwrap();
            let (x, y) = vera20k_core::util::lepton::absolute_leptons_to_screen(
                i32::from(at[0]) * 256,
                i32::from(at[1]) * 256,
                0,
            );
            let preview = sprite_position([x + 50., y], image);
            let (tx, ty) = vera20k_core::util::lepton::absolute_leptons_to_screen(
                i32::from(at[0]) * 256 + turret.world_offset[0],
                i32::from(at[1]) * 256 + turret.world_offset[1],
                0,
            );
            assert_eq!(
                [
                    preview.x + (p % image.info.width as usize) as f32,
                    preview.y + (p / image.info.width as usize) as f32
                ],
                [tx - 1., ty],
                "offset={offset:?}, at={at:?}"
            );
        }
    }
}

#[test]
fn packed_turret_materials_and_shadows_keep_picking_and_export_in_sync() {
    for (translucency, selector) in [(25, 2), (50, 4), (75, 6)] {
        let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
        let path = fixture.0.join("artmd.ini");
        let art=std::fs::read_to_string(&path).unwrap().replace("LoopEnd=32",
            &format!("LoopEnd=32\nShouldUseCellDrawer=no\nTranslucency={translucency}\nUseNormalLight=yes\nShadow=yes"));
        std::fs::write(path, art).unwrap();
        let mut palette = vec![0; 768];
        palette[9..12].copy_from_slice(&[0, 0, 50]);
        std::fs::write(fixture.0.join("anim.pal"), palette).unwrap();
        let (_, scene, id) = turret_scene(&fixture, 512, -30, 0, None);
        let draw = scene
            .visual
            .draws
            .iter()
            .find(|d| d.owner == Some(id) && !d.writes)
            .unwrap();
        assert_eq!(scene.visual.surfaces[draw.surface].blend, selector);
        assert_eq!(draw.tint, [1.; 3]);
        let rgb = ra2_map_voxel::packed_blend([60, 80, 100], [0, 0, 200], selector);
        let expected = [rgb[0], rgb[1], rgb[2], 255];
        assert_eq!(
            scene.visual.sample(egui::pos2(-0.5, 615.5), unlit()),
            (expected, Some(id))
        );
        let rect = egui::Rect::from_min_size(egui::pos2(-1., 615.), egui::vec2(2., 1.));
        assert_eq!(
            &scene.visual.raster(rect, [2, 1], unlit()).unwrap()[..4],
            &expected
        );
    }
    // A physical shadow at a different crop is independently visible, while
    // Flat and Shadow=no never manufacture one. It does not own picking.
    for shadow in [true, false] {
        let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
        let path = fixture.0.join("artmd.ini");
        std::fs::write(
            &path,
            std::fs::read_to_string(&path).unwrap().replace(
                "LoopEnd=32",
                &format!("LoopEnd=32\nShadow={}", if shadow { "yes" } else { "no" }),
            ),
        )
        .unwrap();
        let path = fixture.0.join("guntur.shp");
        let mut bytes = std::fs::read(&path).unwrap();
        for frame in 32..64usize {
            let header = 8 + 24 * frame;
            bytes[header..header + 2].copy_from_slice(&2u16.to_le_bytes());
            bytes[8 + 64 * 24 + frame * 2] = 3;
        }
        std::fs::write(path, bytes).unwrap();
        let (_, scene, id) = turret_scene(&fixture, 512, -30, 0, None);
        let masks: Vec<_> = scene
            .visual
            .draws
            .iter()
            .filter(|d| scene.visual.surfaces[d.surface].blend == 1)
            .collect();
        assert_eq!(masks.len(), usize::from(shadow));
        if shadow {
            assert_eq!(masks[0].owner, None);
            assert_eq!(
                scene.visual.sample(egui::pos2(0.5, 615.5), unlit()).1,
                Some(id)
            );
        }
    }
}

#[test]
fn shp_turret_y_sort_changes_painter_order_without_changing_pixel_depth() {
    for (neighbor, behind, front) in [(None, -1, 1), (Some(("Units", "TANK")), 255, 257)] {
        let mut depths = Vec::new();
        for (y_sort, expected) in [
            (
                behind,
                if neighbor.is_some() {
                    [0, 200, 0, 255]
                } else {
                    [60, 80, 100, 255]
                },
            ),
            (front, [200, 0, 0, 255]),
        ] {
            let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
            let (_, scene, gun_id) = turret_scene(&fixture, y_sort, -30, 0, neighbor);
            let draws: Vec<_> = scene
                .visual
                .draws
                .iter()
                .filter(|draw| draw.owner == Some(gun_id))
                .collect();
            assert_eq!(draws.len(), 2);
            let turret = draws.iter().find(|draw| !draw.writes).unwrap();
            depths.push(scene.visual.surfaces[turret.surface].depth.clone());
            let point = egui::pos2(-0.5, 615.5);
            let sample = scene.visual.sample(point, unlit());
            assert_eq!(sample.0, expected, "YSort={y_sort}, neighbor={neighbor:?}");
            if y_sort == front {
                assert_eq!(sample.1, Some(gun_id));
            }
            let rect = egui::Rect::from_min_size(egui::pos2(-1., 615.), egui::vec2(2., 1.));
            let raster = scene.visual.raster(rect, [2, 1], unlit()).unwrap();
            assert_eq!(&raster[..4], &expected);
            assert_eq!(
                &raster[4..],
                &[60, 80, 100, 255],
                "transparent turret pixel keeps the body"
            );
        }
        assert_eq!(depths[0], depths[1], "YSort must not be added to pixel Z");
    }
}

#[test]
fn shp_turret_z_adjust_tests_body_and_neighbor_depth_at_each_height() {
    for level in [0, 4, 12, 36] {
        for (neighbor, z_adjust, expected) in [
            (None, 100, [60, 80, 100, 255]),
            (None, -30, [200, 0, 0, 255]),
            (Some(("Structures", "BLOCK")), 30, [0, 0, 200, 255]),
            (Some(("Structures", "BLOCK")), -30, [200, 0, 0, 255]),
        ] {
            let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
            let (_, scene, gun_id) = turret_scene(&fixture, 512, z_adjust, level, neighbor);
            let turret = scene
                .visual
                .draws
                .iter()
                .find(|draw| draw.owner == Some(gun_id) && !draw.writes)
                .unwrap();
            let surface = &scene.visual.surfaces[turret.surface];
            let row = if level > 20 { 1200 } else { 600 };
            let top = turret.position[1] as i32 - row;
            let expected_z = native_z::sprite_row_z(
                ZGradient::Vertical,
                top,
                1,
                z_adjust - 2 - i32::from(level) * 15,
                0,
            );
            assert_eq!(i32::from(surface.depth[0]), expected_z);
            let point = egui::pos2(-0.5, row as f32 + 15.5 - f32::from(level) * 15.);
            assert_eq!(
                scene.visual.sample(point, unlit()).0,
                expected,
                "level={level}, ZAdjust={z_adjust}, neighbor={neighbor:?}"
            );
        }
    }
}

#[test]
fn shp_turret_offsets_and_flat_depth_use_the_animation_coordinate() {
    let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
    let path = fixture.0.join("rulesmd.ini");
    let rules = std::fs::read_to_string(&path).unwrap().replace(
        "TurretAnimYSort=512",
        "TurretAnimX=7\nTurretAnimY=-10\nTurretAnimYSort=512",
    );
    std::fs::write(path, rules).unwrap();
    let path = fixture.0.join("artmd.ini");
    let art = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            "NormalZAdjust=60",
            "NormalZAdjust=60\nXDrawOffset=11\nYDrawOffset=7",
        )
        .replace(
            "LoopEnd=32",
            "LoopEnd=32\nXDrawOffset=3\nYDrawOffset=4\nFlat=yes",
        );
    std::fs::write(path, art).unwrap();
    let (_, scene, id) = turret_scene(&fixture, 512, -30, 4, None);
    let draws: Vec<_> = scene
        .visual
        .draws
        .iter()
        .filter(|draw| draw.owner == Some(id))
        .collect();
    let body = draws.iter().find(|draw| draw.writes).unwrap();
    let turret = draws.iter().find(|draw| !draw.writes).unwrap();
    assert_eq!(body.position, [10., 562.]);
    assert_eq!(turret.position, [9., 549.]);
    assert_eq!(
        i32::from(scene.visual.surfaces[turret.surface].depth[0]),
        native_z::sprite_row_z(ZGradient::Flat, -51, 1, -30 + 4 - 3 - 60, 0),
    );
    let appearance = &scene.sprites.objects[&("Structures".into(), "GUN".into())];
    let preview = appearance.image(0);
    let point = object_anchor(
        &MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone()).records()[&id],
        appearance,
        416,
    );
    assert_eq!(sprite_position(point, preview), egui::pos2(9., 549.));
}

#[test]
fn shp_turret_record_reuse_matches_full_reload_through_move_and_history() {
    use ra2_map_document::document::{Target, Value};
    let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
    let (mut install, mut scene, _) = turret_scene(&fixture, 257, -30, 4, Some(("Units", "TANK")));
    let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    let gun_id = doc
        .records()
        .into_iter()
        .find(|(_, record)| record.type_id() == "GUN")
        .unwrap()
        .0;
    let rect = egui::Rect::from_min_size(egui::pos2(-4., 550.), egui::vec2(70., 45.));
    for step in 0..3 {
        let before = doc.snapshot();
        match step {
            0 => {
                let record = doc.records()[&gun_id].moved([21, 20]).unwrap();
                doc.set(Target::Record(gun_id), Some(Value::Record(record)))
                    .unwrap();
                doc.commit("move turret building");
            }
            1 => {
                doc.undo();
            }
            _ => {
                doc.redo();
            }
        }
        let updated = crate::work::update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            ra2_map_document::lighting::Profile::Normal,
            &|| false,
        )
        .unwrap()
        .unwrap();
        let full = crate::map::load_scene_bytes(
            &mut install,
            "turret.map",
            "full".into(),
            doc.snapshot().bytes().unwrap(),
            std::time::Instant::now(),
        )
        .unwrap();
        assert_eq!(
            updated.visual.raster(rect, [140, 90], unlit()).unwrap(),
            full.visual.raster(rect, [140, 90], unlit()).unwrap(),
            "step {step}"
        );
        for point in [egui::pos2(-0.5, 555.5), egui::pos2(29.5, 570.5)] {
            assert_eq!(
                updated.visual.sample(point, unlit()),
                full.visual.sample(point, unlit()),
                "step {step}"
            );
        }
        assert!(updated.visual.reused_surfaces > 0);
        scene = updated;
    }
}

#[test]
fn placed_building_uses_render_location_and_keeps_it_after_save_reopen() {
    use ra2_map_assets::test_support::Fixture;
    use ra2_map_document::new_map::NewMapOptions;
    use std::time::Instant;
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[BuildingTypes]\n0=GI\n[InfantryTypes]\n0=SOLDIER\n[SOLDIER]\nImage=GI\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("artmd.ini"),
        b"[GI]\nFoundation=3x2\nXDrawOffset=3\nYDrawOffset=4\n",
    )
    .unwrap();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    for level in [0, 4] {
        let scene = crate::map::load_scene_bytes(
            &mut install,
            "anchor.map",
            "test".into(),
            NewMapOptions {
                width: 20,
                height: 20,
                level,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        doc.add_record("Structures", [20, 20], "Neutral", "GI")
            .unwrap();
        doc.add_record("Infantry", [22, 22], "Neutral", "SOLDIER")
            .unwrap();
        doc.commit("place");
        let saved = doc.snapshot().bytes().unwrap();
        let scene = crate::map::load_scene_bytes(
            &mut install,
            "anchor.map",
            "test".into(),
            saved.clone(),
            Instant::now(),
        )
        .unwrap();
        let reopened = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        assert_eq!(reopened.snapshot().bytes().unwrap(), saved);
        for (id, record) in reopened.records() {
            if record.section == "Waypoints" {
                continue;
            }
            let at = record.cell().unwrap();
            // Building 459EF0 subtracts half a cell on both axes from
            // Location (NW cell centre). Foot objects retain their centre.
            let sub = if record.section == "Structures" {
                0
            } else {
                128
            };
            let (x, y) = vera20k_core::util::lepton::absolute_leptons_to_screen(
                i32::from(at[0]) * 256 + sub,
                i32::from(at[1]) * 256 + sub,
                i32::from(level) * 104,
            );
            let image = scene.sprites.objects[&(record.section.clone(), record.type_id())].image(0);
            let draw = scene
                .visual
                .draws
                .iter()
                .find(|d| d.owner == Some(id))
                .unwrap();
            assert_eq!(
                draw.position,
                [
                    (x + 3. + image.info.offset[0] as f32).round(),
                    (y + 4. + image.info.offset[1] as f32).round(),
                ],
                "{} at level {level}",
                record.section
            );
            assert_eq!(
                at,
                if record.section == "Structures" {
                    [20, 20]
                } else {
                    [22, 22]
                }
            );
        }
    }
}

fn fixture() -> VisualScene {
    let mut s = VisualScene::default();
    s.surfaces.push(std::sync::Arc::new(Surface {
        size: [2, 1],
        rgba: vec![50, 100, 150, 255, 60, 120, 180, 255],
        original: Vec::new(),
        framework: vec![220, 180, 200, 255, 200, 160, 180, 255],
        depth: vec![200, 200],
        blend: 0,
    }));
    s.surfaces.push(std::sync::Arc::new(Surface {
        size: [2, 1],
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        original: vec![0, 0, 255, 255, 0, 0, 255, 255],
        framework: Vec::new(),
        depth: vec![201, 199],
        blend: 0,
    }));
    s.draws.push(Draw {
        surface: 0,
        position: [0., 0.],
        base: 0,
        tint: [1.; 3],
        layer: 0,
        writes: true,
        owner: None,
    });
    s.draws.push(Draw {
        surface: 1,
        position: [0., 0.],
        base: 0,
        tint: [0.5; 3],
        layer: 2,
        writes: false,
        owner: Some(9),
    });
    s.index();
    s
}

#[test]
fn framework_material_keeps_depth_object_ownership_and_cpu_sampling_in_sync() {
    let s = fixture();
    let framework = ViewOptions {
        framework: true,
        ..Default::default()
    };
    assert_ne!(
        s.sample(egui::pos2(0.5, 0.5), framework).0,
        s.sample(egui::pos2(0.5, 0.5), ViewOptions::default()).0
    );
    for x in [0.5, 1.5] {
        let point = egui::pos2(x, 0.5);
        assert_eq!(
            s.sample(point, framework).1,
            s.sample(point, ViewOptions::default()).1
        );
        let image = s
            .raster(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2., 1.)),
                [2, 1],
                framework,
            )
            .unwrap();
        assert_eq!(
            &image[x.floor() as usize * 4..x.floor() as usize * 4 + 4],
            &s.sample(point, framework).0
        );
    }
}

#[test]
fn magnified_framework_strokes_keep_their_coverage_across_fractional_pan_and_zoom() {
    // Two-pixel strokes, like the generated height digits, separated by clear space.
    let rgba: Vec<u8> = (0..64)
        .flat_map(|x| {
            if (4..6).contains(&x) {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            }
        })
        .collect();
    let mut s = VisualScene::default();
    s.surfaces.push(std::sync::Arc::new(Surface {
        size: [64, 1],
        rgba: rgba.clone(),
        original: Vec::new(),
        framework: rgba,
        depth: vec![200; 64],
        blend: 0,
    }));
    s.draws.push(Draw {
        surface: 0,
        position: [0., 0.],
        base: 0,
        tint: [1.; 3],
        layer: 0,
        writes: true,
        owner: None,
    });
    for zoom in [1.25, 1.5, 2.45, 3.1, 5.] {
        for phase in [0., 0.13, 0.5, 0.91] {
            let rect = egui::Rect::from_min_size(
                egui::pos2(phase / zoom, 0.),
                egui::vec2(32. / zoom, 1. / zoom),
            );
            let filtered = s
                .raster(
                    rect,
                    [32, 1],
                    ViewOptions {
                        framework: true,
                        ..Default::default()
                    },
                )
                .unwrap();
            let width: f32 = filtered
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| (255 - p[0]) as f32 / 255.)
                .sum();
            assert!(
                (width - 2. * zoom).abs() < 0.01,
                "zoom={zoom}, phase={phase}, width={width}"
            );
        }
    }
    // At native scale and when reduced, framework still uses the original texels.
    for zoom in [0.5, 1.] {
        let rect =
            egui::Rect::from_min_size(egui::pos2(0.13, 0.), egui::vec2(16. / zoom, 1. / zoom));
        assert_eq!(
            s.raster(
                rect,
                [16, 1],
                ViewOptions {
                    framework: true,
                    ..Default::default()
                }
            )
            .unwrap(),
            s.raster(rect, [16, 1], Default::default()).unwrap()
        );
    }
}

#[test]
fn incremental_bucket_moves_preserve_painter_order_and_full_raster_pixels() {
    let old = fixture();
    let mut updated = old.clone();
    updated.draws[1].position = [130., 0.];
    updated.index_from(Some(&old));
    let mut rebuilt = updated.clone();
    rebuilt.buckets.clear();
    rebuilt.index();
    let rect = egui::Rect::from_min_size(egui::pos2(-1., -1.), egui::vec2(134., 4.));
    assert_eq!(
        updated.raster(rect, [134, 4], Default::default()).unwrap(),
        rebuilt.raster(rect, [134, 4], Default::default()).unwrap()
    );
    for x in [-0.5, 0.5, 1.5, 129.5, 130.5, 131.5] {
        assert_eq!(
            updated.sample(egui::pos2(x, 0.5), Default::default()),
            rebuilt.sample(egui::pos2(x, 0.5), Default::default())
        );
    }
    let count = std::cell::Cell::new(0);
    assert!(
        updated
            .raster_controlled(rect, [134, 4], Default::default(), &|| {
                count.set(count.get() + 1);
                count.get() > 1
            })
            .is_err()
    );
    assert!(count.get() > 1);
}
#[test]
fn shared_depth_hides_pixels_behind_cliffs_and_picking_agrees_with_publication() {
    let s = fixture();
    let o = ViewOptions::default();
    assert_eq!(
        s.sample(egui::pos2(0.5, 0.5), o),
        ([50, 100, 150, 255], None)
    );
    assert_eq!(
        s.sample(egui::pos2(1.5, 0.5), o),
        ([0, 128, 0, 255], Some(9))
    );
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2., 1.));
    let rgba = s.raster(rect, [2, 1], o).unwrap();
    assert_eq!(rgba, [50, 100, 150, 255, 0, 128, 0, 255]);
}
#[test]
fn layer_lighting_remap_toggles_and_transparent_holes_preserve_ground() {
    let mut s = fixture();
    let p = egui::pos2(1.5, 0.5);
    assert_eq!(
        s.sample(
            p,
            ViewOptions {
                objects: false,
                ..Default::default()
            }
        )
        .0,
        [60, 120, 180, 255]
    );
    assert_eq!(
        s.sample(
            p,
            ViewOptions {
                remap: false,
                lighting: false,
                ..Default::default()
            }
        )
        .0,
        [0, 0, 255, 255]
    );
    std::sync::Arc::make_mut(&mut s.surfaces[1]).rgba[7] = 0;
    assert_eq!(s.sample(p, ViewOptions::default()).1, None);
    assert!(
        s.raster(egui::Rect::EVERYTHING, [9000, 9000], ViewOptions::default())
            .is_err()
    );
}
#[test]
fn owner_color_resolves_house_override_country_and_neutral() {
    let schemes = color_scheme::parse_color_schemes(&IniFile::from_str(
        "[Colors]\nRed=0,255,255\nBlue=160,255,255\n",
    ));
    let rules = IniFile::from_str("[Russians]\nColor=Red\n");
    let ini =
        IniFile::from_str("[Custom]\nCountry=Russians\nColor=Blue\n[Other]\nCountry=Russians\n");
    assert_eq!(
        resolve_house_color(&ini, &rules, &schemes, "Custom"),
        Some(1)
    );
    assert_eq!(
        resolve_house_color(&ini, &rules, &schemes, "Other"),
        Some(0)
    );
    assert_eq!(resolve_house_color(&ini, &rules, &schemes, "Neutral"), None);
}
