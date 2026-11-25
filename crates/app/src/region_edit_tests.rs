use super::*;
use crate::editing::tests::{editor, terrain_scene};

#[test]
fn region_drag_crops_outside_size_and_tracks_reentry_in_both_selection_modes() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.activate_tool(Tool::Region);
    let [start, edge, outside, end] = [[25, 10], [29, 10], [34, 10], [32, 15]];
    let doc = e.doc.as_ref().unwrap();
    assert!(doc.contains_cell(end) && !doc.contains_cell(outside));
    let before = doc.snapshot().bytes().unwrap();
    assert_eq!(e.region.dimensions(), None);
    for custom in [false, true] {
        e.region.custom = custom;
        e.pointer(&scene, Some(start), true, true, false, None);
        assert_eq!(e.region.cells, BTreeSet::from([start]));
        assert_eq!(e.region.dimensions(), Some([1, 1]));
        e.pointer(&scene, Some(outside), false, true, false, None);
        assert_eq!(e.region.cells, cell_line(start, edge).into_iter().collect());
        assert_eq!(e.region.dimensions(), Some([5, 1]));
        e.pointer(&scene, Some(end), false, false, true, None);
        assert!(!e.stroking);
        let doc = e.doc.as_ref().unwrap();
        assert!(e.region.cells.contains(&start) && e.region.cells.contains(&end));
        assert!(e.region.cells.iter().all(|&at| doc.contains_cell(at)));
        assert_eq!(e.region.dimensions(), Some([8, 6]));
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn water_vehicle_paste_ghost_uses_the_pasted_terrain_and_overlay() {
    use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
    let fixture = Fixture::with_water_vehicle();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    for (source_tile, target_tile, overlay, expected) in [
        (1, 0, 255, "WATER.vxl"),
        (0, 1, 255, "SAM.vxl"),
        (1, 1, 0, "SAM.vxl"),
    ] {
        let bytes = crate::new_map::NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let (source, _) =
            ra2_map_document::source::SourceMap::parse("water.map".into(), "test".into(), bytes)
                .unwrap();
        let mut doc = MapDocument::new(source.edit_base.unwrap());
        for (at, tile) in [([20, 20], source_tile), ([21, 20], target_tile)] {
            let mut value = doc.terrain(at).unwrap();
            value.tile = tile;
            doc.set(Target::Terrain(at), Some(Value::Terrain(value)))
                .unwrap();
        }
        doc.add_record("Units", [20, 20], "Neutral", "ROBO")
            .unwrap();
        doc.set_ini("ROBO", "WaterImage", Some(b"WATER".to_vec()))
            .unwrap();
        doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(overlay, 0)))
            .unwrap();
        doc.commit("setup");
        let scene = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "water.map",
            "test".into(),
            doc.snapshot().bytes().unwrap(),
            Instant::now(),
        )
        .unwrap();
        let (mut editor, ctx, root) = editor();
        editor.attach(&scene);
        editor.auto_lat = false;
        editor.auto_walls = false;
        editor.region.cells = BTreeSet::from([[20, 20]]);
        editor.region_action(&scene, Action::Copy);
        editor.region_action(&scene, Action::Paste);
        let at = [21, 20];
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500., 400.));
        let camera = crate::camera::MapCamera {
            center: egui::pos2(60., 615.),
            zoom: 1.,
        };
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            editor.region_preview(ui.painter(), &scene, rect, &camera, Some(at));
        });
        assert!(editor.region.preview.as_ref().unwrap().error.is_none());
        editor.pointer(&scene, Some(at), true, false, true, None);
        let placed = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "placed.map",
            "test".into(),
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            Instant::now(),
        )
        .unwrap();
        let image = placed
            .object_sprite("Units", "ROBO", at, false)
            .unwrap()
            .image(0);
        assert_eq!(image.info.name, expected);
        let ghost = if expected == "WATER.vxl" {
            scene.sprites.water_objects["ROBO"].image(0)
        } else {
            scene.sprites.objects[&("Units".into(), "ROBO".into())].image(0)
        };
        assert!(
            editor
                .ui_state
                .map_sprites
                .contains_key(&(Arc::as_ptr(ghost) as usize)),
            "ghost must match the committed model for source {source_tile}, target {target_tile}, overlay {overlay}"
        );
        assert_eq!(ghost.rgba, image.rgba);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn high_bridge_region_ghost_matches_the_reloaded_unit() {
    use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
    use ra2_map_scene::visual::{object_anchor, sprite_position};
    use vera20k_core::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

    let fixture = Fixture::with_authoring();
    let units = Fixture::with_shp_turret();
    std::fs::copy(units.0.join("tank.shp"), fixture.0.join("tank.shp")).unwrap();
    std::fs::copy(units.0.join("unittem.pal"), fixture.0.join("unittem.pal")).unwrap();
    let rules_path = fixture.0.join("rulesmd.ini");
    let mut rules = std::fs::read_to_string(&rules_path).unwrap();
    rules.push_str("\n[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\n");
    let mut art = String::from("[TANK]\nFacings=1\nWalkFrames=0\nStandingFrames=1\n");
    for name in ["BRIDGE1", "BRIDGE2", "BRIDGEB1", "BRIDGEB2"] {
        rules.push_str(&format!("\n[{name}]\nImage={name}\n"));
        art.push_str(&format!("\n[{name}]\nTheater=yes\n"));
    }
    std::fs::write(rules_path, rules).unwrap();
    std::fs::write(fixture.0.join("artmd.ini"), art).unwrap();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let ids = vera20k_core::map::rmg::TileIds::resolve(&install.theater);
    let load = |install: &mut ra2_map_assets::assets::Install, doc: &MapDocument| {
        ra2_map_scene::map::load_scene_bytes(
            install,
            "bridge.map",
            "test".into(),
            doc.snapshot().bytes().unwrap(),
            Instant::now(),
        )
        .unwrap()
    };
    for (transpose, wood, overlay) in [
        (false, false, 24),
        (true, false, 25),
        (false, true, 237),
        (true, true, 238),
    ] {
        let orient = |x| if transpose { [40, x] } else { [x, 40] };
        let join_at = |[x, y]: Cell| {
            if transpose {
                [x + 5, y + 14]
            } else {
                [x + 14, y + 5]
            }
        };
        let source = orient(40);
        let frame = if transpose { 9 } else { 0 };
        let base = if wood {
            ids.cliffs.wood_bridge_set
        } else {
            ids.cliffs.bridge_set
        }
        .unwrap();
        let start_tile = install.catalog.raw_tiles[&(base + if transpose { 3 } else { 0 })];
        let end_tile = install.catalog.raw_tiles[&(base + if transpose { 5 } else { 2 })];
        let bytes = crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let (parsed, _) =
            ra2_map_document::source::SourceMap::parse("bridge.map".into(), "test".into(), bytes)
                .unwrap();
        let mut doc = MapDocument::new(parsed.edit_base.unwrap());
        for (x, tile, sub, level) in [
            (37, start_tile, 7, 9),
            (38, start_tile, if transpose { 12 } else { 8 }, 4),
            (42, 0, 0, 6),
            (43, end_tile, if transpose { 2 } else { 4 }, 11),
        ] {
            doc.set(
                Target::Terrain(orient(x)),
                Some(Value::Terrain(crate::document::TerrainValue {
                    tile,
                    sub,
                    level,
                    ice: 0,
                })),
            )
            .unwrap();
        }
        for x in 38..=42 {
            doc.set(
                Target::Overlay(orient(x)),
                Some(Value::Overlay(overlay, frame)),
            )
            .unwrap();
        }
        for x in [43, 44] {
            doc.set(
                Target::Overlay(join_at(orient(x))),
                Some(Value::Overlay(overlay, frame)),
            )
            .unwrap();
        }
        doc.set(
            Target::Terrain(join_at(orient(45))),
            Some(Value::Terrain(crate::document::TerrainValue {
                tile: end_tile,
                sub: if transpose { 2 } else { 4 },
                level: 6,
                ice: 0,
            })),
        )
        .unwrap();
        let id = doc.add_record("Units", source, "Neutral", "TANK").unwrap();
        let unit = doc.records()[&id].patched(&[(10, "1".into())]).unwrap();
        doc.set(Target::Record(id), Some(Value::Record(unit)))
            .unwrap();
        doc.commit("uneven high bridge");
        let scene = load(&mut install, &doc);
        // A complete span, partial spans, a paste that extends an existing
        // span, and a move that clears the start road while retaining stamps.
        for (selected, moving, joining, level) in [
            ((37..=43).collect::<Vec<_>>(), false, false, 9),
            ((38..=43).collect(), false, false, 11),
            ((38..=42).collect(), false, false, 8),
            ((37..=42).collect(), false, true, 6),
            (vec![37, 40], true, false, 11),
        ] {
            let (mut editor, ctx, root) = editor();
            editor.attach(&scene);
            editor.auto_lat = false;
            editor.auto_walls = false;
            editor.region.cells = selected.iter().map(|&x| orient(x)).collect();
            let origin = orient(selected[0]);
            let (at, target) = if moving {
                editor.region.layers.overlays = false;
                editor.region_action(&scene, Action::Move);
                (orient(selected[0] - 1), orient(39))
            } else {
                editor.region_action(&scene, Action::Copy);
                editor.region_action(&scene, Action::Paste);
                if joining {
                    (join_at(origin), join_at(source))
                } else {
                    (origin.map(|n| n + 10), source.map(|n| n + 10))
                }
            };
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500., 500.));
            let camera = crate::camera::MapCamera {
                center: egui::pos2(0., 1300.),
                zoom: 1.,
            };
            let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
                editor.region_preview(ui.painter(), &scene, rect, &camera, Some(at));
            });
            let preview = editor.region.preview.as_ref().unwrap();
            assert!(preview.error.is_none(), "{:?}", preview.error);
            let [ghost] = preview.records.as_slice() else {
                panic!("expected one ghost")
            };
            assert_eq!(ghost.at, target);
            assert_eq!(
                ghost.world_z,
                level * GROUND_LEVEL_HEIGHT_LEPTONS,
                "overlay={overlay}, selected={selected:?}, moving={moving}, joining={joining}"
            );
            let sprite = scene
                .object_sprite_for_terrain("Units", "TANK", ghost.ground, ghost.overlay, true)
                .unwrap();
            let expected = sprite_position(
                object_anchor(&ghost.record, sprite, ghost.world_z),
                sprite.image_at(0, target),
            );
            editor.pointer(&scene, Some(at), true, false, true, None);
            let placed = load(&mut install, editor.doc.as_ref().unwrap());
            let placed_doc = MapDocument::new(placed.source.edit_base.as_ref().unwrap().clone());
            let (owner, _) = placed_doc
                .records()
                .into_iter()
                .find(|(_, r)| r.section == "Units" && r.cell() == Some(target))
                .unwrap();
            assert!(
                placed
                    .visual
                    .draws
                    .iter()
                    .any(|draw| draw.owner == Some(owner)
                        && draw.position == [expected.x, expected.y]),
                "ghost at {expected:?} must match loaded unit, overlay={overlay}, selected={selected:?}, moving={moving}, joining={joining}"
            );
            drop(editor);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn startup_owner_property_dropdown_auto_commits_and_undo_restores_graph() {
    use egui_kittest::kittest::Queryable as _;
    use ra2_map_document::{mission, startup_owner};
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let id = editor
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [20, 20], "Americans", "GI")
        .unwrap();
    editor.commit(&scene, "place");
    editor.select(Some(id));
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    {
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size(egui::vec2(450., 1200.))
            .build_ui(|ui| {
                editor.selection_properties(ui, &scene);
            });
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "开局交给")
            .click();
        harness.run();
        harness.get_by_label("航点 0 的玩家").click();
        harness.run();
    }
    assert!(!editor.object_properties_dirty());
    assert_eq!(editor.property_startup_owner, Some(0));
    assert_eq!(editor.property_owner, "Neutral");
    assert_eq!(
        startup_owner::get(editor.doc.as_ref().unwrap(), &editor.records[&id]),
        Some(0)
    );
    let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    editor.property_startup_owner = None;
    let ctx = egui::Context::default();
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        editor.finish_property_for_save(ui.ctx(), &scene)
    });
    assert!(!editor.object_properties_dirty());
    assert!(mission::ids(editor.doc.as_ref().unwrap(), mission::Kind::Trigger).is_empty());
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn selection_modifiers_are_temporary_and_frozen_on_mouse_down() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.tool = Tool::Region;
    e.pointer(&scene, Some([20, 20]), true, false, true, None);
    e.region_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([22, 20]), true, true, false, None);
    e.region_modifiers(egui::Modifiers::ALT);
    e.pointer(&scene, Some([23, 20]), false, false, true, None);
    assert_eq!(
        e.region.cells,
        BTreeSet::from([[20, 20], [22, 20], [23, 20]])
    );
    e.region_modifiers(egui::Modifiers::ALT);
    e.pointer(&scene, Some([22, 20]), true, false, true, None);
    assert_eq!(e.region.cells, BTreeSet::from([[20, 20], [23, 20]]));
    assert_eq!(e.region.dimensions(), Some([4, 1]));
    assert!(e.region.op == SelectionOp::Replace);
    e.region_modifiers(egui::Modifiers::NONE);
    e.pointer(&scene, Some([21, 21]), true, false, true, None);
    assert_eq!(e.region.cells, BTreeSet::from([[21, 21]]));
    assert!(!e.doc.as_ref().unwrap().dirty());
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_drafts_survive_reselection_and_block_save_until_applied_or_discarded() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    let id = e
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [20, 20], "Neutral", "GI")
        .unwrap();
    e.commit(&scene, "place");
    e.select(Some(id));
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.property_health = 64;
    assert!(e.object_properties_dirty());
    assert!(e.select_from_canvas(Some(id)));
    assert_eq!(e.property_health, 64);
    assert!(!e.select_from_canvas(None));
    e.save(false); // A draft must not open a dialog or save stale properties.
    assert!(!e.saving);
    e.delete(&scene);
    assert!(e.records.contains_key(&id));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(false, &scene);
    assert!(!e.object_properties_dirty());
    assert_eq!(e.property_health, 256);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.property_health = 128;
    let r = e.records[&id].clone();
    e.apply_object_properties(&scene, &r, id).unwrap();
    e.history(false, &scene);
    assert_eq!(e.property_health, 256);
    assert!(!e.object_properties_dirty());
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_edits_auto_commit_once_after_drag_and_keep_invalid_input_out_of_history() {
    let (mut editor, ctx, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let id = editor
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [20, 20], "Neutral", "GI")
        .unwrap();
    editor.commit(&scene, "place");
    editor.select(Some(id));
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    for (pressed, value) in [(true, 200), (false, 128)] {
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(50., 50.),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| {
                editor.property_health = value;
                editor.finish_property_edit(ui.ctx(), &scene);
            },
        );
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
    }
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        editor.finish_property_edit(ui.ctx(), &scene)
    });
    assert!(!editor.object_properties_dirty());
    assert_eq!(editor.records[&id].fields()[2], "128");
    let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    assert!(
        editor.select_from_canvas(None),
        "completed edits must not block reselection"
    );
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    editor.select(Some(id));
    editor.property_type = "INVALID".into();
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        editor.finish_property_edit(ui.ctx(), &scene)
    });
    assert!(!editor.property_error.is_empty());
    assert!(!editor.select_from_canvas(None));
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    editor.history(false, &scene); // Cancel invalid input, preserving committed history.
    assert!(!editor.object_properties_dirty());
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn building_ghost_matches_placed_pixels_across_foundations_height_and_zoom() {
    use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
    use ra2_map_document::new_map::NewMapOptions;
    use std::time::Instant;
    for foundation in ["1x1", "3x2", "2x3", "3x3"] {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("rulesmd.ini"), b"[BuildingTypes]\n0=GI\n").unwrap();
        std::fs::write(
            fixture.0.join("artmd.ini"),
            format!("[GI]\nFoundation={foundation}\nXDrawOffset=3\nYDrawOffset=4\n"),
        )
        .unwrap();
        let mut install = fixture.install();
        install.catalog = Catalog::load(&install.manager, &install.theater);
        for level in [0, 4] {
            let scene = ra2_map_scene::map::load_scene_bytes(
                &mut install,
                "ghost.map",
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
            let (mut editor, ctx, root) = editor();
            editor.attach(&scene);
            editor.tool = Tool::Place;
            editor.section = "Structures".into();
            editor.type_id = "GI".into();
            let at = [20, 20];
            let (sx, sy) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
            let mut ghost_position = None;
            for zoom in [0.5, 1., 1.75] {
                let rect = egui::Rect::from_min_size(egui::pos2(71., 37.), egui::vec2(500., 400.));
                let camera = crate::camera::MapCamera {
                    center: egui::pos2(sx + 30., sy + 15.),
                    zoom,
                };
                let output = crate::test_support::run_ui(&ctx, egui::RawInput::default(), |ui| {
                    editor.region_preview(ui.painter(), &scene, rect, &camera, Some(at));
                });
                assert!(editor.region.preview.as_ref().unwrap().error.is_none());
                let mesh = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Mesh(mesh) = &shape.shape
                            && mesh
                                .vertices
                                .iter()
                                .all(|v| v.color == Color32::from_white_alpha(160))
                        {
                            Some(mesh)
                        } else {
                            None
                        }
                    })
                    .expect("placement ghost image");
                let min = mesh
                    .vertices
                    .iter()
                    .fold(egui::pos2(f32::INFINITY, f32::INFINITY), |p, v| {
                        p.min(v.pos)
                    });
                let world_min = camera.world_point(rect, min);
                // Canvas (NW tile row), ART offsets (+3,+4), cropped SHP (-1,0).
                assert!(
                    (world_min - egui::pos2(sx + 32., sy + 4.)).length() < 0.001,
                    "{foundation}, level={level}, zoom={zoom}: {world_min:?}"
                );
                ghost_position = Some(world_min);
            }
            editor.pointer(&scene, Some(at), true, false, true, None);
            let saved = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            let placed = ra2_map_scene::map::load_scene_bytes(
                &mut install,
                "placed.map",
                "test".into(),
                saved,
                Instant::now(),
            )
            .unwrap();
            let draw = placed
                .visual
                .draws
                .iter()
                .find(|d| d.owner.is_some())
                .expect("placed body");
            assert!(
                (egui::pos2(draw.position[0], draw.position[1]) - ghost_position.unwrap()).length()
                    < 0.001
            );
            drop(editor);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn unchanged_unknown_types_and_legacy_owners_do_not_block_nonspatial_property_edits() {
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let id = doc
        .add_record("Units", [20, 20], "Neutral", "UNLISTED")
        .unwrap();
    let legacy = doc.records()[&id]
        .patched(&[(0, "Map House".into())])
        .unwrap();
    doc.set(Target::Record(id), Some(Value::Record(legacy)))
        .unwrap();
    doc.add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    doc.commit("overlapping legacy objects");
    editor.cache(&scene);
    editor.select(Some(id));
    let r = editor.records[&id].clone();
    editor.property_extra.insert(8, "100".into());
    editor.apply_object_properties(&scene, &r, id).unwrap();
    let r = editor.doc.as_ref().unwrap().records()[&id].clone();
    assert_eq!(r.fields()[0], "Map House");
    assert_eq!(r.type_id(), "UNLISTED");
    assert_eq!(r.fields()[8], "100");
    editor.property_type = "ANOTHER_UNKNOWN".into();
    assert!(editor.apply_object_properties(&scene, &r, id).is_err());
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn selection_samples_release_custom_add_subtract_and_cancel_restores_previous_selection() {
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    editor.tool = Tool::Region;
    editor.pointer(&scene, Some([20, 20]), true, true, false, None);
    editor.pointer(&scene, Some([22, 21]), false, false, true, None);
    assert_eq!(editor.region.cells.len(), 6);
    let selected = editor.region.cells.clone();
    editor.pointer(&scene, Some([24, 24]), true, true, false, None);
    editor.region.cancel();
    editor.stroking = false;
    assert_eq!(editor.region.cells, selected);
    editor.region.custom = true;
    editor.region.op = SelectionOp::Add;
    editor.pointer(&scene, Some([23, 22]), true, true, false, None);
    editor.pointer(&scene, Some([25, 22]), false, false, true, None);
    assert_eq!(editor.region.cells.len(), 9);
    editor.region.op = SelectionOp::Subtract;
    editor.pointer(&scene, Some([24, 22]), true, false, true, None);
    assert!(!editor.region.cells.contains(&[24, 22]));
    assert_eq!(editor.region.cells.len(), 8);
    assert!(
        !editor.doc.as_ref().unwrap().dirty(),
        "selection creates no edits"
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn hidden_object_layers_are_not_copied_or_cut_and_boundary_failure_is_atomic() {
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let id = doc
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    doc.commit("fixture");
    editor.cache(&scene);
    editor.region.cells = BTreeSet::from([[20, 20], [21, 20]]);
    editor.layers[1] = false;
    editor.auto_lat = false;
    editor.region_action(&scene, Action::Cut);
    assert!(editor.region.clipboard.as_ref().unwrap().records.is_empty());
    assert!(editor.doc.as_ref().unwrap().records().contains_key(&id));
    editor.layers[1] = true;
    editor.region_action(&scene, Action::Copy);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.tool = Tool::Paste;
    editor.pointer(&scene, Some([1, 1]), true, false, true, None);
    assert!(editor.status.starts_with("未修改"));
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn native_clipboard_events_copy_and_paste_complete_region_data_and_focus_protects_text() {
    let (mut editor, ctx, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    editor.tool = Tool::Region;
    editor.region.cells = BTreeSet::from([[20, 20]]);
    let output = crate::test_support::run_ui(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::Copy],
            ..Default::default()
        },
        |ui| editor.shortcuts(ui.ctx(), &scene),
    );
    let text = output
        .platform_output
        .commands
        .into_iter()
        .find_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text)
            } else {
                None
            }
        })
        .unwrap();
    assert!(text.starts_with(CLIPBOARD_PREFIX));
    editor.region.clipboard = None;
    let _ = crate::test_support::run_ui(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::Paste(text)],
            ..Default::default()
        },
        |ui| editor.shortcuts(ui.ctx(), &scene),
    );
    assert_eq!(editor.tool, Tool::Paste);
    assert_eq!(editor.region.clipboard.as_ref().unwrap().cells.len(), 1);
    editor.tool = Tool::Region;
    let mut value = String::from("text");
    let _ = crate::test_support::run_ui(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::Paste("ordinary text".into())],
            ..Default::default()
        },
        |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
            editor.shortcuts(ui.ctx(), &scene);
        },
    );
    assert_eq!(
        editor.tool,
        Tool::Region,
        "typing must not arm region paste"
    );

    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn startup_garrison_property_applies_and_undoes() {
    use egui_kittest::kittest::Queryable as _;
    use ra2_map_document::{
        mission::{self, Kind},
        startup_garrison,
    };
    use vera20k_core::rules::ini_parser::IniFile;

    let (mut editor, _, root) = editor();
    let mut scene = terrain_scene();
    let catalog = Arc::make_mut(&mut scene.catalog);
    catalog.object_types.insert(
        "Infantry".into(),
        ["E1", "E2", "INIT"].map(str::to_owned).into(),
    );
    catalog.rules = IniFile::from_str(
        "[BUNKER]\nCanBeOccupied=yes\nMaxNumberOccupants=5\n\
         [E1]\nName=盟军大兵\nOccupier=yes\n[E2]\nName=征召兵\nOccupier=yes\n\
         [INIT]\nName=教团新兵\nOccupier=yes\n",
    );
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let id = doc
        .add_record("Structures", [20, 20], "Neutral", "BUNKER")
        .unwrap();
    doc.commit("place bunker");
    editor.cache(&scene);
    editor.select(Some(id));
    {
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([700., 950.])
            .build_ui(|ui| editor.selection_properties(ui, &scene));
        harness.get_by_label("开局派兵驻满").click();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "驻军步兵")
            .click();
        harness.run();
        for label in ["美国大兵 · E1", "动员兵 · E2", "尤里新兵 · INIT"] {
            harness.get_by_label(label);
        }
        harness.get_by_label("尤里新兵 · INIT").click();
        harness.run();
    }
    let doc = editor.doc.as_ref().unwrap();
    assert_eq!(
        startup_garrison::infantry(doc, doc.record(id).unwrap()).as_deref(),
        Some("INIT")
    );
    assert_eq!(mission::ids(doc, Kind::Team).len(), 1);
    editor.history(false, &scene);
    assert_eq!(editor.property_startup_garrison.as_deref(), Some("E1"));
    editor.history(false, &scene);
    let doc = editor.doc.as_ref().unwrap();
    assert!(startup_garrison::infantry(doc, doc.record(id).unwrap()).is_none());
    editor.history(true, &scene);
    editor.history(true, &scene);
    let doc = editor.doc.as_ref().unwrap();
    assert_eq!(
        startup_garrison::infantry(doc, doc.record(id).unwrap()).as_deref(),
        Some("INIT")
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn property_edit_preserves_non_utf8_tail_and_aircraft_uses_its_own_recruitment_columns() {
    let (mut editor, _, root) = editor();
    let mut scene = terrain_scene();
    Arc::make_mut(&mut scene.catalog)
        .object_types
        .insert("Aircraft".into(), vec!["ORCA".into()]);
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let id = doc
        .add_record("Aircraft", [20, 20], "Neutral", "ORCA")
        .unwrap();
    let mut r = doc.records()[&id].clone();
    r.value.extend_from_slice(b",\xd6\xd0,extension");
    doc.set(Target::Record(id), Some(Value::Record(r.clone())))
        .unwrap();
    doc.commit("fixture");
    editor.cache(&scene);
    editor.select(Some(id));
    editor.property_extra.insert(6, "Hunt".into());
    editor.property_extra.insert(8, "100".into());
    editor.property_extra.insert(10, "0".into());
    editor.apply_object_properties(&scene, &r, id).unwrap();
    let r = editor.doc.as_ref().unwrap().records()[&id].clone();
    assert!(r.value.ends_with(b",\xd6\xd0,extension"));
    assert_eq!(r.fields()[10], "0");
    assert_eq!(r.fields()[11], "1");
    let (_, map) = crate::map::SourceMap::parse(
        "reopen.map".into(),
        "fixture".into(),
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
    )
    .unwrap();
    let entity = map.entities.iter().find(|r| r.type_id == "ORCA").unwrap();
    assert_eq!(entity.veterancy, Some(100));
    assert!(!entity.recruitable_a && entity.recruitable_b);
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn house_choices_track_pending_creates_deletes_and_history_without_reopening() {
    use ra2_map_document::mission::{self, Kind};
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    mission::create(
        editor.doc.as_mut().unwrap(),
        Kind::House,
        "CustomHouse",
        "Neutral",
        None,
    )
    .unwrap();
    assert!(editor.houses(&scene).contains(&"CustomHouse".into()));
    editor.commit(&scene, "new house");
    editor.history(false, &scene);
    assert!(!editor.houses(&scene).contains(&"CustomHouse".into()));
    editor.history(true, &scene);
    assert!(editor.houses(&scene).contains(&"CustomHouse".into()));
    mission::delete(editor.doc.as_mut().unwrap(), Kind::House, "CustomHouse").unwrap();
    editor.commit(&scene, "delete house");
    assert!(!editor.houses(&scene).contains(&"CustomHouse".into()));
    editor.history(false, &scene);
    assert!(editor.houses(&scene).contains(&"CustomHouse".into()));
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cell_tag_clipboards_preserve_legacy_payloads_and_respect_hidden_layer_on_paste() {
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    editor
        .doc
        .as_mut()
        .unwrap()
        .set_ini("CellTags", "20020", Some(b"TAG_SHARED".to_vec()))
        .unwrap();
    editor.commit(&scene, "tag fixture");
    editor.region.cells.insert([20, 20]);
    editor.region_action(&scene, Action::Copy);
    let clip = editor.region.clipboard.as_ref().unwrap();
    assert_eq!(
        clip.cells[0].cell_tag.as_deref(),
        Some(b"TAG_SHARED".as_slice())
    );
    let json = serde_json::to_value(clip).unwrap();
    let mut legacy = json.clone();
    legacy["layers"]
        .as_object_mut()
        .unwrap()
        .remove("cell_tags");
    for cell in legacy["cells"].as_array_mut().unwrap() {
        cell.as_object_mut().unwrap().remove("cell_tag");
    }
    let old: Clipboard = serde_json::from_value(legacy).unwrap();
    assert!(!old.layers.cell_tags);
    assert!(old.cells[0].cell_tag.is_none());
    let copied: Clipboard = serde_json::from_value(json).unwrap();
    assert_eq!(
        copied.cells[0].cell_tag.as_deref(),
        Some(b"TAG_SHARED".as_slice())
    );
    editor.task_labels = false;
    editor.tool = Tool::Paste;
    editor.pointer(&scene, Some([21, 20]), true, false, true, None);
    assert!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .ini_value("CellTags", "20021")
            .is_none()
    );
    editor.region_action(&scene, Action::Cut);
    assert_eq!(
        editor.doc.as_ref().unwrap().ini_text("CellTags", "20020"),
        "TAG_SHARED"
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
