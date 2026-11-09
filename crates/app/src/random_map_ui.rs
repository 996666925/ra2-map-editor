use super::*;
use crate::i18n::tr;
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
    ui.label(crate::i18n::text(label));
    egui::ComboBox::from_id_salt(label)
        .width(215.)
        .selected_text(crate::i18n::text(
            labels
                .get(*value as usize)
                .copied()
                .unwrap_or(tr!("请选择")),
        ))
        .show_ui(ui, |ui| {
            for (index, label) in labels.iter().enumerate() {
                ui.selectable_value(value, index as i32, crate::i18n::text(label));
            }
        });
    ui.end_row();
}

fn percentage(ui: &mut egui::Ui, label: &str, value: &mut i32, enabled: bool, hint: &str) {
    ui.label(label).on_hover_text(hint);
    ui.add_enabled(enabled, egui::Slider::new(value, 0..=100))
        .accessible_name(label)
        .on_hover_text(hint)
        .on_disabled_hover_text(hint);
    ui.end_row();
}

impl MapView {
    pub(super) fn show_random_map(&mut self) {
        if self.pending.is_none() {
            self.workbench.new_map = false;
            self.workbench.random_map = true;
        }
    }

    pub(super) fn random_map_window(&mut self, ctx: &egui::Context, directory: &mut String) {
        if !self.workbench.random_map {
            return;
        }
        let mut create = false;
        let mut cancel = false;
        let modal = theme::modal(ctx, "random-map", |ui| {
            ui.set_width(490.0_f32.min((ctx.content_rect().width() - 64.).max(240.)));
            // Area remembers its previous size; bound it again after a native resize.
            ui.set_max_height((ctx.content_rect().height() - 64.).max(120.));
            ui.heading(tr!("新建随机地图"));
            ui.label(tr!("生成包含自然地貌、玩家起点和资源的遭遇战地图。"));
            ui.separator();
            let mut valid = false;
            egui::ScrollArea::vertical().id_salt("random-map-form")
                .max_height((ctx.content_rect().height() - 185.).clamp(120., 640.))
                .show(ui, |ui| {
                    ui.label(tr!("游戏资源目录"));
                    if theme::path_entry(ui, directory, tr!("包含 ra2.mix 与 ra2md.mix 的目录"), tr!("浏览…"))
                        && let Some(path) = rfd::FileDialog::new().pick_folder() {
                        *directory = path.display().to_string();
                    }
                    let options = &mut self.workbench.random_options;
                    ui.label(tr!("地图名称"));
                    ui.add(egui::TextEdit::singleline(&mut options.title).desired_width(f32::INFINITY));
                    egui::Grid::new("random-map-fields").num_columns(2).spacing([20., 8.]).show(ui, |ui| {
                        let t = &mut options.terrain;
                        choice(ui, "地图环境", &mut t.theater, &ENVIRONMENTS);
                        choice(ui, "地貌类型", &mut t.map_type, &LANDSCAPES);
                        ui.label("");
                        if ui.button(tr!("应用推荐参数"))
                            .on_hover_text(tr!("按当前地貌重设水域规模和地形崎岖度，保留其他参数和种子。"))
                            .clicked() {
                            (t.water_amount, t.ruggedness) = match t.map_type {
                                0 => (65, 10), 1 => (40, 20), 2 => (60, 15),
                                3 => (20, 20), _ => (10, 70),
                            };
                        }
                        ui.end_row();
                        choice(ui, "宽度档位", &mut t.width, &SIZES);
                        choice(ui, "高度档位", &mut t.height, &SIZES);
                        ui.label(tr!("玩家起点"));
                        crate::form_edit::number(ui, egui::DragValue::new(&mut t.num_players).range(2..=i32::from(crate::new_map::MAX_PLAYERS)).suffix(tr!(" 个")), tr!("玩家起点"));
                        ui.end_row();
                        percentage(ui, tr!("水域规模"), &mut t.water_amount, t.map_type != 0,
                            tr!("群岛的海陆布局由生成器决定，此选项不生效。其他地貌中数值越大，水域通常越多；不代表实际占地百分比。"));
                        percentage(ui, tr!("地形崎岖度"), &mut t.ruggedness, true,
                            tr!("控制后期地表起伏。0～10 不增加随机高度扰动，但不会消除已有高台、悬崖或岸线起伏。"));
                        percentage(ui, tr!("区域连通程度"), &mut t.accessibility, matches!(t.map_type, 3 | 4),
                            tr!("仅内陆湖泊和山地生效。数值越大，区域间尝试的坡道连接越多；0 也会尝试连接，不保证所有区域连通。"));
                        percentage(ui, tr!("树林数量"), &mut t.vegetation, true,
                            tr!("控制普通树林的生成数量，温带中还影响部分粗糙地表。0 不会移除矿树或所有地表斑块。"));
                        percentage(ui, tr!("矿区分散程度"), &mut t.tiberium_layout, options.ore,
                            tr!("数值越大，通常会生成更多、更小的矿区；不直接调整玩家起点间距。"));
                        percentage(ui, tr!("区域大小"), &mut t.region_size, matches!(t.map_type, 3 | 4),
                            tr!("仅内陆湖泊和山地生效。数值越小，大块陆地区域越容易被切分。"));
                        ui.label(tr!("矿区"));
                        ui.checkbox(&mut options.ore, tr!("生成矿石资源"));
                        ui.end_row();
                        ui.label(tr!("资源丰度"));
                        ui.add_enabled(options.ore, egui::Slider::new(&mut t.tiberium, 1..=100)).accessible_name(tr!("资源丰度"));
                        ui.end_row();
                        ui.label(tr!("宝石"));
                        let mut gems = t.resources == 3;
                        if ui.add_enabled(options.ore, egui::Checkbox::new(&mut gems, tr!("矿区包含宝石"))).changed() {
                            t.resources = if gems { 3 } else { 1 };
                        }
                        ui.end_row();
                        ui.label(tr!("中立科技建筑"));
                        ui.checkbox(&mut options.tech_buildings, tr!("按游戏规则随机放置（可能不生成）"))
                            .on_hover_text(tr!("群岛仅尝试在没有玩家起点的岛屿上放置；各地貌均可能因随机结果或缺少合适位置而不生成。"));
                        ui.end_row();
                        ui.label(tr!("城镇"));
                        ui.checkbox(&mut t.urban_enabled, tr!("生成中立城镇"));
                        ui.end_row();
                        percentage(ui, tr!("城镇规模"), &mut t.urban_presence, t.urban_enabled,
                            tr!("控制中立城镇的数量和规模；仅放置当前环境有素材的民用建筑，不改变地形与玩家起点。"));
                        ui.label(tr!("随机种子"));
                        ui.horizontal(|ui| {
                            crate::form_edit::number(ui, egui::DragValue::new(&mut t.seed).range(0..=65535), tr!("随机种子"));
                            if ui.button(tr!("换一个")).clicked() { t.seed = fresh_seed(); }
                        });
                        ui.end_row();
                    });
                    let (width, height) = options.dimensions();
                    theme::field(ui, tr!("实际地图尺寸"), tr!("{width} × {height} 格（含边界）", width = width, height = height));
                    ui.small(tr!("尺寸由档位和玩家数共同决定。水量等数值表示生成强度，并非实际占地百分比。"));
                    ui.small(tr!("相同版本、资源、参数和种子可复现地图。重新打开保留参数和种子，点击“换一个”可更换种子。生成后可继续编辑。"));
                    let validation = options.validate();
                    valid = validation.is_ok() && !directory.trim().is_empty();
                    if let Err(error) = validation { ui.colored_label(theme::ACCENT, error.to_string()); }
                    if directory.trim().is_empty() { ui.colored_label(theme::ACCENT, tr!("请先选择游戏资源目录。")); }
                });
            ui.separator();
            theme::right_to_left(ui, |ui| {
                create = ui
                    .add_enabled(valid, theme::primary(tr!("生成地图")))
                    .clicked();
                cancel = ui.button(tr!("取消")).clicked();
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
