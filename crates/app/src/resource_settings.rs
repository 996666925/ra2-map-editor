//! Map resource settings: earlier rows have higher priority.
use crate::i18n::tr;
use eframe::egui;
use ra2_map_assets::map_resources::{AresMode, INI_ENTRIES, MapResources, PathBase, ResourcePath};

pub(crate) fn show(ui: &mut egui::Ui, config: &mut MapResources) -> anyhow::Result<()> {
    if ui.button(tr!("导入资源配置…")).clicked()
        && let Some(path) = rfd::FileDialog::new()
            .add_filter(tr!("资源配置"), &["json"])
            .pick_file()
    {
        *config = MapResources::import(&path)?;
    }
    list(ui, tr!("资源目录"), &mut config.directories, false);
    ui.add_space(8.);
    list(ui, tr!("资源归档（MIX）"), &mut config.archives, true);
    ui.add_space(8.);
    ui.collapsing(tr!("高优先级和回退目录"), |ui| {
        list(ui, tr!("高优先级目录"), &mut config.high_priority, false);
        list(ui, tr!("低优先级目录"), &mut config.low_priority, false);
        ui.small(tr!(
            "完整顺序：高优先级目录 → 额外目录 → 游戏散文件 → 额外 MIX → 游戏归档 → 低优先级目录。"
        ));
    });
    ui.collapsing(tr!("INI 入口"), |ui| {
        for (key, default) in INI_ENTRIES {
            ui.horizontal(|ui| {
                let label = ui.label(entry_label(key));
                let mut name = config
                    .ini
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(*default)
                            .desired_width(ui.available_width()),
                    )
                    .labelled_by(label.id)
                    .changed()
                {
                    config.ini.retain(|k, _| !k.eq_ignore_ascii_case(key));
                    if !name.trim().is_empty() {
                        config.ini.insert((*key).into(), name.trim().into());
                    }
                }
            });
        }
        ui.small(tr!("入口文件替换对应的默认 INI；留空使用原版文件名。"));
    });
    ui.horizontal(|ui| {
        let label = ui.label(tr!("Ares 模式"));
        egui::ComboBox::from_id_salt("resource-ares")
            .selected_text(mode(config.ares))
            .show_ui(ui, |ui| {
                for value in [AresMode::Auto, AresMode::On, AresMode::Off] {
                    ui.selectable_value(&mut config.ares, value, mode(value));
                }
            })
            .response
            .labelled_by(label.id);
    });
    if ui.button(tr!("清空资源配置")).clicked() {
        *config = MapResources::default();
    }
    Ok(())
}
fn entry_label(key: &str) -> &'static str {
    match key {
        "rules" => tr!("规则 INI"),
        "art" => tr!("美术 INI"),
        "ai" => tr!("AI INI"),
        "sound" => tr!("音效 INI"),
        "eva" => tr!("语音 INI"),
        "theme" => tr!("音乐 INI"),
        "temperate" => tr!("温带 INI"),
        "snow" => tr!("雪地 INI"),
        "urban" => tr!("城市 INI"),
        "newurban" => tr!("新城市 INI"),
        "desert" => tr!("沙漠 INI"),
        "lunar" => tr!("月球 INI"),
        _ => unreachable!("unknown resource entry"),
    }
}

fn mode(value: AresMode) -> &'static str {
    match value {
        AresMode::Auto => tr!("自动"),
        AresMode::On => tr!("启用"),
        AresMode::Off => tr!("关闭"),
    }
}
fn list(ui: &mut egui::Ui, title: &str, items: &mut Vec<ResourcePath>, mix: bool) {
    ui.push_id(title, |ui| {
        ui.strong(title);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if mix {
                    tr!("添加 MIX…")
                } else {
                    tr!("添加目录…")
                })
                .clicked()
            {
                let dialog = rfd::FileDialog::new();
                let path = if mix {
                    dialog.add_filter("MIX", &["mix"]).pick_file()
                } else {
                    dialog.pick_folder()
                };
                if let Some(path) = path {
                    items.push(ResourcePath::map(path));
                }
            }
            if ui.button(tr!("手动输入路径")).clicked() {
                items.push(ResourcePath::default());
            }
        });
        let mut action = None;
        let count = items.len();
        for (index, item) in items.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.group(|ui| {
                    let mut path = item.path.display().to_string();
                    if ui
                        .add_sized(
                            [ui.available_width(), ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut path)
                                .id_salt("path")
                                .hint_text(tr!("素材路径")),
                        )
                        .changed()
                    {
                        item.path = path.into();
                    }
                    ui.horizontal_wrapped(|ui| {
                        let label = ui.label(tr!("路径基准"));
                        egui::ComboBox::from_id_salt("base")
                            .width(92.)
                            .selected_text(base(item.base))
                            .show_ui(ui, |ui| {
                                for value in [PathBase::Map, PathBase::Game, PathBase::Editor] {
                                    ui.selectable_value(&mut item.base, value, base(value));
                                }
                            })
                            .response
                            .labelled_by(label.id);
                        if ui.button(tr!("浏览…")).clicked() {
                            let dialog = rfd::FileDialog::new();
                            let selected = if mix {
                                dialog.add_filter("MIX", &["mix"]).pick_file()
                            } else {
                                dialog.pick_folder()
                            };
                            if let Some(path) = selected {
                                item.path = path;
                                item.base = PathBase::Map;
                            }
                        }
                        if ui
                            .add_enabled(index > 0, egui::Button::new(tr!("上移")))
                            .clicked()
                        {
                            action = Some((index, -1));
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new(tr!("下移")))
                            .clicked()
                        {
                            action = Some((index, 1));
                        }
                        if ui.button(tr!("移除")).clicked() {
                            action = Some((index, 0));
                        }
                    });
                });
            });
        }
        if let Some((index, delta)) = action {
            if delta == 0 {
                items.remove(index);
            } else {
                items.swap(index, (index as isize + delta) as usize);
            }
        }
    });
}

fn base(value: PathBase) -> &'static str {
    match value {
        PathBase::Map => tr!("地图"),
        PathBase::Game => tr!("游戏"),
        PathBase::Editor => tr!("编辑器"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_buttons_reorder_and_remove_sources_without_touching_files() {
        use egui_kittest::kittest::Queryable as _;
        let config = MapResources {
            directories: vec![ResourcePath::map("first"), ResourcePath::map("second")],
            ..Default::default()
        };
        let mut harness = egui_kittest::Harness::<MapResources>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([490., 900.])
            .build_ui_state(
                |ui, config| {
                    show(ui, config).unwrap();
                },
                config,
            );
        harness.run();
        let add = harness.get_by_label(tr!("添加目录…")).rect();
        let first_rect = harness
            .get(
                egui_kittest::kittest::By::new()
                    .role(egui::accesskit::Role::TextInput)
                    .value("first"),
            )
            .rect();
        assert!(
            add.bottom() < first_rect.top(),
            "add button below source rows: {add:?} / {first_rect:?}"
        );
        let first = harness
            .get(
                egui_kittest::kittest::By::new()
                    .role(egui::accesskit::Role::TextInput)
                    .value("first"),
            )
            .rect();
        let second = harness
            .get(
                egui_kittest::kittest::By::new()
                    .role(egui::accesskit::Role::TextInput)
                    .value("second"),
            )
            .rect();
        assert!(first.width() > 420., "path input squeezed: {first:?}");
        assert!(second.width() > 420., "path input squeezed: {second:?}");
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(490., 900.));
        for label in [
            tr!("上移"),
            tr!("下移"),
            tr!("移除"),
            tr!("浏览…"),
            tr!("手动输入路径"),
        ] {
            for button in harness.get_all_by_label(label) {
                assert!(
                    bounds.contains_rect(button.rect()),
                    "{label} clipped: {:?}",
                    button.rect()
                );
            }
        }
        assert!(
            harness
                .get_all_by_label(tr!("浏览…"))
                .next()
                .unwrap()
                .rect()
                .top()
                >= first.bottom()
        );
        harness
            .get_all_by_label(tr!("下移"))
            .next()
            .unwrap()
            .click();
        harness.run();
        assert_eq!(
            harness.state().directories[0].path,
            std::path::Path::new("second")
        );
        harness.get_by_label(tr!("清空资源配置")).click();
        harness.run();
        assert!(harness.state().is_empty());
    }
}
