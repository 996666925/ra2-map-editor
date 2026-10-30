use super::*;
use ra2_map_document::ai_triggers;

pub(super) fn form(
    ui: &mut egui::Ui,
    value: &mut String,
    doc: &ra2_map_document::document::MapDocument,
    scene: &MapScene,
) {
    ui.small("出队需先用动作 74 启用对应所属方的电脑触发器；建造需动作 3 或所属方智能等级的生产条件。小队还需满足工厂/招募、上限及基地防御条件。");
    if let Ok(mut f) = ai_triggers::decode(value) {
        let before = f.clone();
        for (i, label) in [
            (0, "名称"),
            (1, "主小队"),
            (14, "次小队"),
            (2, "国家 ID / <all>"),
            (12, "阵营：0 全部，1 盟军，2 苏军，3 尤里"),
        ] {
            ui.push_id(i, |ui| {
                if matches!(i, 1 | 14) {
                    forms::parameter(
                        ui,
                        mission::schema::Parameter {
                            index: i,
                            label,
                            kind: mission::schema::ParameterKind::Reference(Kind::Team),
                        },
                        &mut f[i],
                        doc,
                        scene,
                    );
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(label);
                        ui.text_edit_singleline(&mut f[i]);
                    });
                }
            });
        }
        egui::ComboBox::from_id_salt("ai-condition")
            .selected_text(format!("条件 {}", f[4]))
            .show_ui(ui, |ui| {
                for (id, label) in ai_triggers::CONDITIONS {
                    ui.selectable_value(&mut f[4], id.to_string(), format!("{id} · {label}"));
                }
            });
        // The Type chooser's canvas button is intentionally omitted here; the
        // scalar selection uses the same cached catalog as mission parameters.
        ui.horizontal_wrapped(|ui| {
            ui.label("对象类型（数量条件）");
            ui.text_edit_singleline(&mut f[5]);
            egui::ComboBox::from_id_salt("ai-type")
                .selected_text("选择类型…")
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f[5], "<none>".into(), "无");
                    for (section, ids) in &scene.catalog.object_types {
                        if matches!(
                            section.as_str(),
                            "Structures" | "Units" | "Infantry" | "Aircraft"
                        ) {
                            for id in ids {
                                ui.selectable_value(
                                    &mut f[5],
                                    id.clone(),
                                    scene.catalog.object_label(id),
                                );
                            }
                        }
                    }
                });
        });
        if let Ok((mut amount, mut op)) = ai_triggers::compare_words(&f[6]) {
            let old = (amount, op);
            ui.horizontal_wrapped(|ui| {
                ui.label("比较数量 / 资金");
                ui.add(egui::DragValue::new(&mut amount));
                egui::ComboBox::from_id_salt("ai-comparison")
                    .selected_text(format!("运算 {op}"))
                    .show_ui(ui, |ui| {
                        for (n, label) in ["<", "<=", "==", ">=", ">", "!="].iter().enumerate() {
                            ui.selectable_value(&mut op, n as i32, *label);
                        }
                    });
            });
            if (amount, op) != old
                && let Ok(hex) = ai_triggers::with_comparison(&f[6], amount, op)
            {
                f[6] = hex;
            }
        }
        for (i, label) in [(7, "当前权重"), (8, "最小权重"), (9, "最大权重")] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.text_edit_singleline(&mut f[i]);
            });
        }
        ui.small("权重按原生整数处理；5000 为优先权重。比较参数的剩余 24 字节保持原值。");
        ui.horizontal_wrapped(|ui| {
            for (i, label) in [(10, "可用于多人"), (15, "简单"), (16, "普通"), (17, "困难")]
            {
                let mut flag = f[i].parse::<i32>().is_ok_and(|n| n != 0);
                if ui.checkbox(&mut flag, label).changed() {
                    f[i] = u8::from(flag).to_string();
                }
            }
        });
        ui.small("科技门槛由主、次小队成员的科技等级计算；字段 3 原文不决定门槛。国家使用国家列表中的 ID。字段 11 被引擎丢弃，字段 13 仅存储；原文保留。");
        for (i, error) in ai_triggers::errors(&f) {
            ui.colored_label(egui::Color32::YELLOW, format!("字段 {i}：{error}"));
        }
        if f != before {
            *value = f.join(",");
        }
    } else {
        ui.small("无法解释为已核对的 18 字段格式，请使用原文编辑。");
    }
    ui.collapsing("电脑触发器原文", |ui| {
        ui.text_edit_singleline(value);
    });
}
pub(super) fn enable(ui: &mut egui::Ui, value: &mut String) {
    egui::ComboBox::from_id_salt("ai-enable")
        .selected_text(if value.is_empty() {
            "不写启用项"
        } else {
            match value.as_str() {
                "yes" => "启用",
                "no" => "单人禁用",
                _ => value.as_str(),
            }
        })
        .show_ui(ui, |ui| {
            for (v, label) in [
                ("", "不写启用项（沿用固定定义/默认）"),
                ("yes", "启用"),
                ("no", "单人禁用"),
            ] {
                ui.selectable_value(value, v.into(), label);
            }
        });
    ui.small("新地图的电脑触发器默认禁用，需显式启用；固定定义默认启用。多人模式中只要存在启用字段，即使填 no 也会启用。");
}
