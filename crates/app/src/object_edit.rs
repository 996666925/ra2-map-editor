//! Object forms patch selected CSV columns and preserve every other byte.
use super::*;
use ra2_map_document::object_properties::{self, Kind};

impl Editor {
    pub(crate) fn check_property_draft(&mut self) -> bool {
        if !self.object_properties_dirty() {
            return true;
        }
        self.status = "对象属性尚未应用；请先点击“应用属性”或“放弃属性修改”。".into();
        false
    }
    pub(crate) fn object_properties_dirty(&self) -> bool {
        let Some(r) = self.selected.and_then(|id| self.records.get(&id)) else {
            return false;
        };
        if r.section == "Waypoints" {
            return false;
        }
        let f = r.fields();
        self.property_type != r.type_id()
            || (!matches!(r.section.as_str(), "Terrain" | "Smudge")
                && (f.first() != Some(&self.property_owner)
                    || f.get(2).and_then(|n| n.parse().ok()).unwrap_or(256)
                        != self.property_health
                    || f.get(if r.section == "Infantry" { 7 } else { 5 })
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0)
                        != self.property_facing
                    || self
                        .property_extra
                        .iter()
                        .any(|(index, v)| f.get(*index) != Some(v))))
    }
    pub(crate) fn select_from_canvas(&mut self, id: Option<u64>) -> bool {
        if self.selected != id && self.object_properties_dirty() {
            self.status = "对象属性尚未应用；请先点击“应用属性”或“放弃属性修改”。".into();
            return false;
        }
        if self.selected != id {
            self.select(id);
        }
        true
    }
    pub(crate) fn select(&mut self, id: Option<u64>) {
        self.selected = id;
        self.property_extra.clear();
        if let Some(r) = id.and_then(|id| self.records.get(&id)) {
            let f = r.fields();
            self.property_owner = f.first().cloned().unwrap_or_default();
            self.property_type = r.type_id();
            self.property_health = f.get(2).and_then(|f| f.parse().ok()).unwrap_or(256);
            self.property_facing = f
                .get(if r.section == "Infantry" { 7 } else { 5 })
                .and_then(|f| f.parse().ok())
                .unwrap_or(0);
            for field in object_properties::fields(&r.section) {
                if let Some(value) = f.get(field.index) {
                    self.property_extra.insert(field.index, value.clone());
                }
            }
        }
    }
    pub(super) fn houses(&self, scene: &MapScene) -> Vec<String> {
        let mut houses = BTreeSet::from(["Neutral".to_owned(), "Special".to_owned()]);
        if let Some(doc) = &self.doc {
            houses.extend(ra2_map_document::mission::ids(
                doc,
                ra2_map_document::mission::Kind::House,
            ));
            for r in self.records.values().filter(|r| {
                matches!(
                    r.section.as_str(),
                    "Structures" | "Units" | "Infantry" | "Aircraft"
                )
            }) {
                if let Some(owner) = r.fields().first() {
                    houses.insert(owner.clone());
                }
            }
        }
        for name in scene
            .catalog
            .rules
            .section_or_empty("Countries")
            .registry_ids()
        {
            houses.insert(name.to_owned());
        }
        houses.into_iter().filter(|s| !s.is_empty()).collect()
    }
    pub(super) fn house_picker(ui: &mut egui::Ui, id: &str, value: &mut String, houses: &[String]) {
        egui::ComboBox::from_id_salt(id)
            .selected_text(value.as_str())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for house in houses {
                    ui.selectable_value(value, house.clone(), house);
                }
            });
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(ui.available_width())
                .hint_text("所属方 ID"),
        );
    }
    pub(super) fn apply_object_properties(
        &mut self,
        scene: &MapScene,
        r: &Record,
        id: u64,
    ) -> Result<()> {
        if self.property_type != r.type_id() {
            ensure!(
                scene
                    .catalog
                    .object_types
                    .get(&r.section)
                    .is_some_and(|types| types.contains(&self.property_type)),
                "对象类型不在当前目录中"
            );
        }
        let old = r.fields();
        let mut edits = Vec::new();
        let type_index = if matches!(r.section.as_str(), "Terrain" | "Smudge") {
            0
        } else {
            1
        };
        if old.get(type_index) != Some(&self.property_type) {
            edits.push((type_index, self.property_type.clone()));
        }
        if !matches!(r.section.as_str(), "Terrain" | "Smudge" | "Waypoints") {
            ensure!(
                old.first() == Some(&self.property_owner)
                    || (!self.property_owner.is_empty()
                        && self
                            .property_owner
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')),
                "所属方必须是 ASCII ID"
            );
            for (index, value) in [
                (0, self.property_owner.clone()),
                (2, self.property_health.to_string()),
                (
                    if r.section == "Infantry" { 7 } else { 5 },
                    self.property_facing.to_string(),
                ),
            ] {
                if old.get(index).is_none_or(|v| *v != value) {
                    edits.push((index, value));
                }
            }
            for field in object_properties::fields(&r.section) {
                if let Some(value) = self.property_extra.get(&field.index)
                    && old.get(field.index) != Some(value)
                {
                    match field.kind {
                        Kind::Number(min, max) => {
                            let n = value.parse::<i32>().context("属性必须为整数")?;
                            ensure!((min..=max).contains(&n), "{} 超出范围", field.name);
                        }
                        Kind::Flag => {
                            ensure!(matches!(value.as_str(), "0" | "1"), "布尔标志须为 0 或 1")
                        }
                        Kind::Text => {
                            if field.index == 6 && r.section != "Structures" {
                                ensure!(value.eq_ignore_ascii_case("None") || vera20k_core::rules::mission_data::MissionType::from_map_name(value).is_some(),"初始任务名称无效");
                            }
                        }
                    }
                    edits.push((field.index, value.clone()));
                }
            }
        }
        let changed = r.patched(&edits)?;
        let doc = self.doc.as_mut().context("没有地图")?;
        let geometry_changed = edits.iter().any(|(index, _)| {
            *index == type_index
                || (r.section == "Infantry" && matches!(index, 5 | 11))
                || (r.section == "Units" && *index == 10)
        });
        if geometry_changed {
            ra2_map_scene::placement::validate(
                doc,
                scene,
                std::slice::from_ref(&changed),
                &BTreeSet::from([id]),
                self.allow_overlap,
                &BTreeMap::new(),
            )?;
        }
        doc.set(Target::Record(id), Some(Value::Record(changed)))?;
        self.commit(scene, "修改对象属性");
        self.select(Some(id));
        Ok(())
    }
    pub fn selection_properties(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        let Some((id, r)) = self
            .selected
            .and_then(|id| self.records.get(&id).cloned().map(|r| (id, r)))
        else {
            return;
        };
        let type_id = r.type_id();
        crate::ui_theme::field(
            ui,
            "对象类型",
            if r.section == "Waypoints" {
                std::borrow::Cow::Borrowed(type_id.as_str())
            } else {
                scene.catalog.object_label(&type_id)
            },
        );
        crate::ui_theme::field(ui, "记录编号", &r.key);
        if let Some(at) = r.cell() {
            crate::ui_theme::field(ui, "坐标", format!("{}, {}", at[0], at[1]));
        }
        if r.section != "Waypoints" {
            let dirty = self.object_properties_dirty();
            if dirty {
                ui.colored_label(
                    crate::ui_theme::ACCENT,
                    "属性有未应用的修改；保存地图前请先应用。",
                );
            }
            ui.label("类型");
            ui.text_edit_singleline(&mut self.property_type);
            ui.small(scene.catalog.object_name(&self.property_type));
            if !matches!(r.section.as_str(), "Terrain" | "Smudge") {
                let houses = self.houses(scene);
                ui.label("所属方");
                Self::house_picker(ui, "object-owner", &mut self.property_owner, &houses);
                ui.add(egui::Slider::new(&mut self.property_health, 0..=256).text("生命"));
                ui.add(egui::Slider::new(&mut self.property_facing, 0..=255).text("朝向"));
                egui::CollapsingHeader::new("分类属性")
                    .default_open(true)
                    .show(ui, |ui| {
                        for field in object_properties::fields(&r.section) {
                            let Some(value) = self.property_extra.get_mut(&field.index) else {
                                continue;
                            };
                            match field.kind {
                                Kind::Flag => {
                                    let mut enabled = value.parse::<i32>().is_ok_and(|n| n != 0);
                                    if ui.checkbox(&mut enabled, field.name).changed() {
                                        *value = if enabled { "1" } else { "0" }.into();
                                    }
                                }
                                Kind::Number(min, max) => {
                                    let mut n = value.parse::<i32>().unwrap_or(min);
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut n)
                                                .range(min..=max)
                                                .prefix(format!("{}  ", field.name)),
                                        )
                                        .changed()
                                    {
                                        *value = n.to_string();
                                    }
                                }
                                Kind::Text => {
                                    ui.label(field.name);
                                    ui.text_edit_singleline(value);
                                }
                            }
                        }
                    });
            }
            ui.checkbox(&mut self.allow_overlap, "允许对象重叠");
            if ui
                .add_enabled(
                    self.object_properties_dirty(),
                    crate::ui_theme::primary("应用属性"),
                )
                .clicked()
                && let Err(e) = self.apply_object_properties(scene, &r, id)
            {
                self.status = format!("未修改：{e:#}");
            }
            if self.object_properties_dirty() && ui.button("放弃属性修改").clicked() {
                self.select(Some(id));
            }
            if ui
                .add_enabled(
                    !self.object_properties_dirty(),
                    egui::Button::new("克隆对象  Ctrl+D"),
                )
                .on_disabled_hover_text("先应用或放弃属性修改，克隆才会使用正确的属性。")
                .clicked()
            {
                self.tool = Tool::Clone;
            }
        }
        if ui.button("删除选中对象 / 航点").clicked() {
            self.delete(scene);
        }
    }
}
