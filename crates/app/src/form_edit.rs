//! Commit a completed field edit, keeping typing and a drag in one undo step.
use eframe::egui;

#[derive(Default)]
pub(crate) struct EditCompletion {
    text: Option<egui::Id>,
    cancel: Option<egui::Id>,
}

impl EditCompletion {
    pub fn ready(&self, ctx: &egui::Context) -> bool {
        if egui::Popup::is_any_open(ctx) {
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
    fn text_typing_waits_for_enter_and_drags_wait_for_release() {
        let ctx = egui::Context::default();
        let mut completion = EditCompletion::default();
        let mut value = String::from("name");
        let mut ready = false;
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
            completion.observe(ui.ctx());
        });
        let _ = ctx.run_ui(
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
        let _ = ctx.run_ui(
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
                    ready = EditCompletion::default().ready(ui.ctx());
                },
            );
            assert!(!ready, "drag and release frame must stay in one edit");
        }
        let _ = ctx.run_ui(Default::default(), |ui| {
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
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
            navigation = ui.button("navigate").rect.center();
            let response = ui.button("cancel");
            cancel = response.rect.center();
            completion.cancel_button(&response);
            completion.observe(ui.ctx());
        });
        let mut ready = false;
        let _ = ctx.run_ui(
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
        let _ = ctx.run_ui(
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
        let _ = ctx.run_ui(
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
