//! Play setup stays visible; optional economy/rules settings can be expanded.
use super::*;
use crate::ui_theme as theme;
use ra2_map_io::playtest::{PLAYER_COLOR_COUNT, PlayOptions, player_start_indices};

const COUNTRIES: [&str; 10] = [
    "美国",
    "韩国",
    "法国",
    "德国",
    "英国",
    "利比亚",
    "伊拉克",
    "古巴",
    "苏俄",
    "尤里",
];
// Spawn GameSpeed is reversed: 0 fastest, 6 slowest. Present slow -> fast.
const SPEEDS: [&str; 7] = ["最慢", "更慢", "慢", "正常", "快", "更快", "最快"];

/// Query current document records, including pending edits, for launch starts.
fn map_starts(doc: &MapDocument) -> Vec<u8> {
    let starts: BTreeSet<_> = doc
        .records()
        .values()
        .filter_map(|r| {
            let slot = r.key.parse::<u8>().ok()?;
            (r.section == "Waypoints"
                && slot < crate::new_map::MAX_PLAYERS
                && r.key == slot.to_string()
                && r.cell()
                    .is_some_and(|at| at != [0, 0] && doc.terrain(at).is_some()))
            .then_some(slot)
        })
        .collect();
    player_start_indices(doc.ini_text("Basic", "MaxPlayers").parse().ok(), starts)
}

fn country(ui: &mut egui::Ui, id: &str, label: &str, value: &mut u8) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(COUNTRIES[usize::from(*value).min(9)])
            .show_ui(ui, |ui| {
                for (i, name) in COUNTRIES.iter().enumerate() {
                    ui.selectable_value(value, i as u8, *name);
                }
            });
    });
}

fn speed(ui: &mut egui::Ui, options: &mut PlayOptions) {
    let mut position = 6 - options.speed.min(6);
    ui.horizontal(|ui| {
        ui.label("游戏速度");
        if ui
            .add(
                egui::Slider::new(&mut position, 0..=6)
                    .custom_formatter(|n, _| SPEEDS[(n as usize).min(6)].to_owned()),
            )
            .changed()
        {
            options.speed = 6 - position;
        }
    });
    ui.small("向右加快；最快档不限制游戏帧率。");
}

impl Editor {
    pub fn playtest_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("配置本次试玩，使用当前地图（包含未保存的修改）。");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.play_options.mission, false, "遭遇战 · 配置电脑");
            ui.selectable_value(&mut self.play_options.mission, true, "单人任务");
        });
        ui.separator();
        let starts = self.doc.as_ref().map(map_starts).unwrap_or_default();
        let capacity = starts.len().saturating_sub(1) as u8;
        let mut invalid = None;
        egui::ScrollArea::vertical()
            .id_salt("playtest-options")
            .auto_shrink([false, false])
            .max_height((ui.available_height() - 128.).max(140.))
            .show(ui, |ui| {
                let o = &mut self.play_options;
                if o.mission {
                    ui.strong("任务难度");
                    ui.horizontal_wrapped(|ui| {
                        for (i, label) in ["简单", "普通", "困难"].iter().enumerate() {
                            ui.selectable_value(&mut o.mission_difficulty, i as u8, *label);
                        }
                    });
                    ui.small("电脑阵营、部队和资金由地图任务定义；选择遭遇战可设置电脑对手。");
                    if let Some(doc) = &self.doc {
                        let player = doc.ini_text("Basic", "Player");
                        if !ra2_map_document::mission::ids(
                            doc,
                            ra2_map_document::mission::Kind::House,
                        )
                        .contains(&player)
                        {
                            invalid =
                                Some("单人任务需要在任务制作中设置有效的玩家 House。".to_owned());
                        }
                    }
                } else {
                    ui.strong("玩家与电脑");
                    country(ui, "playtest-player-country", "玩家国家", &mut o.side);
                    ui.horizontal(|ui| {
                        ui.label("玩家起点");
                        egui::ComboBox::from_id_salt("playtest-start")
                            .selected_text(format!("航点 {}", o.start))
                            .show_ui(ui, |ui| {
                                for start in &starts {
                                    ui.selectable_value(
                                        &mut o.start,
                                        *start,
                                        format!("航点 {start}"),
                                    );
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("电脑数量");
                        egui::ComboBox::from_id_salt("playtest-ai-count")
                            .selected_text(if o.ai == 0 {
                                "不添加电脑".into()
                            } else {
                                format!("{} 个电脑", o.ai)
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut o.ai, 0, "不添加电脑");
                                for count in 1..=capacity {
                                    ui.selectable_value(
                                        &mut o.ai,
                                        count,
                                        format!("{count} 个电脑"),
                                    );
                                }
                            });
                    });
                    ui.add_enabled_ui(o.ai > 0, |ui| {
                        country(ui, "playtest-ai-country", "电脑国家", &mut o.ai_side);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("电脑难度");
                            // Native HouseHandicaps: 0 hard, 1 normal, 2 easy.
                            for (value, label) in [(2, "简单"), (1, "普通"), (0, "困难")] {
                                ui.selectable_value(&mut o.difficulty, value, label);
                            }
                        });
                    });
                    ui.small(format!(
                        "地图可用玩家起点 {} 个，最多添加 {capacity} 个电脑（总人数最多 30 人）。",
                        starts.len()
                    ));
                    ui.small("电脑共用上述国家和难度。");
                    if o.ai >= PLAYER_COLOR_COUNT {
                        ui.small("超过 8 人时循环使用现有 8 种玩家颜色。");
                    }
                    if !starts.contains(&o.start) {
                        invalid = Some("请选择地图上已定义的玩家起点（航点 0～29）。".to_owned());
                    } else if o.ai > capacity {
                        invalid = Some(format!(
                            "当前地图最多容纳 {capacity} 个电脑，请减少电脑数量。"
                        ));
                    }
                }
                ui.separator();
                speed(ui, o);
                if !o.mission {
                    egui::CollapsingHeader::new("资金、单位与对局规则").show(ui, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut o.credits)
                                .range(0..=1_000_000)
                                .prefix("初始资金  "),
                        );
                        ui.add(egui::Slider::new(&mut o.units, 0..=50).text("初始单位"));
                        ui.add(egui::Slider::new(&mut o.tech, 0..=10).text("科技等级"));
                        ui.add(
                            egui::Slider::new(&mut o.color, 0..=PLAYER_COLOR_COUNT - 1)
                                .text("玩家颜色"),
                        );
                        ui.add(egui::DragValue::new(&mut o.seed).prefix("随机种子  "));
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut o.superweapons, "超级武器");
                            ui.checkbox(&mut o.crates, "随机箱子");
                            ui.checkbox(&mut o.short_game, "快速游戏");
                        });
                    });
                }
                ui.collapsing("游戏程序", |ui| {
                    if theme::path_entry(
                        ui,
                        &mut self.executable,
                        "vera20k.exe 的完整路径",
                        "选择…",
                    ) && let Some(path) = rfd::FileDialog::new()
                        .add_filter("VERA20k", &["exe"])
                        .pick_file()
                    {
                        self.executable = path.display().to_string();
                    }
                });
            });
        if !Path::new(&self.executable).is_file() {
            invalid = Some("请在“游戏程序”中选择 vera20k.exe。".into());
        }
        if self.doc.is_none() {
            invalid = Some("请先打开或新建可编辑地图。".into());
        }
        if let Some(message) = &invalid {
            ui.colored_label(Color32::YELLOW, message);
        }
        ui.separator();
        if ui
            .add_enabled(
                invalid.is_none() && !self.launching && self.process.is_none() && !self.stroking,
                theme::primary(if self.launching {
                    "正在准备试玩…"
                } else {
                    "启动试玩"
                }),
            )
            .clicked()
        {
            self.play();
        }
        if let Some(process) = &mut self.process {
            let folder = format!("日志：{}", process.folder.display());
            ui.add(egui::Label::new(&folder).truncate())
                .on_hover_text(&folder);
            if ui.button("停止本次试玩").clicked()
                && let Err(e) = process.child.kill()
            {
                self.status = e.to_string();
            }
        }
        if !self.status.is_empty() {
            ui.add(egui::Label::new(&self.status).truncate())
                .on_hover_text(&self.status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_thirty_map_starts_are_available_for_direct_launch() {
        let bytes = crate::new_map::NewMapOptions {
            players: 30,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc =
            MapDocument::new(crate::document::Baseline::parse(bytes.into(), &map).unwrap());
        assert_eq!(map_starts(&doc), (0..30).collect::<Vec<_>>());
        assert_eq!(
            ra2_map_io::playtest::map_player_starts(&map),
            map_starts(&doc)
        );
        // Maps from the previous editor lacked the explicit declaration.
        doc.set_ini("Basic", "MaxPlayers", None).unwrap();
        assert_eq!(map_starts(&doc).len(), 30);
        let id = *doc
            .records()
            .iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == "8")
            .unwrap()
            .0;
        doc.set(Target::Record(id), None).unwrap();
        assert_eq!(map_starts(&doc).len(), 8);
    }

    #[test]
    fn selectable_starts_follow_pending_waypoint_edits_and_history() {
        let bytes = crate::new_map::NewMapOptions {
            players: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc =
            MapDocument::new(crate::document::Baseline::parse(bytes.into(), &map).unwrap());
        assert_eq!(map_starts(&doc), vec![0, 1, 2, 3]);
        let (&id, _) = doc
            .records()
            .iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == "1")
            .unwrap();
        doc.set(Target::Record(id), None).unwrap();
        doc.waypoint(7, [80, 80]).unwrap();
        doc.waypoint(8, [81, 80]).unwrap();
        doc.commit("edit starts");
        assert_eq!(map_starts(&doc), vec![0, 2, 3, 7]);
        doc.undo();
        assert_eq!(map_starts(&doc), vec![0, 1, 2, 3]);
        doc.redo();
        assert_eq!(map_starts(&doc), vec![0, 2, 3, 7]);
    }
}
