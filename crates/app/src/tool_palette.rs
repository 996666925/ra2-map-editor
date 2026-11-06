//! Direct tool entry points shared by the workbench and terrain workspace.
use super::*;
use crate::i18n::tr;
use crate::ui_theme::{self as theme, Icon};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum TerrainPage {
    #[default]
    Ground,
    Cliffs,
    Water,
    Roads,
}

impl TerrainPage {
    fn for_mode(mode: TerrainMode) -> Self {
        match mode {
            TerrainMode::Subtile
            | TerrainMode::Template
            | TerrainMode::Fill
            | TerrainMode::Raise
            | TerrainMode::Lower
            | TerrainMode::Flatten => Self::Ground,
            TerrainMode::Plateau
            | TerrainMode::CliffOutline
            | TerrainMode::CliffFront
            | TerrainMode::CliffBack
            | TerrainMode::PlateauCut
            | TerrainMode::RampEntrance => Self::Cliffs,
            TerrainMode::Water
            | TerrainMode::WaterOutline
            | TerrainMode::LandFill
            | TerrainMode::ShoreRepair
            | TerrainMode::CoastalCliff => Self::Water,
            TerrainMode::Road | TerrainMode::RoadCut | TerrainMode::Bridge => Self::Roads,
        }
    }

    fn modes(self) -> &'static [(TerrainMode, &'static str)] {
        use TerrainMode::*;
        match self {
            Self::Ground => &[
                (Subtile, "地表画笔"),
                (Fill, "铺地 G"),
                (Template, "手工模板"),
                (Raise, "升高"),
                (Lower, "降低"),
                (Flatten, "平整高度"),
            ],
            Self::Cliffs => &[
                (CliffOutline, "围合悬崖 C"),
                (Plateau, "矩形高台 H"),
                (CliffFront, "正面悬崖"),
                (CliffBack, "背面悬崖"),
                (RampEntrance, "坡道入口"),
                (PlateauCut, "切除高台"),
            ],
            Self::Water => &[
                (WaterOutline, "围合水域"),
                (Water, "矩形水域"),
                (LandFill, "回填陆地"),
                (ShoreRepair, "修复岸线"),
                (CoastalCliff, "临水悬崖"),
            ],
            Self::Roads => &[
                (Road, "绘制道路"),
                (RoadCut, "切除道路"),
                (Bridge, "架设桥梁"),
            ],
        }
    }
}

impl Editor {
    pub(crate) fn activate_terrain_mode(&mut self, mode: TerrainMode) {
        if self.doc.is_none() || self.stroking {
            return;
        }
        if mode == TerrainMode::Fill && (self.tool != Tool::Terrain || self.terrain_mode != mode) {
            self.show_library(0, "", "");
        }
        if matches!(
            mode,
            TerrainMode::Subtile | TerrainMode::Template | TerrainMode::Fill
        ) {
            self.ui_state.terrain_subtiles = mode == TerrainMode::Subtile;
        }
        self.activate_tool(Tool::Terrain);
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        self.terrain_mode = mode;
        self.ui_state.terrain_page = TerrainPage::for_mode(mode);
        self.status = tr!(
            "{arg0}；在左侧调整参数，Esc 取消当前操作。",
            arg0 = crate::i18n::text(mode.name())
        );
    }

    /// Selecting a fill material keeps rectangular ground painting active.
    pub(super) fn select_terrain_material(&mut self, index: usize, subtiles: bool) {
        if self.doc.is_none() || self.stroking {
            return;
        }
        let mode = if self.tool == Tool::Terrain && self.terrain_mode == TerrainMode::Fill {
            TerrainMode::Fill
        } else if subtiles {
            TerrainMode::Subtile
        } else {
            TerrainMode::Template
        };
        self.activate_terrain_mode(mode);
        self.brush = index;
    }

    pub(crate) fn quick_tools(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.doc.is_some() && !self.stroking, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr!("工具")).color(theme::MUTED));
                for (tool, icon, label) in [
                    (Tool::Select, Icon::Select, tr!("选择")),
                    (Tool::Region, Icon::Select, tr!("框选")),
                    (Tool::Move, Icon::Move, tr!("移动")),
                    (Tool::Terrain, Icon::Terrain, tr!("地形")),
                    (Tool::Pick, Icon::Pick, tr!("吸管")),
                    (Tool::Place, Icon::Building, tr!("对象")),
                    (Tool::Ore, Icon::Ore, tr!("矿石")),
                    (Tool::Gems, Icon::Gem, tr!("宝石")),
                    (Tool::Waypoint, Icon::Flag, tr!("航点")),
                    (Tool::Erase, Icon::Erase, tr!("擦除")),
                ] {
                    if theme::button(ui, icon, label, 72., self.tool == tool)
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
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr!("地形")).color(theme::MUTED));
                for (mode, label) in [
                    (TerrainMode::Subtile, tr!("画笔")),
                    (TerrainMode::Fill, tr!("铺地 G")),
                    (TerrainMode::CliffOutline, tr!("围合悬崖 C")),
                    (TerrainMode::CliffFront, tr!("悬崖线")),
                    (TerrainMode::Plateau, tr!("高台 H")),
                    (TerrainMode::RampEntrance, tr!("坡道")),
                    (TerrainMode::WaterOutline, tr!("水域")),
                    (TerrainMode::Road, tr!("道路")),
                ] {
                    if ui
                        .add(
                            egui::Button::new(label)
                                .selected(self.tool == Tool::Terrain && self.terrain_mode == mode),
                        )
                        .on_hover_text(crate::i18n::text(mode.name()))
                        .clicked()
                    {
                        self.activate_terrain_mode(mode);
                    }
                }
            });
        });
    }

    pub(super) fn terrain_tools(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (page, label) in [
                (TerrainPage::Ground, tr!("地表")),
                (TerrainPage::Cliffs, tr!("悬崖")),
                (TerrainPage::Water, tr!("水岸")),
                (TerrainPage::Roads, tr!("道路 / 桥")),
            ] {
                ui.selectable_value(&mut self.ui_state.terrain_page, page, label);
            }
        });
        if self.ui_state.terrain_page == TerrainPage::Ground
            && self.tool == Tool::Terrain
            && matches!(
                self.terrain_mode,
                TerrainMode::Subtile | TerrainMode::Template | TerrainMode::Fill
            )
        {
            egui::CollapsingHeader::new(tr!("画笔、填充与高度工具"))
                .show(ui, |ui| self.terrain_mode_buttons(ui));
        } else {
            self.terrain_mode_buttons(ui);
        }
    }

    fn terrain_mode_buttons(&mut self, ui: &mut egui::Ui) {
        let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.;
        ui.add_enabled_ui(self.doc.is_some() && !self.stroking, |ui| {
            for pair in self.ui_state.terrain_page.modes().chunks(2) {
                ui.horizontal(|ui| {
                    for &(mode, label) in pair {
                        if ui
                            .add_sized(
                                [width, 28.],
                                egui::Button::new(crate::i18n::text(label)).wrap().selected(
                                    self.tool == Tool::Terrain && self.terrain_mode == mode,
                                ),
                            )
                            .on_hover_text(crate::i18n::text(mode.name()))
                            .clicked()
                        {
                            self.activate_terrain_mode(mode);
                        }
                    }
                });
            }
        });
    }
}
