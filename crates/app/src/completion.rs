//! Editable IDs share authoritative picker candidates; unknown mod values stay editable.
use eframe::egui;

#[derive(Clone, Default)]
struct OpenPopups(std::collections::HashMap<egui::Id, u64>);

fn popup_state_id(ctx: &egui::Context) -> egui::Id {
    egui::Id::unique("reference-completions").with(ctx.viewport_id())
}

/// CompletionPopup owns its open state, so Popup::is_any_open cannot see it.
/// Keep the previous pass visible to pre-widget commit and shortcut handling.
pub(crate) fn is_open(ctx: &egui::Context) -> bool {
    let oldest = ctx.cumulative_pass_nr().saturating_sub(1);
    let state_id = popup_state_id(ctx);
    ctx.data(|data| data.get_temp::<OpenPopups>(state_id))
        .is_some_and(|popups| popups.0.values().any(|pass| *pass >= oldest))
}

pub(crate) fn reference(
    ui: &mut egui::Ui,
    id: egui::Id,
    value: &mut String,
    choices: &[(String, String)],
    name: &str,
    width: f32,
) -> egui::Response {
    let output = egui::CompletionPopup::new(id)
        // An ID (including a CSF key or a name with spaces) is one operand.
        .word_boundary(|_| false)
        .show(
            ui,
            egui::TextEdit::singleline(value)
                .hint_text(name)
                .desired_width(width),
            |query| suggestions(query.text, choices),
        );
    let pass = ui.ctx().cumulative_pass_nr();
    let state_id = popup_state_id(ui.ctx());
    ui.data_mut(|data| {
        let popups = data.get_temp_mut_or_default::<OpenPopups>(state_id);
        popups.0.retain(|_, last| *last >= pass.saturating_sub(1));
        if output.is_open && output.accepted.is_none() {
            popups.0.insert(id, pass);
        } else {
            popups.0.remove(&id);
        }
    });
    let mut response = output.text_edit.response.response;
    if let Some(accepted) = output.accepted {
        // CompletionPopup replaces up to the cursor. References replace the
        // whole operand, including any suffix when editing in the middle.
        *value = accepted.insert;
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(egui::text::CharIndex(value.chars().count())),
            )));
        state.store(ui.ctx(), id);
        response.mark_changed();
    }
    response.accessible_name(name)
}

fn suggestions(text: &str, choices: &[(String, String)]) -> Vec<egui::Suggestion> {
    if text.is_empty() {
        return Vec::new();
    }
    let query = text.to_lowercase();
    choices
        .iter()
        .filter(|(token, label)| {
            !token.eq_ignore_ascii_case(text)
                && (token.to_lowercase().contains(&query) || label.to_lowercase().contains(&query))
        })
        .take(50)
        .map(|(token, label)| {
            egui::Suggestion::new(token.clone()).content(format!("{token} · {label}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable as _;

    fn choices() -> Vec<(String, String)> {
        vec![
            ("E1".into(), "GI".into()),
            ("E2".into(), "Conscript".into()),
            ("TANK".into(), "Grizzly Tank".into()),
        ]
    }

    #[test]
    fn reference_completion_accepts_a_display_name_search_as_an_id() {
        let mut text = String::new();
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            reference(
                ui,
                ui.make_persistent_id("type"),
                &mut text,
                &choices(),
                "Type ID",
                200.,
            );
        });
        harness.get_by_label("Type ID").focus();
        harness.get_by_label("Type ID").type_text("grizzly");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(
            harness.ctx.text_edit_focused(),
            "accepting a suggestion keeps keyboard focus"
        );
        drop(harness);
        assert_eq!(text, "TANK");
    }

    #[test]
    fn reference_completion_navigation_escape_and_unknown_ids_preserve_free_text() {
        let mut text = String::new();
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            reference(
                ui,
                ui.make_persistent_id("type"),
                &mut text,
                &choices(),
                "Type ID",
                200.,
            );
        });
        harness.get_by_label("Type ID").focus();
        harness.get_by_label("Type ID").type_text("E");
        harness.run();
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        harness.key_press(egui::Key::Tab);
        harness.run();
        assert_eq!(
            harness.get_by_label("Type ID").value().as_deref(),
            Some("E2")
        );
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.get_by_label("Type ID").type_text("MOD_UNLISTED");
        harness.run();
        harness.key_press(egui::Key::Escape);
        harness.run();
        drop(harness);
        assert_eq!(text, "MOD_UNLISTED");
    }

    #[test]
    fn reference_completion_replaces_a_whole_operand_when_the_cursor_is_in_the_middle() {
        let mut text = "grizzly".to_owned();
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            reference(
                ui,
                ui.make_persistent_id("type"),
                &mut text,
                &choices(),
                "Type ID",
                200.,
            );
        });
        harness.get_by_label("Type ID").focus();
        harness.run();
        harness.key_press(egui::Key::Home);
        harness.run();
        harness.key_press(egui::Key::ArrowRight);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        drop(harness);
        assert_eq!(text, "TANK");
    }

    #[test]
    fn reference_suggestions_match_ids_and_chinese_labels_and_bound_large_catalogs() {
        let choices = vec![
            ("E1".into(), "美国大兵".into()),
            ("e2".into(), "动员兵".into()),
        ];
        assert_eq!(suggestions("大兵", &choices)[0].insert, "E1");
        assert_eq!(suggestions("E", &choices).len(), 2);
        assert!(suggestions("E1", &choices).is_empty());
        assert!(suggestions("", &choices).is_empty());
        assert!(suggestions("MOD_ID", &choices).is_empty());
        let large = (0..1000)
            .map(|i| (format!("UNIT{i}"), "Unit".into()))
            .collect::<Vec<_>>();
        assert_eq!(suggestions("unit", &large).len(), 50);
    }

    #[test]
    fn reference_popup_blocks_save_and_completes_before_a_field_can_commit() {
        let mut text = String::new();
        let saves = std::cell::Cell::new(0);
        let mut completion = crate::form_edit::EditCompletion::default();
        let mut harness = crate::test_support::harness().build_ui(|ui| {
            // The app's logic phase checks completion before drawing widgets.
            if is_open(ui.ctx()) {
                assert!(!completion.ready(ui.ctx()));
            }
            reference(
                ui,
                ui.make_persistent_id("type"),
                &mut text,
                &choices(),
                "Type ID",
                200.,
            );
            completion.observe(ui.ctx());
            if crate::form_edit::take_save_shortcut(ui.ctx()).is_some() {
                saves.set(saves.get() + 1);
            }
        });
        harness.get_by_label("Type ID").focus();
        harness.get_by_label("Type ID").type_text("grizzly");
        harness.run();
        assert!(is_open(&harness.ctx));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(saves.get(), 0);
        harness.get_by_label("TANK · Grizzly Tank").click();
        harness.run();
        assert_eq!(
            harness.get_by_label("Type ID").value().as_deref(),
            Some("TANK")
        );
        assert!(!is_open(&harness.ctx));
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(saves.get(), 1);
    }
}
