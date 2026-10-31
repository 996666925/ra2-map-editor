use super::*;
use crate::ui_theme as theme;
use ra2_map_document::random_map::{ENVIRONMENTS, LANDSCAPES, SIZES};

fn fresh_seed() -> i32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mut mixed = time.wrapping_add(
        SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9e3779b97f4a7c15),
    );
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((mixed ^ (mixed >> 31)) & 0xffff) as i32
}

fn choice(ui: &mut egui::Ui, label: &str, value: &mut i32, labels: &[&str]) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .width(215.)
        .selected_text(labels.get(*value as usize).copied().unwrap_or("请选择"))
        .show_ui(ui, |ui| {
            for (index, label) in labels.iter().enumerate() {
                ui.selectable_value(value, index as i32, *label);
            }
        });
    ui.end_row();
}

impl MapView {
    pub(super) fn show_random_map(&mut self) {
        if self.pending.is_none() {
            self.workbench.new_map = false;
            self.workbench.random_options.terrain.seed = fresh_seed();
            self.workbench.random_map = true;
        }
    }

    pub(super) fn random_map_window(&mut self, ctx: &egui::Context, directory: &mut String) {
        if !self.workbench.random_map {
            return;
        }
        let mut create = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("random-map")).show(ctx, |ui| {
            ui.set_width(490.0_f32.min((ctx.content_rect().width() - 64.).max(240.)));
            // Area remembers its previous size; bound it again after a native resize.
            ui.set_max_height((ctx.content_rect().height() - 64.).max(120.));
            ui.heading("新建随机地图");
            ui.label("生成包含自然地貌、玩家起点和资源的遭遇战地图。");
            ui.separator();
            let mut valid = false;
            egui::ScrollArea::vertical().id_salt("random-map-form")
                .max_height((ctx.content_rect().height() - 185.).max(120.))
                .show(ui, |ui| {
                    ui.label("游戏资源目录");
                    if theme::path_entry(ui, directory, "包含 ra2.mix 与 ra2md.mix 的目录", "浏览…")
                        && let Some(path) = rfd::FileDialog::new().pick_folder() {
                        *directory = path.display().to_string();
                    }
                    let options = &mut self.workbench.random_options;
                    ui.label("地图名称");
                    ui.add(egui::TextEdit::singleline(&mut options.title).desired_width(f32::INFINITY));
                    egui::Grid::new("random-map-fields").num_columns(2).spacing([20., 8.]).show(ui, |ui| {
                        let t = &mut options.terrain;
                        choice(ui, "地图环境", &mut t.theater, &ENVIRONMENTS);
                        let before = t.map_type;
                        choice(ui, "地貌类型", &mut t.map_type, &LANDSCAPES);
                        if before != t.map_type {
                            // Useful starting values; every slider remains editable.
                            (t.water_amount, t.ruggedness) = match t.map_type {
                                0 => (65, 10), 1 => (40, 20), 2 => (60, 15),
                                3 => (20, 20), _ => (10, 70),
                            };
                        }
                        choice(ui, "宽度档位", &mut t.width, &SIZES);
                        choice(ui, "高度档位", &mut t.height, &SIZES);
                        ui.label("玩家起点");
                        ui.add(egui::DragValue::new(&mut t.num_players).range(2..=i32::from(crate::new_map::MAX_PLAYERS)).suffix(" 个"));
                        ui.end_row();
                        for (label, value) in [
                            ("水域规模", &mut t.water_amount),
                            ("地形崎岖度", &mut t.ruggedness),
                            ("通行开阔度", &mut t.accessibility),
                            ("植被密度", &mut t.vegetation),
                            ("起点分散程度", &mut t.tiberium_layout),
                        ] {
                            ui.label(label);
                            ui.add(egui::Slider::new(value, 0..=100));
                            ui.end_row();
                        }
                        ui.label("矿区");
                        ui.checkbox(&mut options.ore, "生成矿石资源");
                        ui.end_row();
                        ui.label("资源丰度");
                        ui.add_enabled(options.ore, egui::Slider::new(&mut t.tiberium, 1..=100));
                        ui.end_row();
                        ui.label("宝石");
                        let mut gems = t.resources == 3;
                        if ui.add_enabled(options.ore, egui::Checkbox::new(&mut gems, "矿区包含宝石")).changed() {
                            t.resources = if gems { 3 } else { 1 };
                        }
                        ui.end_row();
                        ui.label("中立科技建筑");
                        ui.checkbox(&mut options.tech_buildings, "按游戏规则随机放置");
                        ui.end_row();
                        ui.label("随机种子");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut t.seed).range(0..=65535));
                            if ui.button("换一个").clicked() { t.seed = fresh_seed(); }
                        });
                        ui.end_row();
                    });
                    let (width, height) = options.dimensions();
                    theme::field(ui, "实际地图尺寸", format!("{width} × {height} 格（含边界）"));
                    ui.small("尺寸由档位和玩家数共同决定。水量等数值表示生成强度，并非实际占地百分比。");
                    ui.small("相同资源、参数和种子可复现地图；每次打开此窗口会换一个种子。生成后可继续编辑。");
                    let validation = options.validate();
                    valid = validation.is_ok() && !directory.trim().is_empty();
                    if let Err(error) = validation { ui.colored_label(theme::ACCENT, error.to_string()); }
                    if directory.trim().is_empty() { ui.colored_label(theme::ACCENT, "请先选择游戏资源目录。"); }
                });
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                create = ui.add_enabled(valid, theme::primary("生成地图")).clicked();
                cancel = ui.button("取消").clicked();
            });
        });
        if create {
            self.workbench.random_map = false;
            self.create_map(
                PathBuf::from(directory.trim()),
                MapCreation::Random(self.workbench.random_options.clone()),
            );
        } else if cancel || modal.should_close() {
            self.workbench.random_map = false;
        }
    }
}
