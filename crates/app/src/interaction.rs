//! Shared tool commands, cancellation and guidance for buttons and shortcuts.
use crate::editing::{Editor, Tool};
use crate::map::MapScene;
use eframe::egui;
use ra2_map_scene::terrain_tools::TerrainMode;

pub(crate) const TOOL_KEYS: [(egui::Key, Tool, &str); 9] = [
    (egui::Key::V, Tool::Select, "V"),
    (egui::Key::R, Tool::Region, "R"),
    (egui::Key::B, Tool::Terrain, "B"),
    (egui::Key::I, Tool::Pick, "I"),
    (egui::Key::M, Tool::Move, "M"),
    (egui::Key::P, Tool::Place, "P"),
    (egui::Key::W, Tool::Waypoint, "W"),
    (egui::Key::E, Tool::Erase, "E"),
    (egui::Key::O, Tool::Ore, "O"),
];

pub(crate) const TERRAIN_KEYS: [(egui::Key, TerrainMode, &str); 3] = [
    (egui::Key::G, TerrainMode::Fill, "G"),
    (egui::Key::C, TerrainMode::CliffOutline, "C"),
    (egui::Key::H, TerrainMode::Plateau, "H"),
];

impl Tool {
    pub(crate) fn hint(self) -> &'static str {
        match self {
            Self::Select => "单击选择格子或对象；M 移动，Ctrl+D 克隆，Delete 删除",
            Self::Region => "拖动框选；Shift 添加，Alt 减选；Ctrl+C/X/V 复制、剪切、粘贴",
            Self::Paste => "单击放置区域，可连续粘贴；Esc 或右键退出",
            Self::RegionMove => "单击目标位置移动整个区域；Esc 或右键取消",
            Self::Clone => "单击克隆完整对象，可连续放置；Esc 或右键退出",
            Self::Move => "单击目标位置移动所选对象；Esc 或右键取消",
            Self::Place => "单击放置对象；红色轮廓说明不可放置的原因；Esc 退出",
            Self::Pick => "单击吸取地形，随后回到地形画笔",
            Self::Waypoint => "单击设置当前航点；编号已存在时会移动该航点",
            Self::Erase => "拖动擦除覆盖物与资源；Ctrl+Z 可撤销整笔",
            Self::Terrain => "选择地形模式后绘制；松开提交，Esc 或右键取消当前一笔",
            Self::Ore | Self::Gems | Self::Overlay => {
                "拖动绘制；[ / ] 调整笔刷，松开提交，Esc 取消"
            }
        }
    }
    pub(crate) fn shortcut(self) -> &'static str {
        TOOL_KEYS
            .iter()
            .find(|(_, tool, _)| *tool == self)
            .map_or("", |(_, _, key)| *key)
    }
}

impl Editor {
    pub(crate) fn activate_tool(&mut self, tool: Tool) {
        if self.stroking {
            return;
        }
        if matches!(tool, Tool::Move | Tool::Clone) && self.selected.is_none() {
            self.status = "请先单击选择对象或航点。".into();
            return;
        }
        if tool == Tool::Clone
            && self
                .selected
                .and_then(|id| self.records.get(&id))
                .is_some_and(|r| r.section == "Waypoints")
        {
            self.status = "航点请使用航点工具新建，或随区域复制。".into();
            return;
        }
        if tool == Tool::Paste && self.region.clipboard.is_none() {
            self.status = "请先框选区域，再按 Ctrl+C 复制。".into();
            return;
        }
        if tool == Tool::RegionMove && self.region.cells.is_empty() {
            self.status = "请先框选需要移动的区域。".into();
            return;
        }
        if matches!(tool, Tool::Move | Tool::Clone | Tool::Place) && self.object_properties_dirty()
        {
            self.status = "请先应用或放弃当前对象的属性修改。".into();
            return;
        }
        self.tool = tool;
        self.feedback.clear();
        self.status = tool.hint().into();
        self.ui_state.tab = match tool {
            Tool::Terrain | Tool::Pick => 0,
            Tool::Place => 1,
            Tool::Waypoint => 2,
            Tool::Ore | Tool::Gems | Tool::Overlay | Tool::Erase => 3,
            _ => return,
        };
    }

    /// Cancel just the in-progress transaction; never pop committed history.
    pub(crate) fn cancel_active(&mut self, scene: &MapScene) -> bool {
        if !self.stroking {
            return false;
        }
        self.plateau.cancel();
        self.region.cancel();
        if let Some(doc) = &mut self.doc {
            doc.cancel();
        }
        self.stroking = false;
        self.last = None;
        self.feedback.clear();
        self.stroke = None;
        self.stroke_cells.clear();
        self.cache(scene);
        self.status = "已取消当前操作；之前完成的修改已保留。".into();
        true
    }

    pub(crate) fn cancel_operation(&mut self, scene: &MapScene) {
        if self.cancel_active(scene) {
            return;
        }
        self.plateau.cancel();
        self.region.cancel();
        self.feedback.clear();
        self.tool = Tool::Select;
        self.status = "已返回选择工具。".into();
    }

    pub(crate) fn tool_shortcuts(&mut self, ctx: &egui::Context) {
        if self.stroking {
            return;
        }
        let tool = ctx.input_mut(|i| {
            if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt || i.modifiers.shift {
                return None;
            }
            TOOL_KEYS.iter().find_map(|&(key, tool, _)| {
                i.consume_key(egui::Modifiers::NONE, key).then_some(tool)
            })
        });
        if let Some(tool) = tool {
            self.activate_tool(tool);
        }
        let mode = ctx.input_mut(|i| {
            if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt || i.modifiers.shift {
                return None;
            }
            TERRAIN_KEYS.iter().find_map(|&(key, mode, _)| {
                i.consume_key(egui::Modifiers::NONE, key).then_some(mode)
            })
        });
        if let Some(mode) = mode {
            self.activate_terrain_mode(mode);
        }
        if matches!(
            self.tool,
            Tool::Ore | Tool::Gems | Tool::Overlay | Tool::Erase
        ) || self.tool == Tool::Terrain
            && self.terrain_mode == ra2_map_scene::terrain_tools::TerrainMode::Subtile
        {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket) {
                    self.radius = self.radius.saturating_sub(1).max(1);
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket) {
                    self.radius = (self.radius + 1).min(5);
                }
            });
        }
    }

    pub(crate) fn context_actions(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        if ui.button("选择工具   V").clicked() {
            self.activate_tool(Tool::Select);
            ui.close();
        }
        for (tool, label) in [
            (Tool::Move, "移动所选对象   M"),
            (Tool::Clone, "克隆所选对象   Ctrl+D"),
        ] {
            let available = self
                .selected
                .and_then(|id| self.records.get(&id))
                .is_some_and(|r| tool != Tool::Clone || r.section != "Waypoints");
            if ui
                .add_enabled(available, egui::Button::new(label))
                .clicked()
            {
                self.activate_tool(tool);
                ui.close();
            }
        }
        if ui
            .add_enabled(
                self.selected.is_some(),
                egui::Button::new("删除所选对象 / 航点"),
            )
            .clicked()
        {
            self.delete(scene);
            ui.close();
        }
        ui.separator();
        for (action, label, available) in [
            (
                super::region_edit::Action::Copy,
                "复制所选区域   Ctrl+C",
                !self.region.cells.is_empty(),
            ),
            (
                super::region_edit::Action::Cut,
                "剪切所选区域   Ctrl+X",
                !self.region.cells.is_empty(),
            ),
            (
                super::region_edit::Action::Move,
                "移动所选区域",
                !self.region.cells.is_empty(),
            ),
            (
                super::region_edit::Action::Paste,
                "粘贴已复制区域   Ctrl+V",
                self.region.clipboard.is_some(),
            ),
        ] {
            if ui
                .add_enabled(available, egui::Button::new(label))
                .clicked()
            {
                self.region_action(scene, action);
                if matches!(
                    action,
                    super::region_edit::Action::Copy | super::region_edit::Action::Cut
                ) {
                    self.export_region_clipboard(ui.ctx());
                }
                ui.close();
            }
        }
    }
}

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;
