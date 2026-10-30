use super::*;
use crate::editing::tests::{editor, terrain_scene};

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
        let _ = ctx.run_ui(
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
    let _ = ctx.run_ui(Default::default(), |ui| {
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
    let _ = ctx.run_ui(Default::default(), |ui| {
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
                let output = ctx.run_ui(egui::RawInput::default(), |ui| {
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
    let output = ctx.run_ui(
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
    let _ = ctx.run_ui(
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
    let _ = ctx.run_ui(
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
