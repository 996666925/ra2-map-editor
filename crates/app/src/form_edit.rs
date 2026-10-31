//! Commit a completed field edit, keeping typing and a drag in one undo step.
use eframe::egui;

pub(crate) fn popup_open(ctx: &egui::Context) -> bool {
    egui::Popup::is_any_open(ctx) || ctx.any_popup_open() || crate::completion::is_open(ctx)
}

/// Consume document save separately from canvas shortcuts. Text undo, delete,
/// copy and paste must continue to belong to the focused input widget.
pub(crate) fn take_save_shortcut(ctx: &egui::Context) -> Option<bool> {
    if popup_open(ctx) || ctx.input(|i| i.pointer.any_down()) {
        return None;
    }
    for (modifiers, as_new) in [
        (egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, true),
        (egui::Modifiers::COMMAND, false),
    ] {
        let shortcut = egui::KeyboardShortcut::new(modifiers, egui::Key::S);
        if (!ctx.text_edit_focused() || !shortcut.conflicts_with_text_editing())
            && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
        {
            return Some(as_new);
        }
    }
    None
}

#[derive(Default)]
pub(crate) struct EditCompletion {
    text: Option<egui::Id>,
    cancel: Option<egui::Id>,
}

/// Finish pending numeric text before the owner captures a document save.
pub(crate) fn number(ui: &mut egui::Ui, value: egui::DragValue<'_>, name: &str) -> egui::Response {
    if ui.input(|i| i.modifiers.command && !i.modifiers.alt && i.key_pressed(egui::Key::S))
        && !popup_open(ui.ctx())
    {
        let id = ui.next_auto_id();
        ui.memory_mut(|memory| memory.surrender_focus(id));
    }
    ui.add(value.update_while_editing(false))
        .accessible_name(name)
}

impl EditCompletion {
    pub fn ready(&self, ctx: &egui::Context) -> bool {
        if popup_open(ctx) {
            return false;
        }
        // Widgets may update once more on release. Wait until the next frame
        // so the final slider value joins the same transaction as the drag.
        if ctx.input(|i| i.pointer.any_released()) {
            ctx.request_repaint();
            return false;
        }
        // Flush before drawing navigation buttons, so the very click that
        // finishes a text edit can also select a different record or save.
        let cancelling = self.cancel.is_some_and(|id| {
            ctx.read_response(id).is_some_and(|response| {
                ctx.input(|i| {
                    i.pointer
                        .interact_pos()
                        .is_some_and(|p| response.rect.contains(p))
                })
            })
        });
        let clicked_elsewhere = !cancelling
            && self.text.is_some_and(|id| {
                ctx.read_response(id).is_some_and(|response| {
                    ctx.input(|i| {
                        i.pointer.any_pressed()
                            && i.pointer
                                .interact_pos()
                                .is_some_and(|p| !response.rect.contains(p))
                    })
                })
            });
        clicked_elsewhere
            || (!ctx.input(|i| i.pointer.any_down())
                && (!ctx.text_edit_focused()
                    || self
                        .text
                        .is_some_and(|id| ctx.memory(|m| m.focused()) != Some(id))))
    }

    pub fn observe(&mut self, ctx: &egui::Context) {
        self.text = ctx
            .text_edit_focused()
            .then(|| ctx.memory(|m| m.focused()))
            .flatten();
    }

    pub fn cancel_button(&mut self, response: &egui::Response) {
        self.cancel = Some(response.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_shortcut_commits_numeric_text_once_and_distinguishes_save_as() {
        use egui_kittest::kittest::Queryable as _;
        let mut value = 1;
        let saved = std::cell::Cell::new(None);
        let changes = std::cell::Cell::new(0);
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            if number(
                ui,
                egui::DragValue::new(&mut value).range(0..=1000),
                "Count",
            )
            .changed()
            {
                changes.set(changes.get() + 1);
            }
            if let Some(as_new) = take_save_shortcut(ui.ctx()) {
                saved.set(Some((value, as_new)));
            }
        });
        harness.get_by_label("Count").focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.get_by_label("Count").type_text("123");
        harness.run();
        assert_eq!(changes.get(), 0, "typing must not commit partial numbers");
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(saved.get(), Some((123, false)));
        assert_eq!(changes.get(), 1);
        saved.set(None);
        harness.key_press_modifiers(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::S,
        );
        harness.run();
        assert_eq!(saved.get(), Some((123, true)));
        assert_eq!(changes.get(), 1);
    }

    #[test]
    fn save_keeps_text_focus_and_leaves_text_undo_to_the_field() {
        use egui_kittest::kittest::Queryable as _;
        let mut value = String::from("original");
        let saves = std::cell::Cell::new(0);
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            ui.text_edit_singleline(&mut value).accessible_name("Name");
            if take_save_shortcut(ui.ctx()).is_some() {
                saves.set(saves.get() + 1);
            }
        });
        harness.get_by_label("Name").focus();
        harness.run();
        harness.get_by_label("Name").type_text(" edited");
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(saves.get(), 1);
        assert!(harness.get_by_label("Name").is_focused());
        assert!(
            harness
                .get_by_label("Name")
                .value()
                .unwrap()
                .contains("edited")
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        harness.run();
        assert_eq!(saves.get(), 1);
        assert_eq!(
            harness.get_by_label("Name").value().as_deref(),
            Some("original")
        );
    }

    #[test]
    fn text_typing_waits_for_enter_and_drags_wait_for_release() {
        let ctx = egui::Context::default();
        let mut completion = EditCompletion::default();
        let mut value = String::from("name");
        let mut ready = false;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
            completion.observe(ui.ctx());
        });
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::Text(" edited".into())],
                ..Default::default()
            },
            |ui| {
                ui.text_edit_singleline(&mut value);
                ready = completion.ready(ui.ctx());
                completion.observe(ui.ctx());
            },
        );
        assert!(!ready, "typing must not commit partial text");
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| {
                ui.text_edit_singleline(&mut value);
                ready = completion.ready(ui.ctx());
            },
        );
        assert!(ready, "Enter completes a single-line text edit");

        for pressed in [true, false] {
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
                    ready = EditCompletion::default().ready(ui.ctx());
                },
            );
            assert!(!ready, "drag and release frame must stay in one edit");
        }
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            ready = EditCompletion::default().ready(ui.ctx());
        });
        assert!(ready);
    }

    #[test]
    fn navigation_flushes_before_its_click_but_cancel_does_not_commit() {
        let ctx = egui::Context::default();
        let mut completion = EditCompletion::default();
        let mut value = "name".to_owned();
        let mut navigation = egui::Pos2::ZERO;
        let mut cancel = egui::Pos2::ZERO;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
            navigation = ui.button("navigate").rect.center();
            let response = ui.button("cancel");
            cancel = response.rect.center();
            completion.cancel_button(&response);
            completion.observe(ui.ctx());
        });
        let mut ready = false;
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: navigation,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| ready = completion.ready(ui.ctx()),
        );
        assert!(
            ready,
            "navigation must see the completed edit before handling the click"
        );
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: navigation,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| {
                ui.text_edit_singleline(&mut value).request_focus();
                let _ = ui.button("navigate");
                let response = ui.button("cancel");
                completion.cancel_button(&response);
                completion.observe(ui.ctx());
            },
        );
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: cancel,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ui| ready = completion.ready(ui.ctx()),
        );
        assert!(
            !ready,
            "cancel must discard the draft without committing on mouse-down"
        );
    }
}
