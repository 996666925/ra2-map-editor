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
