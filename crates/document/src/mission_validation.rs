//! Authoring diagnostics. Unknown extensions remain editable and round-trip unchanged.
use super::{
    ExternalDefinitions, Issue, Kind, cell_at, csv, decode_list, fields, ids, references, same_id,
    schema,
};
use crate::document::MapDocument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Capability,
}
impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "错误",
            Self::Warning => "警告",
            Self::Capability => "能力提示",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub issue: Issue,
    pub severity: Severity,
    /// Physical form field, e.g. Events or Credits. Row details are in message.
    pub field: String,
}

pub fn diagnostics(doc: &MapDocument, external: Option<&ExternalDefinitions>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut add = |kind, id: &str, field: &str, severity, message: String, cell| {
        out.push(Diagnostic {
            issue: Issue {
                kind,
                id: id.into(),
                message,
                cell,
            },
            severity,
            field: field.into(),
        });
    };
    if let Some(definitions) = external
        && let Some(error) = &definitions.error
    {
        add(
            Kind::Basic,
            "Basic",
            "",
            Severity::Warning,
            format!(
                "{} 读取失败，固定 AI 引用尚未核实：{error}",
                definitions.source
            ),
            None,
        );
    }
    for r in references(doc) {
        if ids(doc, r.to.0)
            .iter()
            .any(|id| same_id(r.to.0, id, &r.to.1))
            || external.is_some_and(|e| e.contains(r.to.0, &r.to.1))
            || (r.to.0 == Kind::House && matches!(r.to.1.as_str(), "Neutral" | "Special"))
        {
            continue;
        }
        let fixed = matches!(r.to.0, Kind::Team | Kind::Script | Kind::TaskForce);
        // Event Team names and object Tag display names have additional native
        // alias lookup paths; do not claim a definite missing ID for those.
        let alias = matches!(r.field.as_str(), "Event Team" | "对象 Tag");
        let uncertain = alias
            || (fixed && external.is_none_or(|e| e.source.is_empty() || e.error.is_some()))
            || (r.to.0 == Kind::House && ids(doc, Kind::House).is_empty());
        let field = match (r.from.0, r.field.as_str()) {
            (Kind::Trigger, "Action" | "Local" | "Global") => "Actions",
            (Kind::Trigger, "Event Team" | "Event Local" | "Event Global") => "Events",
            (Kind::Trigger, _) => "Triggers",
            (Kind::Tag, _) => "Tags",
            (Kind::CellTag, _) => "CellTags",
            (Kind::AiTrigger, _) => "AITriggerTypes",
            _ => &r.field,
        };
        add(
            r.from.0,
            &r.from.1,
            field,
            if uncertain {
                Severity::Warning
            } else {
                Severity::Error
            },
            format!(
                "{} → {} {} {}",
                r.field,
                r.to.0.label(),
                r.to.1,
                if uncertain {
                    "未在地图定义，外部/生成定义尚未核实"
                } else {
                    "不存在于当前地图或已加载的固定定义"
                }
            ),
            r.cell,
        );
    }
    let waypoint = |id: u32| {
        doc.records()
            .values()
            .any(|r| r.section == "Waypoints" && r.key == id.to_string())
    };
    for id in ids(doc, Kind::Trigger) {
        let header = csv(&doc.ini_text("Triggers", &id));
        if header.len() < 8 || header[..header.len().min(8)].iter().any(String::is_empty) {
            add(
                Kind::Trigger,
                &id,
                "Triggers",
                Severity::Warning,
                "触发器头不足 8 个非空字段；原生跳过空项并为缺省开关写入默认值，请检查预期难度和启用状态".into(),
                None,
            );
        }
        for (index, label) in [
            (3, "禁用"),
            (4, "简单"),
            (5, "普通"),
            (6, "困难"),
            (7, "占领转移"),
        ] {
            if header
                .get(index)
                .is_some_and(|v| !matches!(v.as_str(), "0" | "1"))
            {
                add(
                    Kind::Trigger,
                    &id,
                    "Triggers",
                    Severity::Warning,
                    format!("{label} 建议使用 0 或 1；当前值由原生整数读法解释"),
                    None,
                );
            }
        }
        for (section, event) in [("Events", true), ("Actions", false)] {
            let rows = match decode_list(&doc.ini_text(section, &id), event) {
                Ok(rows) => rows,
                Err(error) => {
                    add(
                        Kind::Trigger,
                        &id,
                        section,
                        Severity::Warning,
                        format!("{section}: {error}；原文保留，表单无法可靠解释"),
                        None,
                    );
                    continue;
                }
            };
            for (index, row) in rows.iter().enumerate() {
                let number = row[0].parse::<i32>().unwrap_or(-1);
                let Some(spec) = schema::opcode(event, number) else {
                    add(
                        Kind::Trigger,
                        &id,
                        section,
                        Severity::Capability,
                        format!(
                            "第 {} 项 ID {number} 未列入参数清单；保留原始参数",
                            index + 1
                        ),
                        None,
                    );
                    continue;
                };
                if row[1].parse::<i32>().ok() != Some(spec.parameter_type) {
                    add(
                        Kind::Trigger,
                        &id,
                        section,
                        Severity::Warning,
                        format!(
                            "第 {} 项参数类型 {} 与表单 {} 不同；使用原文字段核实",
                            index + 1,
                            row[1],
                            spec.parameter_type
                        ),
                        None,
                    );
                    continue;
                }
                for p in spec.parameters {
                    if let Some(error) = row
                        .get(p.index)
                        .and_then(|v| schema::parameter_error(doc, p.kind, v))
                    {
                        add(
                            Kind::Trigger,
                            &id,
                            section,
                            Severity::Error,
                            format!("第 {} 项 {}：{error}", index + 1, p.label),
                            None,
                        );
                    }
                }
            }
        }
    }
    for kind in [Kind::TaskForce, Kind::Script] {
        for id in ids(doc, kind) {
            for (key, value) in fields(doc, kind, &id) {
                if let Ok(index) = key.parse::<usize>() {
                    let value = String::from_utf8_lossy(&value);
                    let parts = csv(&value);
                    let valid = index < if kind == Kind::TaskForce { 6 } else { 50 }
                        && parts.len() == 2
                        && parts[0].parse::<i32>().is_ok()
                        && if kind == Kind::TaskForce {
                            !parts[1].is_empty() && parts[0].parse::<i32>().is_ok_and(|n| n > 0)
                        } else {
                            parts[1].parse::<i32>().is_ok()
                        };
                    if valid && kind == Kind::Script {
                        if let Some(spec) = schema::scripts::opcode(parts[0].parse().unwrap()) {
                            for p in spec.parameters {
                                let error = if spec.id == 6 {
                                    parts[1]
                                        .parse::<usize>()
                                        .ok()
                                        .filter(|n| {
                                            *n > 0
                                                && doc
                                                    .ini_value(&id, &(n - 1).to_string())
                                                    .is_some()
                                        })
                                        .is_none()
                                        .then(|| "跳转目标行不存在（从 1 开始）".to_owned())
                                } else {
                                    schema::parameter_error(doc, p.kind, &parts[p.index])
                                };
                                if let Some(error) = error {
                                    add(
                                        kind,
                                        &id,
                                        &key,
                                        Severity::Error,
                                        format!("{}：{error}", p.label),
                                        None,
                                    );
                                }
                            }
                        } else {
                            add(
                                kind,
                                &id,
                                &key,
                                Severity::Capability,
                                format!("脚本动作 {} 未列入已核对参数清单；原文保留", parts[0]),
                                None,
                            );
                        }
                    }
                    if !valid {
                        add(
                            kind,
                            &id,
                            &key,
                            Severity::Error,
                            "超过原生列表容量或参数格式无效".into(),
                            None,
                        );
                    }
                }
            }
        }
    }
    for id in ids(doc, Kind::CellTag) {
        if cell_at(doc, &id).is_none() {
            add(
                Kind::CellTag,
                &id,
                "CellTags",
                Severity::Error,
                "格子坐标无效或超出 Size".into(),
                None,
            );
        }
    }
    for id in ids(doc, Kind::Tag) {
        let f = csv(&doc.ini_text("Tags", &id));
        if f.len() < 3 || f[2].is_empty() || f[0].parse::<u8>().ok().is_none_or(|n| n > 2) {
            add(
                Kind::Tag,
                &id,
                "Tags",
                Severity::Warning,
                "Tag 建议使用 重复方式(0～2),名称,Trigger ID；缺省或扩展值需核实运行语义".into(),
                None,
            );
        }
    }
    for id in ids(doc, Kind::Variable) {
        let f = csv(&doc.ini_text("VariableNames", &id));
        if id.parse::<u8>().ok().is_none_or(|n| n >= 100) {
            add(
                Kind::Variable,
                &id,
                "VariableNames",
                Severity::Error,
                "局部变量 ID 须为 0～99".into(),
                None,
            );
        }
        if f.get(1)
            .is_some_and(|value| !matches!(value.as_str(), "0" | "1"))
        {
            add(
                Kind::Variable,
                &id,
                "VariableNames",
                Severity::Warning,
                "初值建议使用 0 或 1；原生以 atoi != 0 解释，省略则保留之前状态".into(),
                None,
            );
        }
    }
    for kind in [Kind::Basic, Kind::House, Kind::Team] {
        for id in ids(doc, kind) {
            for (key, raw) in fields(doc, kind, &id) {
                if raw.is_empty() {
                    continue;
                }
                let text = String::from_utf8_lossy(&raw);
                let boolean = match kind {
                    Kind::Basic => matches!(
                        key.as_str(),
                        "MultiplayerOnly" | "EndOfGame" | "OneTimeOnly" | "SkipScore"
                    ),
                    Kind::House => key == "PlayerControl",
                    Kind::Team => matches!(
                        key.as_str(),
                        "Autocreate"
                            | "Full"
                            | "Recruiter"
                            | "Prebuild"
                            | "Reinforce"
                            | "Suicide"
                            | "Aggressive"
                            | "LooseRecruit"
                            | "AreTeamMembersRecruitable"
                            | "OnTransOnly"
                            | "AvoidThreats"
                            | "UseTransportOrigin"
                            | "Whiner"
                            | "Droppod"
                            | "Annoyance"
                            | "IonImmune"
                            | "IsBaseDefense"
                            | "OnlyTargetHouseEnemy"
                            | "TransportsReturnOnUnload"
                    ),
                    _ => false,
                };
                // CCINI ReadBool uses the first byte, not a closed list of words.
                if boolean
                    && !matches!(
                        text.bytes().next().map(|b| b.to_ascii_uppercase()),
                        Some(b'0' | b'1' | b'F' | b'N' | b'T' | b'Y')
                    )
                {
                    add(
                        kind,
                        &id,
                        &key,
                        Severity::Warning,
                        "布尔值无法被 ReadBool 识别，将使用引擎默认值".into(),
                        None,
                    );
                }
                let integer = match kind {
                    Kind::House => matches!(key.as_str(), "IQ" | "Credits" | "TechLevel"),
                    Kind::Team => matches!(
                        key.as_str(),
                        "Max" | "Priority" | "VeteranLevel" | "TechLevel" | "Group"
                    ),
                    _ => false,
                };
                if integer && text.parse::<i32>().is_err() {
                    add(
                        kind,
                        &id,
                        &key,
                        Severity::Warning,
                        "不是完整的 32 位整数；引擎会按原生读法回退或截取".into(),
                        None,
                    );
                }
                if kind == Kind::House
                    && key == "Credits"
                    && text
                        .parse::<i32>()
                        .is_ok_and(|n| n.checked_mul(100).is_none())
                {
                    add(
                        kind,
                        &id,
                        &key,
                        Severity::Warning,
                        "Credits × 100 超过 32 位资金范围".into(),
                        None,
                    );
                }
                if kind == Kind::Team
                    && key == "Waypoint"
                    && text != "-1"
                    && schema::waypoint_index(&text, true).is_none_or(|n| !waypoint(n))
                {
                    add(
                        kind,
                        &id,
                        &key,
                        Severity::Error,
                        format!("Waypoint {text} 不存在"),
                        None,
                    );
                }
            }
        }
    }
    for id in ids(doc, Kind::AiTrigger) {
        let raw = doc.ini_text("AITriggerTypes", &id);
        if raw.is_empty() {
            if !external.is_some_and(|e| e.contains(Kind::AiTrigger, &id)) {
                add(
                    Kind::AiTrigger,
                    &id,
                    "AITriggerTypesEnable",
                    Severity::Warning,
                    "启用项未找到对应 AITriggerTypes；固定 AI 定义需在当前安装中核实".into(),
                    None,
                );
            }
        } else {
            match crate::ai_triggers::decode(&raw) {
                Err(e) => add(
                    Kind::AiTrigger,
                    &id,
                    "AITriggerTypes",
                    Severity::Warning,
                    format!("{e:#}；原文保留"),
                    None,
                ),
                Ok(f) => {
                    if let Some(e) = external.filter(|e| e.error.is_none() && !e.source.is_empty())
                    {
                        for (i, known, registry) in [
                            (2, &e.countries, "Countries"),
                            (5, &e.object_types, "ObjectTypes"),
                        ] {
                            let token = &f[i];
                            let local = if registry == "Countries" {
                                doc.ini_entries(registry)
                                    .iter()
                                    .any(|(_, _, v)| v.eq_ignore_ascii_case(token.as_bytes()))
                            } else {
                                [
                                    "BuildingTypes",
                                    "VehicleTypes",
                                    "InfantryTypes",
                                    "AircraftTypes",
                                ]
                                .iter()
                                .any(|s| {
                                    doc.ini_entries(s)
                                        .iter()
                                        .any(|(_, _, v)| v.eq_ignore_ascii_case(token.as_bytes()))
                                })
                            };
                            if !matches!(token.as_str(), "<none>" | "<all>")
                                && !known.iter().any(|id| id.eq_ignore_ascii_case(token))
                                && !local
                            {
                                add(
                                    Kind::AiTrigger,
                                    &id,
                                    "AITriggerTypes",
                                    Severity::Error,
                                    format!("字段 {i}：{token} 未在当前 {registry} 注册"),
                                    None,
                                );
                            }
                        }
                    }
                    for (i, error) in crate::ai_triggers::errors(&f) {
                        add(
                            Kind::AiTrigger,
                            &id,
                            "AITriggerTypes",
                            Severity::Warning,
                            format!("字段 {i}：{error}"),
                            None,
                        );
                    }
                    if f[1].eq_ignore_ascii_case("<none>") {
                        add(
                            Kind::AiTrigger,
                            &id,
                            "AITriggerTypes",
                            Severity::Error,
                            "主小队未指定，AI 不会出队".into(),
                            None,
                        );
                    }
                    if f[4].parse::<i32>().is_ok_and(|n| !(-1..=7).contains(&n)) {
                        add(
                            Kind::AiTrigger,
                            &id,
                            "AITriggerTypes",
                            Severity::Capability,
                            "条件未列入已核对引擎范围，保留原文".into(),
                            None,
                        );
                    }
                }
            }
        }
        let enabled = doc.ini_text("AITriggerTypesEnable", &id);
        if !enabled.is_empty()
            && !["yes", "no", "true", "false", "1", "0", "on", "off"]
                .iter()
                .any(|v| enabled.eq_ignore_ascii_case(v))
        {
            add(
                Kind::AiTrigger,
                &id,
                "AITriggerTypesEnable",
                Severity::Warning,
                "无法识别的启用值将回退为 false；多人模式存在该 key 即启用".into(),
                None,
            );
        }
    }
    for house in ids(doc, Kind::House) {
        match crate::base_nodes::read(doc, &house) {
            Err(error) => add(
                Kind::House,
                &house,
                "NodeCount",
                Severity::Warning,
                format!("基地计划无法安全编辑：{error:#}；原文保留"),
                None,
            ),
            Ok(nodes) => {
                for (i, node) in nodes.iter().enumerate() {
                    if !node.control()
                        && let Some(e) =
                            external.filter(|e| e.error.is_none() && !e.source.is_empty())
                        && !e
                            .buildings
                            .iter()
                            .any(|id| id.eq_ignore_ascii_case(&node.building))
                        && !doc
                            .ini_entries("BuildingTypes")
                            .iter()
                            .any(|(_, _, v)| v.eq_ignore_ascii_case(node.building.as_bytes()))
                    {
                        add(
                            Kind::House,
                            &house,
                            &format!("{i:03}"),
                            Severity::Error,
                            format!(
                                "基地节点建筑类型 {} 未在当前 BuildingTypes 注册",
                                node.building
                            ),
                            Some(node.cell),
                        );
                    }
                    if node.cell != [0, 0] && doc.terrain(node.cell).is_none() {
                        add(
                            Kind::House,
                            &house,
                            &format!("{i:03}"),
                            Severity::Error,
                            "基地节点位于地图格网之外".into(),
                            Some(node.cell),
                        );
                    }
                    if node.control() && !matches!(node.building.as_str(), "-1" | "-2" | "-3") {
                        add(
                            Kind::House,
                            &house,
                            &format!("{i:03}"),
                            Severity::Capability,
                            format!("控制节点 {} 不在已核对的 -1/-2/-3 范围", node.building),
                            Some(node.cell),
                        );
                    }
                }
                for (_, key, _) in doc.ini_entries(&house) {
                    if let Ok(n) = key.parse::<usize>()
                        && key == format!("{n:03}")
                        && n >= nodes.len()
                    {
                        add(
                            Kind::House,
                            &house,
                            &key,
                            Severity::Warning,
                            "孤立节点未计入 NodeCount，引擎不会读取；扩展计划前请先消歧".into(),
                            None,
                        );
                    }
                }
            }
        }
    }
    for (key, value) in fields(doc, Kind::Basic, "Basic") {
        let value = String::from_utf8_lossy(&value);
        if value.is_empty() {
            continue;
        }
        let error = match key.as_str() {
            "CarryOverCap" if value.parse::<i32>().is_err() => {
                Some("须为 32 位整数；-1 表示无上限")
            }
            "CarryOverMoney"
                if value
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .ok()
                    .is_none_or(|n| {
                        !n.is_finite() || n < 0. || n > if value.ends_with('%') { 100. } else { 1. }
                    }) =>
            {
                Some("建议为 0～100%（或 0～1 小数）；接收关决定继承比例")
            }
            "NextScenario" | "AltNextScenario" if value.len() > 259 => {
                Some("目标文件名超过原生 259 字节容量")
            }
            _ => None,
        };
        if let Some(error) = error {
            add(
                Kind::Basic,
                "Basic",
                &key,
                Severity::Warning,
                error.into(),
                None,
            );
        }
    }
    if doc
        .ini_text("Basic", "SkipMapSelect")
        .eq_ignore_ascii_case("yes")
        && !doc
            .ini_text("Basic", "EndOfGame")
            .eq_ignore_ascii_case("yes")
        && !doc
            .ini_text("Basic", "OneTimeOnly")
            .eq_ignore_ascii_case("yes")
    {
        for key in ["NextScenario", "AltNextScenario"] {
            if doc.ini_text("Basic", key).is_empty() {
                add(
                    Kind::Basic,
                    "Basic",
                    key,
                    Severity::Warning,
                    "直接续关目标为空；MAPSELMD 初始化可能失败并重播当前关卡".into(),
                    None,
                );
            }
        }
    }
    out
}
