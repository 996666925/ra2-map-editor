use super::*;
use crate::ui_theme::{self as theme, Icon};
use egui::{RichText, TextureHandle};

#[derive(Default)]
pub(super) struct EditorUi {
    pub(super) tab: u8,
    filters: [String; 5],
    pub sprite_source: Option<Arc<ra2_map_assets::sprites::SpritePalette>>,
    pub sprites: HashMap<(String, String), TextureHandle>,
    pub map_sprites: HashMap<usize, TextureHandle>,
    pub textures: HashMap<(u16, u8, u8), TextureHandle>,
    pub template_textures: HashMap<(u16, u8), TextureHandle>,
    pub terrain_category: Option<ra2_map_assets::terrain_library::TerrainCategory>,
    pub terrain_group: Option<u16>,
    pub terrain_subtiles: bool,
    pub terrain_variants: bool,
}

pub(super) fn map_sprite_texture(
    cache: &mut HashMap<usize, TextureHandle>,
    ctx: &egui::Context,
    image: &Arc<ra2_map_assets::assets::DecodedImage>,
) -> egui::TextureId {
    let key = Arc::as_ptr(image) as usize;
    cache
        .entry(key)
        .or_insert_with(|| {
            ctx.load_texture(
                format!("map-object-{key}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.info.width as usize, image.info.height as usize],
                    &image.rgba,
                ),
                egui::TextureOptions::NEAREST,
            )
        })
        .id()
}

fn tool_icon(tool: Tool) -> Icon {
    match tool {
        Tool::Select | Tool::Region => Icon::Select,
        Tool::Paste | Tool::RegionMove => Icon::Move,
        Tool::Clone => Icon::Building,
        Tool::Terrain => Icon::Terrain,
        Tool::Ore => Icon::Ore,
        Tool::Gems => Icon::Gem,
        Tool::Overlay => Icon::Ore,
        Tool::Erase => Icon::Erase,
        Tool::Place => Icon::Building,
        Tool::Move => Icon::Move,
        Tool::Waypoint => Icon::Flag,
        Tool::Pick => Icon::Pick,
    }
}
fn tool_label(tool: Tool) -> &'static str {
    match tool {
        Tool::Move => "移动",
        Tool::Place => "放置对象",
        Tool::Waypoint => "起点 / 航点",
        _ => tool.name(),
    }
}
impl Editor {
    pub(crate) fn show_library(&mut self, tab: u8, section: &str, filter: &str) {
        self.ui_state.tab = tab;
        self.ui_state.filters[tab as usize] = filter.into();
        if tab == 0 {
            self.ui_state.terrain_group = None;
            self.ui_state.terrain_category = None;
            self.ui_state.terrain_variants = filter.starts_with('v');
        }
        if !section.is_empty() {
            self.section = section.into();
        }
    }
    pub fn document_actions(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        ui.add_enabled_ui(self.doc.is_some() && !self.saving && !self.stroking, |ui| {
            if theme::button(ui, Icon::Save, "保存", 73., false)
                .on_hover_text("保存当前地图  Ctrl+S")
                .clicked()
            {
                self.save(false);
            }
        });
        let (undo, redo) = self
            .doc
            .as_ref()
            .map(|doc| {
                (
                    doc.undo_name().map(str::to_owned),
                    doc.redo_name().map(str::to_owned),
                )
            })
            .unwrap_or_default();
        ui.add_enabled_ui(
            undo.is_some() || self.stroking || self.object_properties_dirty(),
            |ui| {
                if theme::button(ui, Icon::Undo, "", 34., false)
                    .on_hover_text(if self.stroking {
                        "取消当前一笔  Ctrl+Z / Esc".into()
                    } else if self.object_properties_dirty() {
                        "放弃未应用的对象属性修改  Ctrl+Z".into()
                    } else {
                        format!("撤销  Ctrl+Z\n{}", undo.unwrap_or_default())
                    })
                    .clicked()
                {
                    self.history(false, scene);
                }
            },
        );
        ui.add_enabled_ui(redo.is_some(), |ui| {
            if theme::button(ui, Icon::Redo, "", 34., false)
                .on_hover_text(format!("重做  Ctrl+Y\n{}", redo.unwrap_or_default()))
                .clicked()
            {
                self.history(true, scene);
            }
        });
    }
    pub fn toolbox(&mut self, ui: &mut egui::Ui, scene: Option<&MapScene>) {
        theme::section(ui, "编辑工具", "TOOLS");
        let width = (ui.available_width() - 8.) / 2.;
        let compact = ui.available_height() < 680.;
        if compact {
            ui.add_enabled_ui(self.doc.is_some(), |ui| {
                egui::ComboBox::from_id_salt("compact-tool-selector")
                    .width(ui.available_width())
                    .selected_text(self.tool.name())
                    .show_ui(ui, |ui| {
                        for tool in [
                            Tool::Select,
                            Tool::Region,
                            Tool::Paste,
                            Tool::RegionMove,
                            Tool::Clone,
                            Tool::Move,
                            Tool::Terrain,
                            Tool::Pick,
                            Tool::Ore,
                            Tool::Gems,
                            Tool::Overlay,
                            Tool::Place,
                            Tool::Waypoint,
                            Tool::Erase,
                        ] {
                            if ui
                                .selectable_label(
                                    self.tool == tool,
                                    format!("{}  {}", tool.name(), tool.shortcut()),
                                )
                                .on_hover_text(tool.hint())
                                .clicked()
                            {
                                self.activate_tool(tool);
                            }
                        }
                    });
            });
        } else {
            ui.add_enabled_ui(self.doc.is_some(), |ui| {
                for pair in [
                    [Tool::Select, Tool::Move],
                    [Tool::Terrain, Tool::Pick],
                    [Tool::Ore, Tool::Gems],
                    [Tool::Place, Tool::Waypoint],
                ] {
                    ui.horizontal(|ui| {
                        for tool in pair {
                            if theme::button(
                                ui,
                                tool_icon(tool),
                                tool_label(tool),
                                width,
                                self.tool == tool,
                            )
                            .on_hover_text(format!(
                                "{}  {}\n{}",
                                tool.name(),
                                tool.shortcut(),
                                tool.hint()
                            ))
                            .clicked()
                            {
                                self.activate_tool(tool);
                            }
                        }
                    });
                }
                if theme::button(
                    ui,
                    Icon::Erase,
                    "擦除覆盖物",
                    ui.available_width(),
                    self.tool == Tool::Erase,
                )
                .clicked()
                {
                    self.activate_tool(Tool::Erase);
                }
            });
        }
        ui.add_enabled_ui(self.doc.is_some(), |ui| {
            if theme::button(
                ui,
                Icon::Select,
                "区域编辑",
                ui.available_width(),
                self.tool == Tool::Region,
            )
            .clicked()
            {
                self.activate_tool(Tool::Region);
            }
        });
        ui.add_space(4.);
        ui.separator();
        ui.add_space(4.);
        theme::section(ui, "素材库", scene.map_or("未加载", |_| "LIBRARY"));
        if compact {
            let categories = ["地形", "对象", "航点", "覆盖物", "动画"];
            egui::ComboBox::from_id_salt("compact-library-selector")
                .width(ui.available_width())
                .selected_text(categories[self.ui_state.tab as usize])
                .show_ui(ui, |ui| {
                    for (index, label) in categories.into_iter().enumerate() {
                        ui.selectable_value(&mut self.ui_state.tab, index as u8, label);
                    }
                });
        } else {
            ui.horizontal(|ui| {
                for (index, name) in [(0, "地形"), (1, "对象"), (2, "航点")] {
                    ui.selectable_value(&mut self.ui_state.tab, index, name);
                }
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.ui_state.tab, 3, "覆盖物");
                ui.selectable_value(&mut self.ui_state.tab, 4, "动画");
            });
        }
        let Some(scene) = scene else {
            ui.add_space(24.);
            ui.label(RichText::new("先打开一张地图").color(theme::MUTED));
            ui.small("地形素材与对象目录将在这里显示。");
            return;
        };
        if self.ui_state.tab != 2 {
            ui.add(
                egui::TextEdit::singleline(&mut self.ui_state.filters[self.ui_state.tab as usize])
                    .hint_text("筛选名称或编号…")
                    .desired_width(f32::INFINITY),
            );
        }
        let filter = self.ui_state.filters[self.ui_state.tab as usize]
            .trim()
            .to_ascii_lowercase();
        match self.ui_state.tab {
            0 => self.terrain_library(ui, scene, &filter),
            1 | 4 => {
                let animations = self.ui_state.tab == 4;
                if !animations {
                    egui::ComboBox::from_id_salt("palette-object-category")
                        .width(ui.available_width())
                        .selected_text(category(&self.section))
                        .show_ui(ui, |ui| {
                            for (id, label) in [
                                ("Structures", "建筑"),
                                ("Units", "车辆"),
                                ("Infantry", "步兵"),
                                ("Aircraft", "飞机"),
                                ("Terrain", "树木 / 地形对象"),
                                ("Smudge", "弹坑 / 污迹"),
                            ] {
                                ui.selectable_value(&mut self.section, id.into(), label);
                            }
                        });
                }
                let section = if animations {
                    "Animations"
                } else {
                    &self.section
                };
                let types = scene
                    .catalog
                    .object_types
                    .get(section)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let mut matching: Vec<_> = types
                    .iter()
                    .filter(|name| filter.is_empty() || name.to_ascii_lowercase().contains(&filter))
                    .collect();
                matching.sort_by_key(|id| {
                    scene
                        .sprites
                        .get(section, id)
                        .is_none_or(|p| p.image.is_none())
                });
                ui.label(
                    RichText::new(format!("{} / {} 个对象", matching.len(), types.len()))
                        .color(theme::MUTED)
                        .size(11.),
                );
                if animations {
                    ui.small("动画预览；游戏中的生成由规则或触发器控制。");
                }
                egui::ScrollArea::vertical()
                    .id_salt("object-palette")
                    .max_height((ui.available_height() - 12.).max(90.))
                    .show_rows(ui, 40., matching.len(), |ui, rows| {
                        for index in rows {
                            let id = matching[index];
                            let section = if animations {
                                "Animations"
                            } else {
                                self.section.as_str()
                            };
                            let texture = sprite_texture(
                                &mut self.ui_state.sprites,
                                ui.ctx(),
                                scene,
                                section,
                                id,
                            );
                            let detail = scene
                                .sprites
                                .get(section, id)
                                .map(|p| p.detail.as_str())
                                .unwrap_or("无图像");
                            ui.horizontal(|ui| {
                                theme::thumbnail(ui, texture.as_ref(), detail);
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 40.],
                                        egui::Button::new(id)
                                            .truncate()
                                            .selected(self.type_id == *id),
                                    )
                                    .on_hover_ui(|ui| {
                                        ui.label(detail);
                                        if let Some(texture) = sprite_texture(
                                            &mut self.ui_state.sprites,
                                            ui.ctx(),
                                            scene,
                                            section,
                                            id,
                                        ) {
                                            ui.image((
                                                texture.id(),
                                                fit_size(
                                                    texture.size_vec2(),
                                                    egui::vec2(240., 180.),
                                                ),
                                            ));
                                        }
                                    })
                                    .clicked()
                                {
                                    self.type_id = id.clone();
                                    self.tool = if animations {
                                        Tool::Select
                                    } else {
                                        Tool::Place
                                    };
                                }
                            });
                        }
                    });
            }
            3 => {
                ui.small("墙、桥、箱子及其他覆盖物；每格一个类型。");
                let entries: Vec<_> = scene
                    .catalog
                    .overlays
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| {
                        filter.is_empty() || id.to_ascii_lowercase().contains(&filter)
                    })
                    .collect();
                ui.small(format!(
                    "{} / {} 个覆盖物",
                    entries.len(),
                    scene.catalog.overlays.len()
                ));
                egui::ScrollArea::vertical()
                    .id_salt("overlay-palette")
                    .max_height((ui.available_height() - 12.).max(90.))
                    .show_rows(ui, 40., entries.len(), |ui, rows| {
                        for row in rows {
                            let (index, id) = entries[row];
                            let texture = sprite_texture(
                                &mut self.ui_state.sprites,
                                ui.ctx(),
                                scene,
                                "Overlay",
                                id,
                            );
                            let detail = scene
                                .sprites
                                .get("Overlay", id)
                                .map(|p| p.detail.as_str())
                                .unwrap_or("无图像");
                            ui.horizontal(|ui| {
                                theme::thumbnail(ui, texture.as_ref(), detail);
                                let response = ui
                                    .add_enabled_ui(index < 255, |ui| {
                                        ui.add_sized(
                                            [ui.available_width(), 40.],
                                            egui::Button::new(format!("{index}: {id}"))
                                                .truncate()
                                                .selected(
                                                    self.tool == Tool::Overlay
                                                        && usize::from(self.overlay_id) == index,
                                                ),
                                        )
                                    })
                                    .inner
                                    .on_hover_text(detail);
                                if response.clicked() {
                                    self.overlay_id = index as u8;
                                    self.overlay_frame = 0;
                                    self.tool = Tool::Overlay;
                                }
                            });
                        }
                    });
            }
            _ => {
                ui.label(RichText::new("遭遇战起点").color(theme::MUTED).size(11.));
                egui::ScrollArea::vertical()
                    .id_salt("player-start-palette")
                    .max_height(110.)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for index in 0..u32::from(crate::new_map::MAX_PLAYERS) {
                                if ui
                                    .add_sized(
                                        [44., 30.],
                                        egui::Button::new(format!("{:02}", index))
                                            .selected(self.waypoint == index),
                                    )
                                    .clicked()
                                {
                                    self.waypoint = index;
                                    self.tool = Tool::Waypoint;
                                }
                            }
                        })
                    });
                ui.add_space(8.);
                ui.label(RichText::new("地图航点").color(theme::MUTED).size(11.));
                let entries: Vec<_> = self
                    .records
                    .iter()
                    .filter(|(_, r)| r.section == "Waypoints" && r.cell().is_some())
                    .map(|(&id, r)| (id, r.key.clone(), r.cell().unwrap()))
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt("waypoint-palette")
                    .max_height((ui.available_height() - 20.).max(80.))
                    .show_rows(ui, 28., entries.len(), |ui, rows| {
                        for index in rows {
                            let (id, key, cell) = &entries[index];
                            if ui
                                .add_sized(
                                    [ui.available_width(), 28.],
                                    egui::Button::new(format!(
                                        "航点 {key}     {}, {}",
                                        cell[0], cell[1]
                                    ))
                                    .selected(self.selected == Some(*id)),
                                )
                                .clicked()
                            {
                                self.select_from_canvas(Some(*id));
                                self.tool = Tool::Select;
                            }
                        }
                    });
            }
        }
    }
    pub fn tool_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        theme::section(ui, "工具选项", "PROPERTIES");
        egui::Frame::new()
            .fill(theme::SURFACE)
            .corner_radius(5)
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::hover());
                    theme::icon(ui.painter(), rect, tool_icon(self.tool), theme::ACCENT);
                    ui.label(
                        RichText::new(self.tool.name())
                            .strong()
                            .color(theme::ACCENT),
                    );
                });
            });
        if self.doc.is_none() {
            ui.label(
                RichText::new(
                    scene
                        .source
                        .edit_base
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("当前地图仅支持查看"),
                )
                .color(theme::MUTED),
            );
            return;
        }
        if self.tool.brush()
            && !(self.tool == Tool::Terrain
                && (matches!(self.terrain_mode, TerrainMode::Template | TerrainMode::Fill)
                    || self.terrain_mode.planned()))
        {
            ui.add_space(4.);
            ui.label(RichText::new("笔刷大小").color(theme::MUTED).size(11.));
            ui.add(egui::Slider::new(&mut self.radius, 1..=5).text("格"));
        }
        match self.tool {
            Tool::Terrain => {
                if let Some(brush) = scene.brushes.get(self.brush)
                    && !self.terrain_mode.planned()
                {
                    theme::field(
                        ui,
                        "地形编号",
                        format!("{} : {}", brush.key.tile_id, brush.key.sub_tile),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(&brush.label).size(11.).color(theme::MUTED))
                            .wrap(),
                    );
                }
                ui.add_space(6.);
                egui::ComboBox::from_id_salt("terrain-mode")
                    .selected_text(self.terrain_mode.name())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for mode in TerrainMode::ALL {
                            ui.selectable_value(&mut self.terrain_mode, mode, mode.name());
                        }
                    });
                if !self.terrain_mode.planned() {
                    ui.checkbox(&mut self.auto_lat, "自动地表过渡（LAT）");
                }
                if self.terrain_mode == TerrainMode::Bridge {
                    egui::ComboBox::from_id_salt("bridge-kind")
                        .selected_text(self.bridge_kind.name())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for kind in ra2_map_scene::bridge_tools::BridgeKind::ALL {
                                ui.selectable_value(&mut self.bridge_kind, kind, kind.name());
                            }
                        });
                    ui.small("从一端陆地拖到另一端，自动吸附两个地图轴向。高桥两端等高，桥下低 4 层；低桥与水面等高。");
                    ui.small(
                        "自动放置桥面；高桥同时替换桥头。绿色可放置，红色说明原因，Esc 取消。",
                    );
                } else if self.terrain_mode.planned() {
                    self.plateau_options(ui, scene);
                } else if self.terrain_mode.edits_height() {
                    ui.checkbox(&mut self.smooth_heights, "自动生成连续坡面");
                    if self.terrain_mode == TerrainMode::Flatten {
                        ui.add(
                            egui::Slider::new(
                                &mut self.flatten_level,
                                0..=if self.smooth_heights { 12 } else { 14 },
                            )
                            .text("目标高度"),
                        );
                    }
                    ui.small("拖动选择区域，释放后整笔塑形。每格每笔只升降一次。");
                } else if self.terrain_mode == TerrainMode::Template {
                    self.terrain_template_options(ui, scene);
                } else if self.terrain_mode == TerrainMode::Fill {
                    ui.small("点击填充相连且等高的同类地面；请选择 1×1 模板。");
                } else {
                    ui.small("拖动连续绘制，保留原有高度。");
                }
                ui.small("Esc 取消本笔；Ctrl+Z 撤销所有邻接变化。");
            }
            Tool::Ore | Tool::Gems => {
                ui.add_space(4.);
                ui.label(RichText::new("矿石储量阶段").color(theme::MUTED).size(11.));
                ui.add(egui::Slider::new(&mut self.overlay_frame, 0..=11));
            }
            Tool::Overlay => {
                let id = scene
                    .catalog
                    .overlays
                    .get(self.overlay_id as usize)
                    .map(String::as_str)
                    .unwrap_or("未知");
                theme::field(ui, "覆盖物", id);
                let count = scene
                    .sprites
                    .get("Overlay", id)
                    .and_then(|p| p.image.as_ref())
                    .map_or(256, |i| i.info.count)
                    .clamp(1, 256);
                ui.add(
                    egui::Slider::new(&mut self.overlay_frame, 0..=(count - 1) as u8)
                        .text("帧 / 连接阶段"),
                );
                ui.checkbox(&mut self.auto_walls, "自动连接同类墙体");
                ui.small("拖动放置；桥梁连接帧需手动选择。");
            }
            Tool::Region | Tool::Paste | Tool::RegionMove => {
                self.region_options(ui, scene);
            }
            Tool::Clone => {
                ui.checkbox(&mut self.allow_overlap, "允许对象重叠");
                ui.small("点击目标位置克隆完整属性，Esc 退出。");
            }
            Tool::Place => {
                theme::field(ui, "分类", category(&self.section));
                theme::field(ui, "类型", &self.type_id);
                ui.label(RichText::new("所属方").size(11.).color(theme::MUTED));
                let houses = self.houses(scene);
                Self::house_picker(ui, "placement-owner", &mut self.owner, &houses);
                if self.section == "Infantry" {
                    ui.add(egui::Slider::new(&mut self.infantry_subcell, 0..=4).text("子格"));
                }
                ui.checkbox(&mut self.allow_overlap, "允许对象重叠");
                ui.small("轮廓显示实际占地；红色表示越界、占用或高低不平。");
            }
            Tool::Waypoint => {
                ui.add(
                    egui::DragValue::new(&mut self.waypoint)
                        .range(0..=701)
                        .prefix("航点编号  "),
                );
                ui.small(format!(
                    "0–{} 为 VERA20k 玩家起点，点击地图设置位置。",
                    crate::new_map::MAX_PLAYERS - 1
                ));
            }
            Tool::Move => {
                ui.checkbox(&mut self.allow_overlap, "允许对象重叠");
                ui.small("先选择对象，再点击地图上的目标位置。");
            }
            Tool::Select => {
                ui.small("点击格子检查地形，点击对象图像编辑属性。");
            }
            Tool::Pick => {
                ui.small("点击地图吸取地形，并切换到地形画笔。");
            }
            Tool::Erase => {
                ui.checkbox(&mut self.auto_walls, "自动重连剩余墙体");
                ui.small("拖动清除格子中的矿石或其他覆盖物。");
            }
        }
    }
}
fn category(section: &str) -> &str {
    match section {
        "Structures" => "建筑",
        "Units" => "车辆",
        "Infantry" => "步兵",
        "Aircraft" => "飞机",
        "Terrain" => "树木 / 地形对象",
        "Smudge" => "弹坑 / 污迹",
        _ => section,
    }
}

fn fit_size(size: egui::Vec2, bound: egui::Vec2) -> egui::Vec2 {
    size * (bound.x / size.x.max(1.))
        .min(bound.y / size.y.max(1.))
        .min(1.)
}
pub(super) fn sprite_texture(
    cache: &mut HashMap<(String, String), TextureHandle>,
    ctx: &egui::Context,
    scene: &MapScene,
    section: &str,
    id: &str,
) -> Option<TextureHandle> {
    sprite_texture_frame(cache, ctx, scene, section, id, None)
}
pub(super) fn sprite_texture_frame(
    cache: &mut HashMap<(String, String), TextureHandle>,
    ctx: &egui::Context,
    scene: &MapScene,
    section: &str,
    id: &str,
    frame: Option<u8>,
) -> Option<TextureHandle> {
    let key = (format!("{section}:{frame:?}"), id.to_owned());
    if let Some(texture) = cache.get(&key) {
        return Some(texture.clone());
    }
    let image = if let Some(frame) = frame {
        scene.sprites.overlay_frames.get(&(id.to_owned(), frame))?
    } else {
        scene.sprites.get(section, id)?.image.as_ref()?
    };
    if cache.len() >= 192 {
        cache.clear();
    }
    let texture = ctx.load_texture(
        format!("sprite-{section}-{id}"),
        egui::ColorImage::from_rgba_unmultiplied(
            [image.info.width as usize, image.info.height as usize],
            &image.rgba,
        ),
        egui::TextureOptions::NEAREST,
    );
    cache.insert(key, texture.clone());
    Some(texture)
}
