//! Editable IDs share authoritative picker candidates; unknown mod values stay editable.
use crate::i18n::tr;
use eframe::egui;

/// A searchable picker opens even for an empty ID. Explicit custom entry reuses
/// the same completion owner, so save and focus handling see its popup too.
pub(crate) fn picker(
    ui: &mut egui::Ui,
    id: egui::Id,
    value: &mut String,
    choices: &[(String, String)],
    name: &str,
) {
    let custom_id = id.with("custom");
    let mut custom = ui.data(|d| d.get_temp::<bool>(custom_id).unwrap_or(false));
    let selected = if value.is_empty() {
        tr!("使用默认值").to_owned()
    } else {
        choices
            .iter()
            .find(|(token, _)| token.eq_ignore_ascii_case(value))
            .map_or_else(
                || value.clone(),
                |(token, label)| format!("{token} · {label}"),
            )
    };
    let button_text = tr!("自定义…");
    let button_width = (ui
        .painter()
        .layout_no_wrap(
            button_text.to_owned(),
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x
        + ui.spacing().button_padding.x * 2.)
        .max(80.);
    let picker_width =
        (ui.available_width() - button_width - ui.spacing().item_spacing.x).clamp(100., 235.);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let picker = ui
                .allocate_ui_with_layout(
                    egui::vec2(picker_width, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::ComboBox::from_id_salt(id)
                            .width(picker_width)
                            .height(250.)
                            .wrap_mode(egui::TextWrapMode::Truncate)
                            .selected_text(&selected)
                            .show_ui(ui, |ui| {
                                let search_id = id.with("filter");
                                let mut filter = ui
                                    .data(|d| d.get_temp::<String>(search_id).unwrap_or_default());
                                ui.add(
                                    egui::TextEdit::singleline(&mut filter)
                                        .hint_text(tr!("搜索名称或编号…")),
                                )
                                .accessible_name(tr!("搜索{arg0}", arg0 = name));
                                ui.selectable_value(value, String::new(), tr!("使用默认值"));
                                let lower = filter.to_lowercase();
                                for (token, label) in choices
                                    .iter()
                                    .filter(|(token, label)| {
                                        token.to_lowercase().contains(&lower)
                                            || label.to_lowercase().contains(&lower)
                                    })
                                    .take(250)
                                {
                                    ui.selectable_value(
                                        value,
                                        token.clone(),
                                        format!("{token} · {label}"),
                                    );
                                }
                                ui.data_mut(|d| d.insert_temp(search_id, filter));
                            })
                            .response
                            .accessible_name(name)
                            .on_hover_text(&selected)
                    },
                )
                .inner;
            if ui
                .add_sized(
                    egui::vec2(button_width, picker.rect.height()),
                    egui::Button::new(button_text)
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .selected(custom),
                )
                .clicked()
            {
                custom = !custom;
            }
        });
        if custom {
            reference(ui, id.with("text"), value, choices, name, picker_width);
        }
    });
    ui.data_mut(|d| d.insert_temp(custom_id, custom));
}

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

    #[test]
    fn picker_search_selects_from_an_empty_value_and_preserves_unknown_ids() {
        let mut value = "MOD_CUSTOM".to_owned();
        let choices = choices();
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([420., 360.])
            .build_ui(|ui| {
                picker(
                    ui,
                    ui.make_persistent_id("picker"),
                    &mut value,
                    &choices,
                    "Type",
                );
            });
        harness.run();
        #[cfg(feature = "ui-test-video")]
        {
            let output =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/custom-button");
            std::fs::create_dir_all(&output).unwrap();
            harness
                .render()
                .unwrap()
                .save(output.join("closed.png"))
                .unwrap();
        }
        harness.get_by_label("自定义…").click();
        harness.run();
        assert_eq!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::TextInput, "Type")
                .value()
                .as_deref(),
            Some("MOD_CUSTOM")
        );
        #[cfg(feature = "ui-test-video")]
        {
            let output =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/custom-button");
            harness
                .render()
                .unwrap()
                .save(output.join("open.png"))
                .unwrap();
        }
        harness.get_by_label("自定义…").click();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Type")
            .click();
        harness.run();
        harness.get_by_label("使用默认值").click();
        harness.run();
        assert_eq!(
            harness
                .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Type")
                .value()
                .as_deref(),
            Some("使用默认值")
        );
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "Type")
            .click();
        harness.run();
        harness.get_by_label("搜索Type").type_text("grizzly");
        harness.run();
        harness.get_by_label("TANK · Grizzly Tank").click();
        harness.run();
        drop(harness);
        assert_eq!(value, "TANK");
    }

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
