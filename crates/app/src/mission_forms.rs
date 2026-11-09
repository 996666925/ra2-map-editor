use super::*;
use crate::i18n::tr;
use mission::schema::{self, ParameterKind as P};

#[derive(Clone)]
pub(super) struct Pick {
    pub key: String,
    pub row: Option<usize>,
    pub index: usize,
    pub kind: P,
}

/// Raw text remains available for extensions and unlisted operands.
pub(super) fn parameter(
    ui: &mut egui::Ui,
    p: schema::Parameter,
    value: &mut String,
    doc: &ra2_map_document::document::MapDocument,
    scene: &MapScene,
) -> bool {
    let mut pick = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(crate::i18n::text(p.label));
        let choices = choices::get(ui.ctx(), p.kind, doc, scene);
        let mut resource_choices = None;
        match p.kind {
            P::Integer { min, max, unit } => {
                let unit = crate::i18n::text(unit);
                ui.small(format!("{min}–{max} {unit}"));
            }
            P::Waypoint { .. } => {
                pick = ui.small_button(tr!("从地图选取")).clicked();
            }
            P::Type(_) => {
                pick = ui.small_button(tr!("从地图选对象")).clicked();
            }
            P::Reference(_) | P::Country => {}
            P::Csf | P::Sound | P::Eva | P::Theme => {
                let key = match p.kind {
                    P::Csf => "Csf",
                    P::Sound => "Sound",
                    P::Eva => "Eva",
                    _ => "Theme",
                };
                if let Some(items) = scene.mission_resources.choices.get(key) {
                    resource_choices = Some(items.as_slice());
                }
            }
            P::Cell128 => {
                pick = ui.small_button(tr!("从地图选格子")).clicked();
            }
        }
        let choices = resource_choices.unwrap_or(choices.as_slice());
        crate::completion::reference(
            ui,
            ui.make_persistent_id(("parameter", p.index)),
            value,
            choices,
            crate::i18n::text(p.label),
            155.,
        );
        if !choices.is_empty() {
            egui::ComboBox::from_id_salt(p.index)
                .selected_text(tr!("选择…"))
                .width(90.)
                .height(220.)
                .show_ui(ui, |ui| {
                    // Persistent egui field, separate per operand, keeps large CSF
                    // catalogs searchable without attaching state to document data.
                    let id = ui.scope_id().with("filter");
                    let mut filter = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
                    ui.add(
                        egui::TextEdit::singleline(&mut filter)
                            .hint_text(tr!("搜索名称或编号…"))
                            .desired_width(f32::INFINITY),
                    )
                    .accessible_name(tr!("搜索{arg0}", arg0 = crate::i18n::text(p.label)));
                    let lower = filter.to_lowercase();
                    for (token, label) in choices
                        .iter()
                        .filter(|(_, label)| label.to_lowercase().contains(&lower))
                        .take(250)
                    {
                        ui.selectable_value(value, token.clone(), label);
                    }
                    ui.data_mut(|d| d.insert_temp(id, filter));
                });
        }
    });
    if let P::Type(section) = p.kind {
        let valid = scene.catalog.object_types.iter().any(|(key, ids)| {
            (section.is_empty()
                || section == key
                || section == "Foot" && matches!(key.as_str(), "Infantry" | "Units" | "Aircraft"))
                && ids.iter().any(|id| id.eq_ignore_ascii_case(value))
        });
        if !valid {
            ui.colored_label(
                egui::Color32::YELLOW,
                tr!("当前规则目录中没有此类型（原文仍可保存）"),
            );
        }
    }
    if let Some(error) = schema::parameter_error(doc, p.kind, value) {
        let error = if let P::Integer { min, max, unit } = p.kind {
            tr!(
                "须为 {min}～{max} 的整数（{unit}）",
                min = min,
                max = max,
                unit = crate::i18n::text(unit)
            )
        } else {
            crate::i18n::text(&error).to_owned()
        };
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    pick
}

pub(super) fn slots(
    ui: &mut egui::Ui,
    kind: Kind,
    draft: &mut BTreeMap<String, String>,
    doc: &ra2_map_document::document::MapDocument,
    scene: &MapScene,
    pick: &mut Option<Pick>,
    message: &mut String,
) {
    use schema::scripts::{self, Edit};
    let script = kind == Kind::Script;
    let mut keys: Vec<_> = draft
        .keys()
        .filter_map(|k| k.parse::<usize>().ok().map(|i| (i, k.clone())))
        .collect();
    keys.sort();
    let mut edit = None;
    for (index, key) in keys {
        ui.push_id(&key, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("{}", index + 1));
                if ui.small_button(tr!("插入")).clicked() {
                    edit = Some(Edit::Insert(index));
                }
                if ui.small_button(tr!("复制")).clicked() {
                    edit = Some(Edit::Copy(index));
                }
                if ui.small_button("↑").clicked() {
                    edit = Some(Edit::Move(index, index.saturating_sub(1)));
                }
                if ui.small_button("↓").clicked() {
                    edit = Some(Edit::Move(index, index + 1));
                }
                if ui.small_button(tr!("删除")).clicked() {
                    edit = Some(Edit::Remove(index));
                }
            });
            let value = draft.get_mut(&key).unwrap();
            let before = value.clone();
            let mut fields = mission::csv(value);
            if fields.len() == 2 {
                if script {
                    egui::ComboBox::from_id_salt("opcode")
                        .selected_text(
                            scripts::opcode(fields[0].parse().unwrap_or(-1)).map_or_else(
                                || tr!("原始动作 {arg0}", arg0 = fields[0]),
                                |s| format!("{} {}", s.id, crate::i18n::text(s.name)),
                            ),
                        )
                        .show_ui(ui, |ui| {
                            for spec in scripts::opcodes() {
                                if ui
                                    .selectable_label(
                                        fields[0] == spec.id.to_string(),
                                        format!("{} {}", spec.id, crate::i18n::text(spec.name)),
                                    )
                                    .clicked()
                                {
                                    fields = vec![spec.id.to_string(), "0".into()];
                                }
                            }
                        });
                    if let Some(spec) = scripts::opcode(fields[0].parse().unwrap_or(-1)) {
                        ui.small(crate::i18n::text(spec.hint));
                        for p in spec.parameters {
                            if parameter(ui, *p, &mut fields[p.index], doc, scene) {
                                *pick = Some(Pick {
                                    key: key.clone(),
                                    row: None,
                                    index: p.index,
                                    kind: p.kind,
                                });
                            }
                        }
                    }
                } else {
                    let mut count = fields[0].parse::<i32>().unwrap_or(1);
                    ui.horizontal(|ui| {
                        ui.label(tr!("数量"));
                        if crate::form_edit::number(
                            ui,
                            egui::DragValue::new(&mut count).range(1..=i32::MAX),
                            tr!("数量"),
                        )
                        .changed()
                        {
                            fields[0] = count.to_string();
                        }
                    });
                    let p = schema::Parameter {
                        index: 1,
                        label: tr!("成员类型"),
                        kind: P::Type("Foot"),
                    };
                    if parameter(ui, p, &mut fields[1], doc, scene) {
                        *pick = Some(Pick {
                            key: key.clone(),
                            row: None,
                            index: 1,
                            kind: p.kind,
                        });
                    }
                }
                if fields != mission::csv(&before) {
                    *value = fields.join(",");
                }
            }
            egui::CollapsingHeader::new(tr!("原始行")).show(ui, |ui| {
                ui.text_edit_singleline(value)
                    .accessible_name(tr!("原始行"));
            });
            ui.separator();
        });
    }
    let len = draft.keys().filter(|k| k.parse::<usize>().is_ok()).count();
    if ui
        .add_enabled(
            len < if script { 50 } else { 6 },
            egui::Button::new(tr!("＋列表行")),
        )
        .clicked()
    {
        edit = Some(Edit::Insert(len));
    }
    if let Some(edit) = edit
        && let Err(error) = scripts::edit(draft, script, edit)
    {
        *message = format!("{error:#}");
    }
}
