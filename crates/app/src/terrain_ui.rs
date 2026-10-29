//! Terrain authoring palette. Only cached resource data is accessed on the UI thread.
use super::*;
use crate::ui_theme as theme;
use egui::{RichText, TextureHandle};
use ra2_map_assets::terrain_library::TerrainCategory;
use vera20k_core::map::theater::TileKey;

pub(super) fn template_texture(
    state: &mut editing_ui::EditorUi,
    ctx: &egui::Context,
    scene: &MapScene,
    key: TileKey,
) -> Option<TextureHandle> {
    let id = (key.tile_id, key.variant);
    if !state.template_textures.contains_key(&id) {
        let image = scene.palette.previews.get(&id)?;
        if state.template_textures.len() >= 128 {
            state.template_textures.clear();
        }
        state.template_textures.insert(
            id,
            ctx.load_texture(
                format!("terrain-template-{}-{}", id.0, id.1),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.info.width as usize, image.info.height as usize],
                    &image.rgba,
                ),
                egui::TextureOptions::NEAREST,
            ),
        );
    }
    state.template_textures.get(&id).cloned()
}

/// One entry per complete template; holes never become selectable anchors.
pub(super) fn choices(
    scene: &MapScene,
    category: Option<TerrainCategory>,
    group: Option<u16>,
    subtiles: bool,
    variants: bool,
    filter: &str,
) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    scene
        .brushes
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            let metadata = scene.palette.groups.get(&b.key.tile_id);
            (variants || b.key.variant == 0)
                && category.is_none_or(|c| metadata.is_some_and(|g| g.category == c))
                && group.is_none_or(|id| metadata.is_some_and(|g| g.id == id))
                && (filter.is_empty()
                    || b.label.to_ascii_lowercase().contains(filter)
                    || metadata.is_some_and(|g| g.category.name().contains(filter)))
                && (subtiles || seen.insert((b.key.tile_id, b.key.variant)))
        })
        .map(|(i, _)| i)
        .collect()
}

impl Editor {
    pub(crate) fn prepare_cliff_smoke_preview(&mut self, scene: &MapScene) -> Result<()> {
        self.show_library(0, "", "");
        self.ui_state.terrain_category = Some(TerrainCategory::Cliff);
        self.ui_state.terrain_subtiles = false;
        self.brush = choices(scene, Some(TerrainCategory::Cliff), None, false, false, "")
            .into_iter()
            .find(|&i| {
                scene.palette.templates[&(scene.brushes[i].key.tile_id, 0)]
                    .cells
                    .iter()
                    .flatten()
                    .any(|s| s.height > 0)
            })
            .context("缺少多层悬崖预览")?;
        self.tool = Tool::Terrain;
        self.terrain_mode = TerrainMode::Template;
        self.template_level = Some(2);
        Ok(())
    }

    pub(super) fn terrain_library(&mut self, ui: &mut egui::Ui, scene: &MapScene, filter: &str) {
        egui::ComboBox::from_id_salt("terrain-quick-tool")
            .width(ui.available_width())
            .selected_text(if self.tool == Tool::Terrain {
                self.terrain_mode.name()
            } else {
                "高台、悬崖与桥梁…"
            })
            .show_ui(ui, |ui| {
                for mode in TerrainMode::ALL {
                    if ui
                        .selectable_label(
                            self.tool == Tool::Terrain && self.terrain_mode == mode,
                            mode.name(),
                        )
                        .clicked()
                    {
                        self.activate_tool(Tool::Terrain);
                        self.terrain_mode = mode;
                    }
                }
            });
        egui::ComboBox::from_id_salt("terrain-category")
            .width(ui.available_width())
            .selected_text(
                self.ui_state
                    .terrain_category
                    .map_or("全部地形", TerrainCategory::name),
            )
            .show_ui(ui, |ui| {
                let old = self.ui_state.terrain_category;
                ui.selectable_value(&mut self.ui_state.terrain_category, None, "全部地形");
                for category in TerrainCategory::ALL {
                    if scene
                        .palette
                        .groups
                        .values()
                        .any(|g| g.category == category)
                    {
                        ui.selectable_value(
                            &mut self.ui_state.terrain_category,
                            Some(category),
                            category.name(),
                        );
                    }
                }
                if old != self.ui_state.terrain_category {
                    self.ui_state.terrain_group = None;
                }
            });
        let groups: BTreeMap<_, _> = scene
            .palette
            .groups
            .values()
            .filter(|g| {
                self.ui_state
                    .terrain_category
                    .is_none_or(|c| c == g.category)
            })
            .map(|g| (g.id, g.name.as_str()))
            .collect();
        egui::ComboBox::from_id_salt("terrain-group")
            .width(ui.available_width())
            .selected_text(
                self.ui_state
                    .terrain_group
                    .and_then(|id| groups.get(&id))
                    .copied()
                    .unwrap_or("全部地形组"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.ui_state.terrain_group, None, "全部地形组");
                for (id, name) in &groups {
                    ui.selectable_value(
                        &mut self.ui_state.terrain_group,
                        Some(*id),
                        format!("{name} · #{id}"),
                    );
                }
            });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.ui_state.terrain_subtiles, false, "完整地形");
            ui.selectable_value(&mut self.ui_state.terrain_subtiles, true, "子块");
            ui.checkbox(&mut self.ui_state.terrain_variants, "变体");
        });
        let subtiles = self.ui_state.terrain_subtiles;
        let matching = choices(
            scene,
            self.ui_state.terrain_category,
            self.ui_state.terrain_group,
            subtiles,
            self.ui_state.terrain_variants,
            filter,
        );
        ui.small(format!(
            "{} 个{}",
            matching.len(),
            if subtiles { "子块" } else { "完整模板" }
        ));
        if matching.is_empty() {
            ui.label("没有匹配的地形；可清空搜索或切换地形组。");
        }
        let columns = (ui.available_width() / 98.).floor().max(1.) as usize;
        let width = (ui.available_width() - 8. * (columns - 1) as f32) / columns as f32;
        egui::ScrollArea::vertical()
            .id_salt("terrain-palette")
            .max_height((ui.available_height() - 46.).max(80.))
            .show_rows(ui, 94., matching.len().div_ceil(columns), |ui, rows| {
                for row in rows {
                    ui.horizontal(|ui| {
                        for &index in matching.iter().skip(row * columns).take(columns) {
                            let brush = &scene.brushes[index];
                            let key = brush.key;
                            let selected = self.tool == Tool::Terrain
                                && scene.brushes.get(self.brush).is_some_and(|b| {
                                    if subtiles {
                                        b.key == key
                                    } else {
                                        b.key.tile_id == key.tile_id && b.key.variant == key.variant
                                    }
                                });
                            let texture = if subtiles {
                                let cache_key = (key.tile_id, key.sub_tile, key.variant);
                                if !self.ui_state.textures.contains_key(&cache_key)
                                    && let Some(image) = scene.palette.images.get(&key)
                                {
                                    if self.ui_state.textures.len() >= 192 {
                                        self.ui_state.textures.clear();
                                    }
                                    self.ui_state.textures.insert(
                                        cache_key,
                                        ui.ctx().load_texture(
                                            format!("tile-{cache_key:?}"),
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [
                                                    image.info.width as usize,
                                                    image.info.height as usize,
                                                ],
                                                &image.rgba,
                                            ),
                                            egui::TextureOptions::NEAREST,
                                        ),
                                    );
                                }
                                self.ui_state.textures.get(&cache_key).cloned()
                            } else {
                                template_texture(&mut self.ui_state, ui.ctx(), scene, key)
                            };
                            let response = ui
                                .add_sized([width, 94.], egui::Button::new("").selected(selected));
                            let p = ui.painter().with_clip_rect(response.rect);
                            let image_rect = egui::Rect::from_min_max(
                                response.rect.min + egui::vec2(4., 4.),
                                response.rect.max - egui::vec2(4., 22.),
                            );
                            p.rect_filled(image_rect, 3, theme::BACKGROUND);
                            if let Some(texture) = texture {
                                let size = texture.size_vec2();
                                let factor = (image_rect.width() / size.x)
                                    .min(image_rect.height() / size.y)
                                    .min(2.);
                                p.image(
                                    texture.id(),
                                    egui::Rect::from_center_size(
                                        image_rect.center(),
                                        size * factor,
                                    ),
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                    Color32::WHITE,
                                );
                            }
                            let mut label = if subtiles {
                                format!("{}:{}", key.tile_id, key.sub_tile)
                            } else {
                                format!("#{}", key.tile_id)
                            };
                            if self.ui_state.terrain_variants {
                                label.push_str(&format!(" v{}", key.variant));
                            }
                            p.text(
                                response.rect.center_bottom() - egui::vec2(0., 10.),
                                egui::Align2::CENTER_CENTER,
                                label,
                                egui::FontId::monospace(11.),
                                if selected {
                                    theme::ACCENT
                                } else {
                                    theme::MUTED
                                },
                            );
                            let t = scene.palette.templates.get(&(key.tile_id, key.variant));
                            let hint = format!(
                                "{}\n{}\n{}",
                                brush.label,
                                t.map(|t| format!(
                                    "{}×{} · {} 个有效子块",
                                    t.width,
                                    t.height,
                                    t.cells.iter().flatten().count()
                                ))
                                .unwrap_or_default(),
                                if subtiles {
                                    "点击使用子块画笔"
                                } else {
                                    "点击放置完整地形；右侧可切换同组模板、锚点与基准高度"
                                }
                            );
                            if response.on_hover_text(hint).clicked() {
                                self.brush = index;
                                self.tool = Tool::Terrain;
                                self.terrain_mode = if subtiles {
                                    TerrainMode::Subtile
                                } else {
                                    TerrainMode::Template
                                };
                            }
                        }
                    });
                }
            });
        if let Some(brush) = scene.brushes.get(self.brush) {
            ui.add(
                egui::Label::new(RichText::new(&brush.label).size(10.).color(theme::MUTED))
                    .truncate(),
            );
        }
    }

    pub(super) fn terrain_template_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        let Some(brush) = scene.brushes.get(self.brush) else {
            return;
        };
        let group = scene.palette.groups.get(&brush.key.tile_id);
        if let Some(group) = group {
            ui.label(format!("{} · {}", group.category.name(), group.name));
            let siblings = choices(scene, None, Some(group.id), false, false, "");
            if let Some(pos) = siblings
                .iter()
                .position(|&i| scene.brushes[i].key.tile_id == brush.key.tile_id)
            {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(pos > 0, egui::Button::new("◀ 上一块"))
                        .clicked()
                    {
                        self.brush = siblings[pos - 1];
                    }
                    ui.small(format!("{}/{}", pos + 1, siblings.len()));
                    if ui
                        .add_enabled(pos + 1 < siblings.len(), egui::Button::new("下一块 ▶"))
                        .clicked()
                    {
                        self.brush = siblings[pos + 1];
                    }
                });
            }
        }
        let key = scene.brushes[self.brush].key;
        let Some(t) = scene.palette.templates.get(&(key.tile_id, key.variant)) else {
            return;
        };
        theme::field(
            ui,
            "模板尺寸",
            format!(
                "{}×{} · {} 子块",
                t.width,
                t.height,
                t.cells.iter().flatten().count()
            ),
        );
        let heights: Vec<_> = t.cells.iter().flatten().map(|s| s.height).collect();
        theme::field(
            ui,
            "相对高度",
            format!(
                "{}～{}",
                heights.iter().min().unwrap_or(&0),
                heights.iter().max().unwrap_or(&0)
            ),
        );
        let mut fixed = self.template_level.is_some();
        if ui.checkbox(&mut fixed, "指定模板基准高度").changed() {
            self.template_level = fixed.then_some(0);
        }
        if let Some(level) = &mut self.template_level {
            ui.add(egui::Slider::new(level, 0..=14).text("基准高度"));
            ui.small("各子块高度 = 基准高度 + 模板相对高度。");
        } else {
            ui.small("高度跟随鼠标下的格子；所选锚点与该格等高。");
        }
        if let Some(texture) = template_texture(&mut self.ui_state, ui.ctx(), scene, key) {
            let size = texture.size_vec2();
            let scale = ((ui.available_width() - 8.) / size.x)
                .min(90. / size.y)
                .min(2.);
            ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
        }
        ui.small("点击下方菱形选择对齐鼠标的子块：");
        let scale = ((ui.available_width() - 8.) / (f32::from(t.width + t.height) * 30.)).min(0.75);
        let size = egui::vec2(
            f32::from(t.width + t.height) * 30.,
            f32::from(t.width + t.height) * 15.,
        ) * scale;
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let origin = rect.left_top() + egui::vec2(f32::from(t.height) * 30., 15.) * scale;
        for (sub, cell) in t.cells.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let col = sub as u16 % t.width;
            let row = sub as u16 / t.width;
            let center = origin
                + egui::vec2(
                    (f32::from(col) - f32::from(row)) * 30.,
                    f32::from(col + row) * 15.,
                ) * scale;
            let selected = sub == usize::from(key.sub_tile);
            let points = [[0., -14.], [28., 0.], [0., 14.], [-28., 0.]]
                .map(|[x, y]| center + egui::vec2(x, y) * scale);
            ui.painter().add(egui::Shape::convex_polygon(
                points.to_vec(),
                if selected {
                    theme::ACCENT
                } else {
                    theme::BACKGROUND
                },
                egui::Stroke::new(1., theme::MUTED),
            ));
            if scale >= 0.3 {
                ui.painter().text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    cell.height,
                    egui::FontId::monospace(10.),
                    if selected {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    },
                );
            }
            if response.clicked()
                && response.interact_pointer_pos().is_some_and(|p| {
                    let d = (p - center) / scale;
                    d.x.abs() / 30. + d.y.abs() / 15. <= 1.
                })
                && let Some(index) = scene.brushes.iter().position(|b| {
                    b.key
                        == TileKey {
                            sub_tile: sub as u8,
                            ..key
                        }
                })
            {
                self.brush = index;
            }
        }
        ui.small("数字为相对高度，亮色为锚点。绿色轮廓可放置，红色提示原因。");
        if key.variant != 0 {
            ui.small("变体仅供预览；地图保存基础地形编号，实际外观由游戏选择。");
        }
        if group.is_some_and(|g| g.category == TerrainCategory::Bridge) {
            ui.small("此处仅放置地形模板；请用“桥梁工具”联动生成桥头和桥面覆盖物。");
        }
    }
}
