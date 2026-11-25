//! Terrain authoring palette. Only cached resource data is accessed on the UI thread.
use super::*;
use crate::i18n::tr;
use crate::ui_theme as theme;
use egui::{RichText, TextureHandle};
use ra2_map_assets::{catalog::TerrainBrush, terrain_library::TerrainCategory};
use vera20k_core::map::theater::TileKey;

fn sync_palette(state: &mut editing_ui::EditorUi, scene: &MapScene) {
    let source = Arc::as_ptr(&scene.palette) as usize;
    if state.terrain_palette != source {
        state.textures.clear();
        state.template_textures.clear();
        state.terrain_palette = source;
    }
}

pub(super) fn subtile_texture(
    state: &mut editing_ui::EditorUi,
    ctx: &egui::Context,
    scene: &MapScene,
    key: TileKey,
) -> Option<TextureHandle> {
    sync_palette(state, scene);
    let cache_key = (key.tile_id, key.sub_tile, key.variant);
    if !state.textures.contains_key(&cache_key) {
        let image = scene.palette.images.get(&key)?;
        if state.textures.len() >= 192 {
            state.textures.clear();
        }
        state.textures.insert(
            cache_key,
            ctx.load_texture(
                format!("tile-{cache_key:?}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.info.width as usize, image.info.height as usize],
                    &image.rgba,
                ),
                egui::TextureOptions::NEAREST,
            ),
        );
    }
    state.textures.get(&cache_key).cloned()
}

pub(super) fn template_texture(
    state: &mut editing_ui::EditorUi,
    ctx: &egui::Context,
    scene: &MapScene,
    key: TileKey,
) -> Option<TextureHandle> {
    sync_palette(state, scene);
    let id = (key.tile_id, key.variant);
    if !state.template_textures.contains_key(&id) {
        let image = if state.framework {
            &scene.palette.framework_previews
        } else {
            &scene.palette.previews
        }
        .get(&id)?;
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

fn matches_brush(
    scene: &MapScene,
    brush: &TerrainBrush,
    category: Option<TerrainCategory>,
    group: Option<u16>,
    filter: &str,
) -> bool {
    let metadata = scene.palette.groups.get(&brush.key.tile_id);
    category.is_none_or(|c| metadata.is_some_and(|g| g.category == c))
        && group.is_none_or(|id| metadata.is_some_and(|g| g.id == id))
        && (filter.is_empty()
            || brush.label.to_ascii_lowercase().contains(filter)
            || crate::labels::terrain_brush(&brush.label).contains(filter)
            || metadata.is_some_and(|g| g.category.name().contains(filter)))
}

/// Complete cliff templates use the lowest available subtile as their default anchor.
pub(super) fn choices(
    scene: &MapScene,
    category: Option<TerrainCategory>,
    group: Option<u16>,
    subtiles: bool,
    variants: bool,
    filter: &str,
) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut cliff_anchors = HashMap::new();
    if !subtiles {
        for (i, brush) in scene.brushes.iter().enumerate() {
            let key = brush.key;
            if !scene
                .palette
                .groups
                .get(&key.tile_id)
                .is_some_and(|g| g.category == TerrainCategory::Cliff)
            {
                continue;
            }
            let Some(cell) = scene
                .palette
                .templates
                .get(&(key.tile_id, key.variant))
                .and_then(|t| t.cells.get(usize::from(key.sub_tile)))
                .and_then(|s| *s)
            else {
                continue;
            };
            let candidate = (cell.height, key.sub_tile, i);
            let best = cliff_anchors
                .entry((key.tile_id, key.variant))
                .or_insert(candidate);
            *best = (*best).min(candidate);
        }
    }
    scene
        .brushes
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            (variants || b.key.variant == 0)
                && matches_brush(scene, b, category, group, filter)
                && (subtiles || seen.insert((b.key.tile_id, b.key.variant)))
        })
        .map(|(i, b)| {
            cliff_anchors
                .get(&(b.key.tile_id, b.key.variant))
                .map_or(i, |&(_, _, anchor)| anchor)
        })
        .collect()
}

fn fill_choices(scene: &MapScene) -> Vec<usize> {
    scene
        .brushes
        .iter()
        .enumerate()
        .filter_map(|(index, brush)| {
            ra2_map_scene::fill_tools::material(scene, brush)
                .is_ok()
                .then_some(index)
        })
        .collect()
}

impl Editor {
    pub(super) fn terrain_placement_shortcuts(&mut self, ctx: &egui::Context, scene: &MapScene) {
        if self.stroking || self.tool != Tool::Terrain || self.doc.is_none() {
            return;
        }
        let delta = ctx.input_mut(|i| {
            if i.modifiers.any() {
                return 0;
            }
            i8::from(i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp))
                - i8::from(i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown))
        });
        if delta == 0 {
            return;
        }
        let manual = matches!(
            self.terrain_mode,
            TerrainMode::Subtile | TerrainMode::Template
        );
        let automatic = matches!(
            self.terrain_mode,
            TerrainMode::Plateau
                | TerrainMode::CliffOutline
                | TerrainMode::CliffPolyline
                | TerrainMode::CliffFront
                | TerrainMode::CliffBack
                | TerrainMode::RampEntrance
        );
        if !manual && !automatic {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let mut max = if self.terrain_mode == TerrainMode::CliffPolyline {
            127u8
        } else {
            123u8
        };
        let inferred = if manual {
            let Some(brush) = scene.brushes.get(self.brush) else {
                return;
            };
            let Some(t) = scene.palette.templates.get(&(brush.key.tile_id, 0)) else {
                return;
            };
            max = 127
                - t.cells
                    .iter()
                    .flatten()
                    .map(|s| s.height)
                    .max()
                    .unwrap_or(0)
                    .min(127);
            self.terrain_hover
                .map(|at| {
                    let old = terrain_tools::value(doc, scene, at);
                    if self.terrain_mode == TerrainMode::Template {
                        let relative = t
                            .cells
                            .get(brush.key.sub_tile as usize)
                            .and_then(|s| *s)
                            .map_or(0, |s| s.height);
                        terrain_tools::template_placement(doc, scene, brush, at, None)
                            .ok()
                            .and_then(|cells| cells.into_iter().find(|(cell, _)| *cell == at))
                            .map_or(old.level.saturating_sub(relative), |(_, v)| {
                                v.level.saturating_sub(relative)
                            })
                    } else {
                        scene
                            .palette
                            .raw_ids
                            .get(&old.tile)
                            .filter(|tile| {
                                scene
                                    .palette
                                    .groups
                                    .get(tile)
                                    .is_some_and(|g| g.category == TerrainCategory::Cliff)
                            })
                            .and_then(|tile| scene.palette.templates.get(&(*tile, 0)))
                            .and_then(|t| t.cells.get(old.sub as usize))
                            .and_then(|s| *s)
                            .map_or(old.level, |s| old.level.saturating_sub(s.height))
                    }
                })
                .unwrap_or(0)
        } else {
            self.terrain_hover.map_or(0, |at| {
                if self.terrain_mode == TerrainMode::CliffPolyline {
                    self.cliff_polyline_reference_level(scene, at)
                } else {
                    ra2_map_scene::cliff_authoring::infer_base(doc, scene, at)
                }
            })
        };
        let level = (i16::from(self.template_level.unwrap_or(inferred)) + i16::from(delta))
            .clamp(0, i16::from(max)) as u8;
        self.template_level = Some(level);
        self.status = tr!("放置基准高度 {arg0}；PageUp / PageDown 调整", arg0 = level);
    }

    pub(crate) fn set_framework(&mut self, enabled: bool) {
        if self.stroking {
            return;
        }
        self.framework = enabled;
        if self.ui_state.framework != enabled {
            self.ui_state.framework = enabled;
            self.ui_state.textures.clear();
            self.ui_state.template_textures.clear();
        }
    }
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
            .context(tr!("缺少多层悬崖预览"))?;
        self.tool = Tool::Terrain;
        self.terrain_mode = TerrainMode::Template;
        self.template_level = Some(2);
        Ok(())
    }

    pub(super) fn terrain_library(&mut self, ui: &mut egui::Ui, scene: &MapScene, filter: &str) {
        sync_palette(&mut self.ui_state, scene);
        self.set_framework(self.framework);
        let filling = self.terrain_mode == TerrainMode::Fill && self.tool == Tool::Terrain;
        let fill_items = filling.then(|| fill_choices(scene));
        let fill_groups: BTreeSet<_> = fill_items
            .iter()
            .flatten()
            .filter_map(|&i| {
                scene
                    .palette
                    .groups
                    .get(&scene.brushes[i].key.tile_id)
                    .map(|g| g.id)
            })
            .collect();
        egui::CollapsingHeader::new(tr!("分类与模板选项")).show(ui, |ui| {
            egui::ComboBox::from_id_salt("terrain-category")
                .width(ui.available_width())
                .selected_text(
                    self.ui_state
                        .terrain_category
                        .map_or(tr!("全部地形"), TerrainCategory::name),
                )
                .show_ui(ui, |ui| {
                    let old = self.ui_state.terrain_category;
                    ui.selectable_value(&mut self.ui_state.terrain_category, None, tr!("全部地形"));
                    for category in TerrainCategory::ALL {
                        if scene.palette.groups.values().any(|g| {
                            g.category == category && (!filling || fill_groups.contains(&g.id))
                        }) {
                            ui.selectable_value(
                                &mut self.ui_state.terrain_category,
                                Some(category),
                                crate::i18n::text(category.name()),
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
                .filter(|g| !filling || fill_groups.contains(&g.id))
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
                        .map(crate::labels::terrain)
                        .unwrap_or_else(|| tr!("全部地形组").into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ui_state.terrain_group, None, tr!("全部地形组"));
                    for (id, name) in &groups {
                        ui.selectable_value(
                            &mut self.ui_state.terrain_group,
                            Some(*id),
                            format!("{} · #{id}", crate::labels::terrain(name)),
                        )
                        .on_hover_text(*name);
                    }
                });
            ui.add_enabled_ui(!filling, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.ui_state.terrain_subtiles,
                        false,
                        tr!("完整地形"),
                    );
                    ui.selectable_value(&mut self.ui_state.terrain_subtiles, true, tr!("子块"));
                    ui.checkbox(&mut self.ui_state.terrain_variants, tr!("变体"));
                });
            });
        });
        let subtiles = self.ui_state.terrain_subtiles && !filling;
        let category = self.ui_state.terrain_category;
        let group = self.ui_state.terrain_group;
        let matching = if let Some(mut items) = fill_items {
            items.retain(|&index| {
                matches_brush(scene, &scene.brushes[index], category, group, filter)
            });
            items
        } else {
            choices(
                scene,
                category,
                group,
                subtiles,
                self.ui_state.terrain_variants,
                filter,
            )
        };
        ui.small(tr!(
            "{arg0} 个{arg1}",
            arg0 = matching.len(),
            arg1 = if filling {
                tr!("可平铺地表")
            } else if subtiles {
                tr!("子块")
            } else {
                tr!("完整模板")
            }
        ));
        if matching.is_empty() {
            ui.label(tr!("没有匹配的地形；可清空搜索或切换地形组。"));
        }
        let columns = (ui.available_width() / 98.).floor().max(1.) as usize;
        let width = (ui.available_width() - 8. * (columns - 1) as f32) / columns as f32;
        egui::ScrollArea::vertical()
            .id_salt("terrain-palette")
            .max_height((ui.clip_rect().bottom() - ui.cursor().top() - 46.).max(80.))
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
                                    && let Some(image) = if self.framework {
                                        &scene.palette.framework_images
                                    } else {
                                        &scene.palette.images
                                    }
                                    .get(&key)
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
                            if !filling && self.ui_state.terrain_variants {
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
                                crate::labels::terrain_brush(&brush.label),
                                t.map(|t| tr!(
                                    "{arg0}×{arg1} · {arg2} 个有效子块",
                                    arg0 = t.width,
                                    arg1 = t.height,
                                    arg2 = t.cells.iter().flatten().count()
                                ))
                                .unwrap_or_default(),
                                if filling {
                                    tr!("点击选择填充素材，保持填充工具")
                                } else if subtiles {
                                    tr!("点击使用子块画笔")
                                } else {
                                    tr!("点击放置完整地形；上方可调整同组模板、锚点与基准高度")
                                }
                            );
                            if response.on_hover_text(hint).clicked() {
                                self.select_terrain_material(index, subtiles);
                            }
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            if filling {
                ui.set_min_height(ui.spacing().interact_size.y);
            }
            if let Some(brush) = scene.brushes.get(self.brush) {
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::labels::terrain_brush(&brush.label))
                            .size(10.)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                )
                .on_hover_text(&brush.label);
            }
        });
    }

    pub(super) fn terrain_template_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        self.set_framework(self.framework);
        let Some(brush) = scene.brushes.get(self.brush) else {
            return;
        };
        let group = scene.palette.groups.get(&brush.key.tile_id);
        if let Some(group) = group {
            ui.label(format!(
                "{} · {}",
                crate::i18n::text(group.category.name()),
                crate::labels::terrain(&group.name)
            ))
            .on_hover_text(&group.name);
            let siblings = choices(scene, None, Some(group.id), false, false, "");
            if let Some(pos) = siblings
                .iter()
                .position(|&i| scene.brushes[i].key.tile_id == brush.key.tile_id)
            {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(pos > 0, egui::Button::new(tr!("◀ 上一块")))
                        .clicked()
                    {
                        self.brush = siblings[pos - 1];
                    }
                    ui.small(format!("{}/{}", pos + 1, siblings.len()));
                    if ui
                        .add_enabled(pos + 1 < siblings.len(), egui::Button::new(tr!("下一块 ▶")))
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
            tr!("模板尺寸"),
            tr!(
                "{arg0}×{arg1} · {arg2} 子块",
                arg0 = t.width,
                arg1 = t.height,
                arg2 = t.cells.iter().flatten().count()
            ),
        );
        let heights: Vec<_> = t.cells.iter().flatten().map(|s| s.height).collect();
        theme::field(
            ui,
            tr!("相对高度"),
            tr!(
                "{min}～{max}",
                min = heights.iter().min().unwrap_or(&0),
                max = heights.iter().max().unwrap_or(&0)
            ),
        );
        let mut fixed = self.template_level.is_some();
        ui.small(tr!(
            "PageUp / PageDown 调整放置基准高度；取消指定高度可恢复跟随地面。"
        ));
        if ui.checkbox(&mut fixed, tr!("指定模板基准高度")).changed() {
            self.template_level = fixed.then_some(0);
        }
        if let Some(level) = &mut self.template_level {
            ui.add(egui::Slider::new(level, 0..=127).text(tr!("基准高度")));
            ui.small(tr!("各子块高度 = 基准高度 + 模板相对高度。"));
        } else {
            ui.small(tr!("高度跟随鼠标下的格子；所选锚点与该格等高。"));
        }
        if let Some(texture) = template_texture(&mut self.ui_state, ui.ctx(), scene, key) {
            let size = texture.size_vec2();
            let scale = ((ui.available_width() - 8.) / size.x)
                .min(90. / size.y)
                .min(2.);
            ui.add(egui::Image::new(&texture).fit_to_exact_size(size * scale));
        }
        ui.small(tr!("点击下方菱形选择对齐鼠标的子块："));
        if group.is_some_and(|g| g.category == TerrainCategory::Cliff) {
            ui.small(tr!(
                "悬崖模板默认以最低子块对齐；也可点击高侧子块接续已有高台。"
            ));
        }
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
        ui.small(tr!(
            "数字为相对高度，亮色为锚点。绿色轮廓可放置，红色提示原因。"
        ));
        if key.variant != 0 {
            ui.small(tr!(
                "变体仅供预览；地图保存基础地形编号，实际外观由游戏选择。"
            ));
        }
        if group.is_some_and(|g| g.category == TerrainCategory::Bridge) {
            ui.small(tr!(
                "此处仅放置地形模板；请用“桥梁工具”联动生成桥头和桥面覆盖物。"
            ));
        }
    }
}
