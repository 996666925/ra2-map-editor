use crate::i18n::tr;
use eframe::egui;
use ra2_map_document::document::MapDocument;
use std::collections::BTreeMap;

const ROOM_GAME_SPEEDS: [(i32, &str); 7] = [
    (0, "极快"),
    (1, "很快"),
    (2, "快"),
    (3, "中等"),
    (4, "慢"),
    (5, "很慢"),
    (6, "最慢"),
];

const FIELDS: [(&str, &str, i32, i32, i32, bool); 15] = [
    ("GameSpeed", "游戏速度", 1, 0, 6, false),
    ("Credits", "初始资金", 10000, 0, 1_000_000, false),
    ("UnitCount", "初始单位数", 0, 0, 100, false),
    ("TechLevel", "科技等级", 10, 1, 10, false),
    ("ShortGame", "短局", 0, 0, 1, true),
    ("Bases", "开局给基地车", 1, 0, 1, true),
    ("BridgeDestroy", "桥能被炸毁", 1, 0, 1, true),
    ("Superweapons", "超级武器", 1, 0, 1, true),
    ("BuildOffAlly", "能在盟友建筑旁边建造", 0, 0, 1, true),
    ("Crates", "随机箱子", 0, 0, 1, true),
    ("MCVRedeploy", "基地车能重新打包", 1, 0, 1, true),
    ("FogOfWar", "战争迷雾", 0, 0, 1, true),
    ("MultiEngineer", "多工程师占领", 0, 0, 1, true),
    ("HarvesterTruce", "矿车不会被攻击", 0, 0, 1, true),
    ("AlliesAllowed", "游戏里可以结盟", 0, 0, 1, true),
];
const CHECKS: [(&str, &str); 8] = [
    ("chkShortGame", "ShortGame"),
    ("chkRedeplMCV", "MCVRedeploy"),
    ("chkCrates", "Crates"),
    ("chkMultiEng", "MultiEngineer"),
    ("chkIngameAllying", "AlliesAllowed"),
    ("chkDestrBridges", "BridgeDestroy"),
    ("chkSuperWeapons", "Superweapons"),
    ("chkBuildOffAlly", "BuildOffAlly"),
];
const DROPS: [(&str, &str); 3] = [
    ("cmbCredits", "Credits"),
    ("cmbStartingUnits", "UnitCount"),
    ("cmbGameSpeedCap", "GameSpeed"),
];
const CREDIT: [i32; 9] = [100000, 30000, 20000, 15000, 12500, 10000, 7500, 5000, 2500];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Draft {
    defaults: BTreeMap<String, Option<i32>>,
    forced: BTreeMap<String, Option<i32>>,
    original_defaults: BTreeMap<String, Option<i32>>,
    original_forced: BTreeMap<String, Option<i32>>,
}
fn switch(raw: &str) -> Option<i32> {
    match raw.trim().chars().next()?.to_ascii_lowercase() {
        '1' | 'y' | 't' => Some(1),
        '0' | 'n' | 'f' => Some(0),
        _ => None,
    }
}
fn parse_value(raw: &str, field: &(&str, &str, i32, i32, i32, bool)) -> Option<i32> {
    let (_, _, _, min, max, boolean) = *field;
    let value = if boolean {
        switch(raw)
    } else {
        raw.trim().parse().ok()
    }?;
    (min..=max).contains(&value).then_some(value)
}
impl Draft {
    pub fn read(doc: &MapDocument) -> Self {
        let parse_section = |section: &str| {
            let mut out = BTreeMap::new();
            for (_, key, bytes) in doc.ini_entries(section) {
                if let Some(field) = FIELDS.iter().find(|f| f.0.eq_ignore_ascii_case(&key))
                    && let Some(v) = parse_value(&String::from_utf8_lossy(&bytes), field)
                {
                    out.insert(field.0.to_owned(), v);
                }
            }
            out
        };
        let defaults = parse_section("DefaultSpawnIniOptions");
        let mut forced = BTreeMap::new();
        for (control, name) in CHECKS {
            if let Some((_, _, bytes)) = doc
                .ini_entries("ForcedOptions")
                .into_iter()
                .find(|(_, k, _)| k.eq_ignore_ascii_case(control))
                && let Some(v) = switch(&String::from_utf8_lossy(&bytes))
            {
                forced.insert(name.into(), v);
            }
        }
        for (control, name) in DROPS {
            if let Some((_, _, bytes)) = doc
                .ini_entries("ForcedOptions")
                .into_iter()
                .find(|(_, k, _)| k.eq_ignore_ascii_case(control))
                && let Ok(i) = String::from_utf8_lossy(&bytes).trim().parse::<usize>()
            {
                let value = match name {
                    "Credits" => CREDIT.get(i).copied(),
                    "UnitCount" => {
                        if i <= 10 {
                            Some(10 - i as i32)
                        } else {
                            None
                        }
                    }
                    _ => {
                        if i <= 6 {
                            Some(i as i32)
                        } else {
                            None
                        }
                    }
                };
                if let Some(v) = value {
                    forced.insert(name.into(), v);
                }
            }
        }
        for (key, value) in parse_section("ForcedSpawnIniOptions") {
            forced.insert(key, value);
        }
        for name in ["Credits", "UnitCount", "Bases"] {
            let raw = doc.ini_text("Basic", name);
            if (name == "Bases" || raw.trim() != "-1")
                && let Some(field) = FIELDS.iter().find(|f| f.0 == name)
                && let Some(v) = parse_value(&raw, field)
            {
                forced.insert(name.into(), v);
            }
        }
        let mut defaults = defaults
            .into_iter()
            .map(|(k, v)| (k, Some(v)))
            .collect::<BTreeMap<_, _>>();
        let mut forced = FIELDS
            .iter()
            .map(|(k, ..)| (k.to_string(), forced.get(*k).copied()))
            .collect::<BTreeMap<_, _>>();
        let original_defaults = defaults.clone();
        let original_forced = forced.clone();
        // Keep all rows present in each draft so the UI can edit any field.
        for (key, ..) in FIELDS {
            defaults.entry(key.into()).or_insert(None);
            forced.entry(key.into()).or_insert(None);
        }
        Self {
            defaults,
            forced,
            original_defaults,
            original_forced,
        }
    }
    pub fn apply(&self, doc: &mut MapDocument) -> anyhow::Result<()> {
        doc.atomic(|doc| {
            for (key, value) in &self.defaults {
                if self.original_defaults.get(key).copied().flatten() != *value {
                    doc.set_ini(
                        "DefaultSpawnIniOptions",
                        key,
                        value.map(|v| v.to_string().into_bytes()),
                    )?;
                }
            }
            for (key, value) in &self.forced {
                if self.original_forced.get(key).copied().flatten() != *value {
                    doc.set_ini(
                        "ForcedSpawnIniOptions",
                        key,
                        value.map(|v| v.to_string().into_bytes()),
                    )?;
                }
            }
            Ok(())
        })
    }
}
type Field = (&'static str, &'static str, i32, i32, i32, bool);

fn field_label(ui: &mut egui::Ui, label: &str, width: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.add(egui::Label::new(egui::RichText::new(label).strong()).wrap()),
    )
    .inner
}

fn boolean_buttons(ui: &mut egui::Ui, value: &mut i32, label: &str) -> bool {
    let previous = *value != 0;
    let mut on = previous;
    ui.horizontal(|ui| {
        for (state, text) in [(false, tr!("关")), (true, tr!("开"))] {
            ui.selectable_value(&mut on, state, text)
                .accessible_name(format!("{label} · {text}"));
        }
    });
    if on == previous {
        false
    } else {
        *value = i32::from(on);
        true
    }
}

fn room_game_speed_label(value: i32) -> String {
    ROOM_GAME_SPEEDS
        .iter()
        .find(|(speed, _)| *speed == value)
        .map(|(speed, label)| format!("{speed} · {}", crate::i18n::text(label)))
        .unwrap_or_else(|| value.to_string())
}

fn field_card(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    field: Field,
    forced: bool,
    width: f32,
) -> bool {
    let (key, label, default, min, max, boolean) = field;
    let label = crate::i18n::text(label);
    let slot = if forced {
        draft.forced.get_mut(key).unwrap()
    } else {
        draft.defaults.get_mut(key).unwrap()
    };
    let mut changed = false;
    egui::Frame::new()
        .fill(crate::ui_theme::BACKGROUND)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(6)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.set_min_height(58.0);
                let action_width = if forced {
                    72.0
                } else if slot.is_some() {
                    50.0
                } else {
                    0.0
                };
                ui.horizontal(|ui| {
                    field_label(ui, label, (width - action_width).max(80.0)).on_hover_text(key);
                    if forced {
                        let mut locked = slot.is_some();
                        if ui
                            .checkbox(&mut locked, tr!("锁定"))
                            .accessible_name(format!("{label} · {}", tr!("锁定")))
                            .changed()
                        {
                            *slot = locked.then_some(slot.unwrap_or(default));
                            changed = true;
                        }
                    } else if slot.is_some()
                        && ui
                            .small_button(tr!("恢复"))
                            .accessible_name(format!("{label} · {}", tr!("恢复")))
                            .clicked()
                    {
                        *slot = None;
                        changed = true;
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if forced && slot.is_none() {
                        ui.label(egui::RichText::new(tr!("不锁定")).color(crate::ui_theme::MUTED));
                    } else {
                        let mut value = slot.unwrap_or(default);
                        let value_changed = if boolean {
                            boolean_buttons(ui, &mut value, label)
                        } else if key == "GameSpeed" {
                            let previous = value;
                            egui::ComboBox::from_id_salt(("room-option-game-speed", forced))
                                .selected_text(room_game_speed_label(value))
                                .show_ui(ui, |ui| {
                                    for (speed, _) in ROOM_GAME_SPEEDS {
                                        ui.selectable_value(
                                            &mut value,
                                            speed,
                                            room_game_speed_label(speed),
                                        );
                                    }
                                })
                                .response
                                .accessible_name(label);
                            value != previous
                        } else {
                            ui.scope(|ui| {
                                ui.style_mut().spacing.text_edit_width = 96.0;
                                crate::form_edit::number(
                                    ui,
                                    egui::DragValue::new(&mut value).range(min..=max),
                                    label,
                                )
                            })
                            .inner
                            .changed()
                        };
                        if value_changed {
                            *slot = Some(value);
                            changed = true;
                        }
                    }
                    let default_text = if boolean {
                        if default != 0 { tr!("开") } else { tr!("关") }.to_owned()
                    } else if key == "GameSpeed" {
                        room_game_speed_label(default)
                    } else {
                        default.to_string()
                    };
                    ui.small(
                        egui::RichText::new(tr!("引擎默认：{default}", default = default_text))
                            .color(crate::ui_theme::MUTED),
                    );
                    if !boolean && key != "GameSpeed" {
                        ui.small(
                            egui::RichText::new(tr!("范围 {min}–{max}", min = min, max = max))
                                .color(crate::ui_theme::MUTED),
                        );
                    }
                });
            });
        });
    changed
}

fn field_section(
    ui: &mut egui::Ui,
    draft: &mut Draft,
    title: &str,
    fields: &[Field],
    forced: bool,
) -> bool {
    let mut changed = false;
    crate::ui_theme::section_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        crate::ui_theme::section(ui, title, &tr!("{count} 项", count = fields.len()));
        ui.add_space(8.0);
        let gap = 10.0;
        let columns = if ui.available_width() >= 520.0 { 2 } else { 1 };
        let width = ((ui.available_width() - gap * (columns - 1) as f32) / columns as f32 - 22.0)
            .max(120.0);
        egui::Grid::new(("room-option-cards", title, forced))
            .num_columns(columns)
            .spacing([gap, gap])
            .show(ui, |ui| {
                for (index, field) in fields.iter().copied().enumerate() {
                    changed |= field_card(ui, draft, field, forced, width);
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
                if !fields.len().is_multiple_of(columns) {
                    ui.end_row();
                }
            });
    });
    changed
}

pub(super) fn show(ui: &mut egui::Ui, draft: &mut Draft, forced: bool) -> bool {
    let (title, description, color, summary) = if forced {
        let count = draft
            .forced
            .values()
            .filter(|value| value.is_some())
            .count();
        (
            tr!("房间锁定值"),
            tr!("这些设置由地图锁定，房主不能调整。"),
            crate::ui_theme::BLUE,
            tr!(
                "已锁定 {count} / {total} 项",
                count = count,
                total = FIELDS.len()
            ),
        )
    } else {
        let count = draft
            .defaults
            .values()
            .filter(|value| value.is_some())
            .count();
        (
            tr!("房间初始值"),
            tr!("房间开局默认设置，房主可以调整。"),
            crate::ui_theme::ACCENT,
            tr!(
                "已自定义 {count} / {total} 项",
                count = count,
                total = FIELDS.len()
            ),
        )
    };
    crate::ui_theme::page_header(ui, title, description, Some((&summary, color)));
    ui.add_space(10.0);
    let mut changed = field_section(ui, draft, tr!("数值设置"), &FIELDS[..4], forced);
    ui.add_space(10.0);
    changed |= field_section(ui, draft, tr!("游戏规则"), &FIELDS[4..], forced);
    changed
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_room_options_in_launcher_precedence_and_applies_only_edits() {
        let mut doc = ra2_map_document::test_support::document();
        for (section, key, value) in [
            ("DefaultSpawnIniOptions", "GameSpeed", "6"),
            ("DefaultSpawnIniOptions", "Credits", "1000001"),
            ("DefaultSpawnIniOptions", "ShortGame", "true"),
            ("DefaultSpawnIniOptions", "Crates", "yes"),
            ("ForcedOptions", "chkCrates", "false"),
            ("ForcedOptions", "cmbStartingUnits", "10"),
            ("ForcedSpawnIniOptions", "Crates", "true"),
            ("Basic", "Credits", "-1"),
            ("Basic", "UnitCount", "3"),
        ] {
            doc.set_ini(section, key, Some(value.as_bytes().to_vec()))
                .unwrap();
        }

        let mut draft = Draft::read(&doc);
        assert_eq!(draft.defaults["GameSpeed"], Some(6));
        assert_eq!(
            draft.defaults["Credits"], None,
            "out-of-range defaults are ignored"
        );
        assert_eq!(
            draft.defaults["ShortGame"],
            Some(1),
            "switches use their first letter"
        );
        assert_eq!(
            draft.defaults["Crates"],
            Some(1),
            "retain the map default even when locked"
        );
        assert_eq!(
            draft.forced["Crates"],
            Some(1),
            "ForcedSpawnIniOptions wins"
        );
        assert_eq!(
            draft.forced["UnitCount"],
            Some(3),
            "Basic wins over old drop-down"
        );

        draft.defaults.insert("GameSpeed".into(), Some(2));
        draft.apply(&mut doc).unwrap();
        assert_eq!(doc.ini_text("DefaultSpawnIniOptions", "GameSpeed"), "2");
        assert_eq!(doc.ini_text("ForcedSpawnIniOptions", "Crates"), "true");
        assert!(
            doc.ini_value("ForcedSpawnIniOptions", "GameSpeed")
                .is_none()
        );
    }

    #[test]
    fn room_option_cards_edit_defaults_and_locks_without_implicit_writes() {
        use egui_kittest::kittest::Queryable as _;

        let _language = crate::i18n::enter(crate::i18n::Language::Chinese);
        let doc = ra2_map_document::test_support::document();
        let mut draft = Draft::read(&doc);
        draft.defaults.insert("GameSpeed".into(), None);
        draft.defaults.insert("ShortGame".into(), None);
        let mut defaults = egui_kittest::Harness::<Draft>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([760.0, 900.0])
            .build_ui_state(
                |ui, draft| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        show(ui, draft, false);
                    });
                },
                draft,
            );
        defaults.run();
        assert!(
            defaults
                .state()
                .defaults
                .values()
                .all(|value| value.is_none())
        );
        #[cfg(feature = "ui-test-video")]
        {
            let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/room-options-ui");
            std::fs::create_dir_all(&output).unwrap();
            defaults
                .render()
                .unwrap()
                .save(output.join("defaults.png"))
                .unwrap();
        }

        defaults.get_by_label("短局 · 开").click();
        defaults.run();
        assert_eq!(defaults.state().defaults["ShortGame"], Some(1));
        defaults.get_by_label("短局 · 恢复").click();
        defaults.run();
        assert_eq!(defaults.state().defaults["ShortGame"], None);

        defaults
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "游戏速度")
            .click();
        defaults.run();
        defaults.get_by_label("3 · 中等").click();
        defaults.run();
        assert_eq!(defaults.state().defaults["GameSpeed"], Some(3));
        defaults.get_by_label("游戏速度 · 恢复").click();
        defaults.run();
        assert_eq!(defaults.state().defaults["GameSpeed"], None);

        let mut draft = Draft::read(&doc);
        draft.forced.insert("Credits".into(), None);
        draft.forced.insert("ShortGame".into(), None);
        let mut locked = egui_kittest::Harness::<Draft>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([760.0, 900.0])
            .build_ui_state(
                |ui, draft| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        show(ui, draft, true);
                    });
                },
                draft,
            );
        locked.run();
        locked.get_by_label("短局 · 锁定").click();
        locked.run();
        assert_eq!(locked.state().forced["ShortGame"], Some(0));
        locked.get_by_label("短局 · 开").click();
        locked.run();
        assert_eq!(locked.state().forced["ShortGame"], Some(1));
        locked.get_by_label("游戏速度 · 锁定").click();
        locked.run();
        locked
            .get_by_role_and_label(egui::accesskit::Role::ComboBox, "游戏速度")
            .click();
        locked.run();
        locked.get_by_label("6 · 最慢").scroll_to_me();
        locked.run();
        locked.get_by_label("6 · 最慢").click();
        locked.run();
        assert_eq!(locked.state().forced["GameSpeed"], Some(6));
        #[cfg(feature = "ui-test-video")]
        {
            let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/room-options-ui");
            std::fs::create_dir_all(&output).unwrap();
            locked
                .render()
                .unwrap()
                .save(output.join("locked.png"))
                .unwrap();
        }
        locked.get_by_label("短局 · 锁定").click();
        locked.run();
        assert_eq!(locked.state().forced["ShortGame"], None);
        locked.get_by_label("初始资金 · 锁定").click();
        locked.run();
        assert_eq!(locked.state().forced["Credits"], Some(10_000));
    }
}
