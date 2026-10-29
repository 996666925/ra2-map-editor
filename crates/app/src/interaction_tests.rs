use super::*;
use crate::document::{Target, Value};
use crate::editing::tests::{editor, terrain_scene};

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
    egui::RawInput {
        modifiers,
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
        ..Default::default()
    }
}

#[test]
fn undo_during_a_stroke_cancels_only_that_stroke_and_keeps_previous_history() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    let original = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.doc
        .as_mut()
        .unwrap()
        .set(Target::Overlay([20, 20]), Some(Value::Overlay(1, 2)))
        .unwrap();
    e.commit(&scene, "previous operation");
    let previous = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Erase;
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    assert_eq!(e.doc.as_ref().unwrap().overlay([20, 20]).0, 255);
    e.history(false, &scene);
    assert_eq!(
        e.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        previous
    );
    assert_eq!(
        e.doc.as_ref().unwrap().undo_name(),
        Some("previous operation")
    );
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(
        e.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        previous
    );
    e.history(false, &scene);
    assert_eq!(
        e.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        original
    );
    e.history(true, &scene);
    assert_eq!(
        e.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        previous
    );
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shortcuts_switch_tools_but_text_entry_and_open_menus_do_not_edit_the_map() {
    let (mut e, ctx, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let _ = ctx.run_ui(key(egui::Key::R, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.tool, Tool::Region);
    let _ = ctx.run_ui(key(egui::Key::B, egui::Modifiers::ALT), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.tool, Tool::Region);
    let mut text = String::new();
    let _ = ctx.run_ui(Default::default(), |ui| {
        ui.text_edit_singleline(&mut text).request_focus();
    });
    let _ = ctx.run_ui(key(egui::Key::B, egui::Modifiers::NONE), |ui| {
        ui.text_edit_singleline(&mut text);
        e.shortcuts(ui.ctx(), &scene);
    });
    assert_eq!(e.tool, Tool::Region);
    ctx.memory_mut(|m| m.stop_text_input());
    egui::Popup::open_id(&ctx, egui::Id::new("test-menu"));
    let _ = ctx.run_ui(key(egui::Key::E, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.tool, Tool::Region);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn escape_exits_repeat_placement_and_bracket_keys_clamp_the_brush() {
    let (mut e, ctx, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.tool = Tool::Clone;
    let _ = ctx.run_ui(key(egui::Key::Escape, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.tool, Tool::Select);
    e.tool = Tool::Ore;
    e.radius = 5;
    let _ = ctx.run_ui(key(egui::Key::CloseBracket, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.radius, 5);
    let _ = ctx.run_ui(key(egui::Key::OpenBracket, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.radius, 4);
    e.radius = 1;
    let _ = ctx.run_ui(key(egui::Key::OpenBracket, egui::Modifiers::NONE), |ui| {
        e.shortcuts(ui.ctx(), &scene)
    });
    assert_eq!(e.radius, 1);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unavailable_tools_explain_the_prerequisite_and_keep_the_current_tool() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.tool = Tool::Select;
    for tool in [Tool::Move, Tool::Clone, Tool::Paste, Tool::RegionMove] {
        e.activate_tool(tool);
        assert_eq!(e.tool, Tool::Select);
        assert!(e.status.starts_with("请先"));
    }
    e.activate_tool(Tool::Region);
    assert_eq!(e.tool, Tool::Region);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terrain_shortcuts_reveal_the_workspace_and_preserve_text_entry_and_copy() {
    let (mut e, ctx, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    for &(key_code, mode, _) in &TERRAIN_KEYS {
        e.show_library(1, "Structures", "");
        let _ = ctx.run_ui(key(key_code, egui::Modifiers::NONE), |ui| {
            e.shortcuts(ui.ctx(), &scene);
        });
        assert_eq!(e.tool, Tool::Terrain);
        assert_eq!(e.terrain_mode, mode);
        assert_eq!(e.ui_state.tab, 0);
    }
    let mut text = String::new();
    let _ = ctx.run_ui(Default::default(), |ui| {
        ui.text_edit_singleline(&mut text).request_focus();
    });
    let _ = ctx.run_ui(key(egui::Key::G, egui::Modifiers::NONE), |ui| {
        ui.text_edit_singleline(&mut text);
        e.shortcuts(ui.ctx(), &scene);
    });
    assert_eq!(e.terrain_mode, TerrainMode::Plateau);
    ctx.memory_mut(|m| m.stop_text_input());
    let _ = ctx.run_ui(key(egui::Key::C, egui::Modifiers::COMMAND), |ui| {
        e.shortcuts(ui.ctx(), &scene);
    });
    assert_eq!(e.terrain_mode, TerrainMode::Plateau);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn choosing_fill_material_keeps_fill_active_and_cannot_change_an_active_stroke() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.show_library(0, "", "悬崖");
    e.ui_state.terrain_category = Some(ra2_map_assets::terrain_library::TerrainCategory::Cliff);
    e.activate_terrain_mode(TerrainMode::Fill);
    assert!(e.ui_state.terrain_category.is_none());
    e.select_terrain_material(0, false);
    assert_eq!(e.tool, Tool::Terrain);
    assert_eq!(e.terrain_mode, TerrainMode::Fill);

    e.activate_tool(Tool::Select);
    e.select_terrain_material(0, false);
    assert_eq!(e.terrain_mode, TerrainMode::Template);

    e.activate_terrain_mode(TerrainMode::Raise);
    e.smooth_heights = false;
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    assert!(e.stroking);
    e.activate_terrain_mode(TerrainMode::CliffOutline);
    e.select_terrain_material(1, false);
    assert_eq!(e.tool, Tool::Terrain);
    assert_eq!(e.terrain_mode, TerrainMode::Raise);
    assert_eq!(e.brush, 0);
    e.pointer(&scene, None, false, false, true, None);
    assert!(e.doc.as_ref().unwrap().undo_name().is_some());
    e.activate_terrain_mode(TerrainMode::CliffOutline);
    assert_eq!(e.tool, Tool::Terrain);
    assert_eq!(e.terrain_mode, TerrainMode::CliffOutline);
    e.activate_tool(Tool::Pick);
    e.pointer(&scene, Some([20, 20]), true, false, true, None);
    assert_eq!(e.tool, Tool::Terrain);
    assert_eq!(e.terrain_mode, TerrainMode::Subtile);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
