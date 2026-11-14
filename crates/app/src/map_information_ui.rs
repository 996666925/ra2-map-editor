//! Additional map information follows the document's engine-backed field inventory.
use crate::i18n::tr;
use crate::map::MapScene;
use eframe::egui;
use ra2_map_document::map_information::{Draft, FIELDS, Group, Kind};
use ra2_map_document::{
    document::MapDocument,
    mission::{self, schema::ParameterKind},
};

pub(crate) struct IdentityFields<'a> {
    pub name: &'a mut String,
    pub author: &'a mut String,
    pub players: &'a mut u8,
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    original: &Draft,
    doc: &MapDocument,
    scene: &MapScene,
    mut identity: IdentityFields<'_>,
) {
    let label_width = (ui.available_width() * 0.28).clamp(140., 210.);
    for group in Group::ALL {
        egui::Frame::new().fill(ui.visuals().faint_bg_color)
            .stroke(ui.visuals().window_stroke).corner_radius(6).inner_margin(10.)
            .show(ui, |ui| {
                // Frame has already subtracted padding AND border strokes.
                // Using the parent's width minus padding grows the outer frame
                // by its border each pass, and Resize then grows the window.
                ui.set_min_width(ui.available_width());
                let help = match group {
                    Group::Listing => tr!("说明可分行输入，适用模式可多选；出生点数量同时决定地图的最大玩家人数。"),
                    Group::Environment => tr!("冰层和藤蔓开关可保存场景状态，当前引擎尚无对应生长模拟；变异体选项仅在 Ares 生效。"),
                    Group::Special => tr!("单人任务与地图编辑加载读取特殊规则；遭遇战和多人游戏的部分规则由房间设置接管。矿车免于自动索敌不等于免疫伤害。"),
                    Group::Campaign => tr!("适用于单人任务和战役。所属方和航点可从当前地图选择；继承比例按百分比调整，资金上限可选择不限。"),
                    Group::Media => tr!("从当前游戏或 Mod 的音乐、影片目录选择；地图文字说明在“基本信息”中编辑。自定义资源可手动输入。"),
                    Group::Multiplayer => tr!("地图内的多人开局默认值；启动器、房间和试玩设置可能覆盖。"),
                };
                let section = egui::CollapsingHeader::new(crate::i18n::text(group.label()))
                    .id_salt(("map-information-section-v2", group as u8))
                    .default_open(group == Group::Listing)
                    .show(ui, |ui| {
                egui::Grid::new(("map-information-fields", group as u8))
                    .num_columns(2)
                    .spacing([16., 8.])
                    .show(ui, |ui| {
                        if group == Group::Listing {
                            identity_fields(ui, &mut identity, label_width);
                        }
                        for field in FIELDS.iter().filter(|f| f.group == group) {
                            let label = crate::i18n::text(field.label);
                            field_label(ui, label, label_width).on_hover_text(format!("[{}] {}", field.section, field.key));
                            let value = draft.values.get_mut(&(field.section, field.key)).unwrap();
                            match field.kind {
                                Kind::Boolean => {
                                    let parsed = vera20k_core::rules::ini_parser::IniFile::from_str(
                                        &format!("[Value]\nKey={value}\n"),
                                    ).section_or_empty("Value").read_bool_value("Key");
                                    let selected = if value.trim().is_empty() {
                                        tr!("使用默认值")
                                    } else {
                                        match parsed {
                                            Some(true) => tr!("开启"),
                                            Some(false) => tr!("关闭"),
                                            None => tr!("保留原值"),
                                        }
                                    };
                                    egui::ComboBox::from_id_salt((field.section, field.key))
                                        .width(235.)
                                        .selected_text(selected)
                                        .show_ui(ui, |ui| {
                                            for (raw, text) in [("", tr!("使用默认值")), ("yes", tr!("开启")), ("no", tr!("关闭"))] {
                                                ui.selectable_value(value, raw.to_owned(), text);
                                            }
                                        }).response.accessible_name(label);
                                }
                                Kind::Briefing => {
                                    ui.add(egui::TextEdit::multiline(value)
                                        .desired_rows(3).desired_width(320.)
                                        .hint_text(tr!("地图列表显示的文字说明")))
                                        .accessible_name(label);
                                }
                                Kind::Speed => speed(ui, value, label),
                                _ if field.key == "GameMode" => modes(ui, value, scene, label),
                                _ if matches!(field.key, "Player" | "HomeCell" | "AltHomeCell") => {
                                    let kind = if field.key == "Player" { ParameterKind::Reference(mission::Kind::House) }
                                        else { ParameterKind::Waypoint { letters: false } };
                                    let choices = crate::mission_ui::choices::get(ui.ctx(), kind, doc, scene);
                                    crate::completion::picker(ui, ui.make_persistent_id((field.section,field.key)), value, &choices, label);
                                }
                                _ if field.group == Group::Media => {
                                    let resource = if field.key == "Theme" { "Theme" } else { "Movie" };
                                    let choices = scene.mission_resources.choices.get(resource).map_or(&[][..], Vec::as_slice);
                                    crate::completion::picker(ui, ui.make_persistent_id((field.section,field.key)), value, choices, label);
                                }
                                Kind::Integer(min, max) => integer(ui, field.key, value, label, min, max),
                                Kind::Fraction => fraction(ui, value, label),
                                _ => {
                                        if map_file(ui, value, label)
                                            && let Some(path) = rfd::FileDialog::new().add_filter(tr!("地图"), &["map", "yrm", "mpr"]).pick_file()
                                            && let Some(name) = path.file_name()
                                        { *value = name.to_string_lossy().into_owned(); }
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
                section.header_response.on_hover_text(help);
            });
        ui.add_space(4.);
    }
    ui.small(tr!(
        "结束输入后自动生效，可撤销；Ctrl+S 保存。选择“使用默认值”会删除对应覆盖。"
    ));
    if let Err(error) = draft.validate(original) {
        ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
    }
}

fn field_label(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.add(egui::Label::new(label).wrap()),
    )
    .inner
}

fn identity_fields(ui: &mut egui::Ui, identity: &mut IdentityFields<'_>, label_width: f32) {
    field_label(ui, tr!("地图名称"), label_width).on_hover_text(tr!(
        "修改地图显示名称，支持中文；按回车或移开焦点后自动生效，可撤销。Ctrl+S 保存地图。"
    ));
    ui.vertical(|ui| {
        ui.add_sized(
            [320., ui.spacing().interact_size.y],
            egui::TextEdit::singleline(identity.name),
        )
        .accessible_name(tr!("地图名称"));
        if let Err(error) = ra2_map_document::new_map::validate_title(identity.name) {
            ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
        }
    });
    ui.end_row();
    field_label(ui, tr!("作者署名"), label_width).on_hover_text(tr!("作者署名（可留空）"));
    ui.vertical(|ui| {
        ui.add_sized(
            [320., ui.spacing().interact_size.y],
            egui::TextEdit::singleline(identity.author),
        )
        .accessible_name(tr!("作者署名"));
        if let Err(error) = ra2_map_document::new_map::validate_author(identity.author) {
            ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
        }
    });
    ui.end_row();
    field_label(ui, tr!("出生点数量"), label_width).on_hover_text(tr!("支持 2～30 个出生点，对应从 0 开始的连续航点。增加时补齐缺少的航点，保留已有位置；减少时删除多余出生点，其他任务航点保留。"));
    crate::form_edit::number(
        ui,
        egui::DragValue::new(identity.players).range(2..=ra2_map_document::new_map::MAX_PLAYERS),
        tr!("出生点数量"),
    );
    ui.end_row();
}

fn map_file(ui: &mut egui::Ui, value: &mut String, label: &str) -> bool {
    let text = tr!("选择地图…");
    let button_width = (ui
        .painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x
        + ui.spacing().button_padding.x * 2.)
        .max(90.);
    let text_width =
        (ui.available_width() - button_width - ui.spacing().item_spacing.x).clamp(100., 200.);
    ui.horizontal(|ui| {
        let input = ui
            .add_sized(
                egui::vec2(text_width, ui.spacing().interact_size.y),
                egui::TextEdit::singleline(value).hint_text(tr!("使用默认值")),
            )
            .accessible_name(label);
        ui.add_sized(
            egui::vec2(button_width, input.rect.height()),
            egui::Button::new(text).wrap_mode(egui::TextWrapMode::Extend),
        )
        .clicked()
    })
    .inner
}

fn default_toggle(ui: &mut egui::Ui, value: &mut String, initial: &str, label: &str) -> bool {
    let mut inherit = value.trim().is_empty();
    if ui
        .checkbox(&mut inherit, tr!("使用默认值"))
        .accessible_name(tr!("使用默认值：{field}", field = label))
        .changed()
    {
        *value = if inherit {
            String::new()
        } else {
            initial.to_owned()
        };
    }
    inherit
}

fn integer(ui: &mut egui::Ui, key: &str, value: &mut String, label: &str, min: i32, max: i32) {
    let defaults = vera20k_core::sim::game_options::GameOptions::default();
    let initial = match key {
        "Money" => defaults.starting_credits,
        "UnitCount" => defaults.unit_count,
        "TechLevel" => defaults.tech_level,
        "MinPlayers" => 2,
        _ => 0,
    }
    .clamp(min, max);
    ui.horizontal_wrapped(|ui| {
        let inherit = default_toggle(ui, value, &initial.to_string(), label);
        // A legacy value stays intact until the user deliberately replaces it.
        let parsed = value.trim().parse::<i32>();
        if !inherit && parsed.is_err() {
            ui.add(egui::TextEdit::singleline(value).desired_width(140.))
                .accessible_name(label);
        } else {
            let mut number = parsed.unwrap_or(initial);
            ui.add_enabled_ui(!inherit, |ui| {
                if key == "CarryOverCap" {
                    let mut unlimited = number == -1;
                    if ui.checkbox(&mut unlimited, tr!("不限")).changed() {
                        number = if unlimited { -1 } else { 0 };
                        *value = number.to_string();
                    }
                }
                let enabled = key != "CarryOverCap" || number != -1;
                ui.add_enabled_ui(enabled, |ui| {
                    let speed = if matches!(key, "Money" | "CarryOverCap") {
                        100.
                    } else {
                        1.
                    };
                    if crate::form_edit::number(
                        ui,
                        egui::DragValue::new(&mut number)
                            .range(min.max(0)..=max)
                            .clamp_existing_to_range(false)
                            .speed(speed),
                        label,
                    )
                    .changed()
                    {
                        *value = number.to_string();
                    }
                });
            });
        }
    });
}

fn fraction(ui: &mut egui::Ui, value: &mut String, label: &str) {
    ui.horizontal_wrapped(|ui| {
        let inherit = default_toggle(ui, value, "0", label);
        let parsed = value
            .trim()
            .trim_end_matches('%')
            .parse::<f64>()
            .ok()
            .map(|number| {
                if value.trim().ends_with('%') {
                    number
                } else {
                    number * 100.
                }
            });
        if !inherit && parsed.is_none_or(|n| !n.is_finite()) {
            ui.add(egui::TextEdit::singleline(value).desired_width(140.))
                .accessible_name(label);
        } else {
            let mut percent = parsed.unwrap_or(0.);
            ui.add_enabled_ui(!inherit, |ui| {
                if crate::form_edit::number(
                    ui,
                    egui::DragValue::new(&mut percent)
                        .range(0.0..=100.)
                        .clamp_existing_to_range(false)
                        .speed(1.)
                        .suffix("%"),
                    label,
                )
                .changed()
                {
                    *value = format!("{percent}%");
                }
            });
        }
    });
}

fn speed(ui: &mut egui::Ui, value: &mut String, label: &str) {
    let selected = value
        .parse::<u8>()
        .ok()
        .and_then(|speed| {
            crate::editing::SPEEDS
                .iter()
                .find(|(_, number)| *number == speed)
                .map(|(name, _)| crate::i18n::text(name))
        })
        .unwrap_or_else(|| {
            if value.is_empty() {
                tr!("使用默认值")
            } else {
                value.as_str()
            }
        });
    egui::ComboBox::from_id_salt("map-default-speed")
        .height(300.)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), tr!("使用默认值"));
            for (name, speed) in crate::editing::SPEEDS {
                ui.selectable_value(value, speed.to_string(), crate::i18n::text(name));
            }
        })
        .response
        .accessible_name(label);
}

fn modes(ui: &mut egui::Ui, value: &mut String, scene: &MapScene, label: &str) {
    let choices = scene
        .mission_resources
        .choices
        .get("GameMode")
        .map_or(&[][..], Vec::as_slice);
    ui.vertical(|ui| {
        let mut selected: Vec<_> = value
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect();
        let title = if selected.is_empty() {
            tr!("使用默认值").to_owned()
        } else {
            tr!("已选 {count} 项", count = selected.len())
        };
        egui::ComboBox::from_id_salt("map-game-modes")
            .width(235.)
            .selected_text(title)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(selected.is_empty(), tr!("使用默认值"))
                    .clicked()
                {
                    value.clear();
                    selected.clear();
                }
                for (id, name) in choices {
                    let mut checked = selected.iter().any(|v| v.eq_ignore_ascii_case(id));
                    if ui
                        .checkbox(&mut checked, format!("{name} · {id}"))
                        .changed()
                    {
                        selected.retain(|v| !v.eq_ignore_ascii_case(id));
                        if checked {
                            selected.push(id.clone());
                        }
                        *value = selected.join(",");
                    }
                }
                for id in selected.iter().filter(|id| {
                    !choices
                        .iter()
                        .any(|(known, _)| known.eq_ignore_ascii_case(id))
                }) {
                    ui.label(tr!("自定义模式：{id}", id = id));
                }
            })
            .response
            .accessible_name(label);
        egui::CollapsingHeader::new(tr!("自定义模式…"))
            .id_salt("map-custom-modes")
            .show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(value).desired_width(235.))
                    .accessible_name(label);
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable as _;

    #[test]
    fn numeric_controls_keep_defaults_and_legacy_values_and_encode_percentages() {
        let mut values = [String::new(), "0.5".into(), "-1".into(), "22".into()];
        let original = values.clone();
        let mut harness = egui_kittest::Harness::<[String; 4]>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .build_ui_state(
                |ui, values| {
                    ui.push_id("money", |ui| {
                        integer(ui, "Money", &mut values[0], "开局资金默认值", 0, i32::MAX)
                    });
                    ui.push_id("fraction", |ui| {
                        fraction(ui, &mut values[1], "继承资金比例")
                    });
                    ui.push_id("cap", |ui| {
                        integer(
                            ui,
                            "CarryOverCap",
                            &mut values[2],
                            "继承资金上限",
                            -1,
                            i32::MAX,
                        )
                    });
                    ui.push_id("tech", |ui| {
                        integer(ui, "TechLevel", &mut values[3], "科技等级默认值", 0, 10)
                    });
                },
                values,
            );
        harness.run();
        assert_eq!(
            harness.state(),
            &original,
            "opening must not clamp legacy values or write defaults"
        );
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "继承资金比例")
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "继承资金比例")
            .type_text("25");
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state()[1], "25%");
        harness.get_by_label("使用默认值：继承资金比例").click();
        harness.run();
        assert_eq!(harness.state()[1], "");
        harness.get_by_label("使用默认值：开局资金默认值").click();
        harness.run();
        assert_eq!(
            harness.state()[0],
            vera20k_core::sim::game_options::GameOptions::default()
                .starting_credits
                .to_string()
        );
        harness.get_by_label("不限").click();
        harness.run();
        assert_eq!(harness.state()[2], "0");
        values = harness.state().clone();
        assert_eq!(values[3], "22");
    }

    #[test]
    fn mode_and_speed_pickers_preserve_custom_modes_and_store_engine_values() {
        let mut scene = crate::editing::tests::terrain_scene();
        let mut resources = ra2_map_assets::mission_resources::MissionResources::default();
        resources.choices.insert(
            "GameMode".into(),
            vec![
                ("standard".into(), "作战".into()),
                ("teamgame".into(), "团队".into()),
            ],
        );
        scene.mission_resources = std::sync::Arc::new(resources);
        let mut harness = egui_kittest::Harness::<[String; 2]>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .build_ui_state(
                |ui, values| {
                    modes(ui, &mut values[0], &scene, "适用游戏模式");
                    speed(ui, &mut values[1], "游戏速度默认值");
                },
                ["custom-mode".into(), String::new()],
            );
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "适用游戏模式")
            .click();
        harness.run();
        harness.get_by_label("作战 · standard").click();
        harness.run();
        harness.get_by_label("团队 · teamgame").click();
        harness.run();
        assert_eq!(harness.state()[0], "custom-mode,standard,teamgame");
        harness.key_press(egui::Key::Escape);
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "游戏速度默认值")
            .click();
        harness.run();
        harness.get_by_label("90 TPS").scroll_to_me();
        harness.run();
        harness.get_by_label("90 TPS").click();
        harness.run();
        assert_eq!(harness.state()[1], "8");
    }

    #[test]
    fn map_information_boolean_controls_preserve_defaults_until_selected() {
        let (editor, _, root) = crate::editing::tests::editor();
        let draft = Draft::read(editor.doc.as_ref().unwrap());
        let original = draft.clone();
        let scene = crate::editing::tests::terrain_scene();
        let doc = editor.doc.as_ref().unwrap();
        let mut name = doc.ini_text("Basic", "Name");
        let mut author = doc.ini_text("Basic", "Author");
        let mut players = 2;
        let mut harness = egui_kittest::Harness::<(Draft, Draft)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 1000.])
            .build_ui_state(
                |ui, (draft, original)| {
                    show(
                        ui,
                        draft,
                        original,
                        doc,
                        &scene,
                        IdentityFields {
                            name: &mut name,
                            author: &mut author,
                            players: &mut players,
                        },
                    )
                },
                (draft, original.clone()),
            );
        #[cfg(feature = "ui-test-video")]
        {
            harness.get_by_label("基本信息").click();
            harness.run();
            harness.get_by_label("单人任务与战役流程").click();
            harness.run();
            let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/choose-map-button");
            std::fs::create_dir_all(&output).unwrap();
            harness
                .render()
                .unwrap()
                .save(output.join("campaign.png"))
                .unwrap();
            harness.get_by_label("单人任务与战役流程").click();
            harness.run();
        }
        harness.get_by_label("资源与场景规则").click();
        harness.run();
        assert_eq!(harness.state().0, original);
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "免费雷达")
            .click();
        harness.run();
        harness.get_by_label("开启").click();
        harness.run();
        assert_eq!(harness.state().0.values[&("Basic", "FreeRadar")], "yes");
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "免费雷达")
            .click();
        harness.run();
        harness.get_by_label("使用默认值").click();
        harness.run();
        assert_eq!(harness.state().0, original);
        harness.get_by_label("资源与场景规则").click();
        harness.run();
        harness.get_by_label("多人开局默认值").click();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "开局有基地车")
            .click();
        harness.run();
        harness.get_by_label("关闭").click();
        harness.run();
        assert_eq!(
            harness.state().0.values[&("MultiplayerDialogSettings", "Bases")],
            "no"
        );
        #[cfg(feature = "ui-test-video")]
        {
            let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/choose-map-button");
            harness
                .render()
                .unwrap()
                .save(output.join("multiplayer.png"))
                .unwrap();
        }
        harness
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "开局有基地车")
            .click();
        harness.run();
        harness.get_by_label("使用默认值").click();
        harness.run();
        assert_eq!(
            harness.state().0.values[&("MultiplayerDialogSettings", "Bases")],
            ""
        );
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }
}
