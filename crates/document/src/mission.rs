//! Mission authoring over physical INI transactions. Runtime readers remain in core.
use crate::document::{Cell, MapDocument};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    #[default]
    Basic,
    House,
    Trigger,
    Tag,
    CellTag,
    TaskForce,
    Script,
    Team,
    Variable,
    AiTrigger,
    Global,
}
impl Kind {
    pub const ALL: [Self; 11] = [
        Self::Basic,
        Self::House,
        Self::Trigger,
        Self::Tag,
        Self::CellTag,
        Self::TaskForce,
        Self::Script,
        Self::Team,
        Self::Variable,
        Self::AiTrigger,
        Self::Global,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "任务设置",
            Self::House => "所属方",
            Self::Trigger => "触发器",
            Self::Tag => "触发标签",
            Self::CellTag => "格子标签",
            Self::TaskForce => "特遣部队",
            Self::Script => "脚本",
            Self::Team => "小队",
            Self::Variable => "局部变量",
            Self::AiTrigger => "AI 触发器",
            Self::Global => "全局变量",
        }
    }
    pub fn registry(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::House => "Houses",
            Self::Trigger => "Triggers",
            Self::Tag => "Tags",
            Self::CellTag => "CellTags",
            Self::TaskForce => "TaskForces",
            Self::Script => "ScriptTypes",
            Self::Team => "TeamTypes",
            Self::Variable => "VariableNames",
            Self::AiTrigger => "AITriggerTypes",
            Self::Global => "EditorGlobalNames",
        }
    }
    pub fn section_definition(self) -> bool {
        matches!(
            self,
            Self::House | Self::TaskForce | Self::Script | Self::Team
        )
    }
}
pub fn ids(doc: &MapDocument, kind: Kind) -> Vec<String> {
    if kind == Kind::Global {
        return (0..crate::campaign::GLOBAL_COUNT)
            .map(|i| i.to_string())
            .collect();
    }
    if kind == Kind::Basic {
        return vec!["Basic".into()];
    }
    let mut seen = BTreeSet::new();
    let mut entries = doc.ini_entries(kind.registry());
    if kind == Kind::AiTrigger {
        entries.extend(doc.ini_entries("AITriggerTypesEnable"));
    }
    entries
        .into_iter()
        .filter_map(|(_, key, value)| {
            let id = if kind.section_definition() {
                String::from_utf8_lossy(&value).trim().to_owned()
            } else {
                key
            };
            (!id.is_empty() && seen.insert(id.clone())).then_some(id)
        })
        .collect()
}
pub fn fields(doc: &MapDocument, kind: Kind, id: &str) -> Vec<(String, Vec<u8>)> {
    if kind.section_definition() {
        return doc.ini_fields(id).into_iter().collect();
    }
    if kind == Kind::Basic {
        return [
            "Name",
            "Author",
            "Player",
            "MultiplayerOnly",
            "HomeCell",
            "AltHomeCell",
            "Intro",
            "Brief",
            "Action",
            "Theme",
            "Win",
            "Lose",
            "NextScenario",
            "AltNextScenario",
            "SkipMapSelect",
            "TimerInherit",
            "CarryOverMoney",
            "CarryOverCap",
            "PostScore",
            "PreMapSelect",
            "EndOfGame",
            "OneTimeOnly",
            "SkipScore",
        ]
        .into_iter()
        .map(|key| (key.into(), doc.ini_value("Basic", key).unwrap_or_default()))
        .collect();
    }
    if matches!(kind, Kind::Trigger | Kind::AiTrigger) {
        let sections = if kind == Kind::Trigger {
            vec!["Triggers", "Events", "Actions"]
        } else {
            vec!["AITriggerTypes", "AITriggerTypesEnable"]
        };
        return sections
            .into_iter()
            .map(|s| (s.into(), doc.ini_value(s, id).unwrap_or_default()))
            .collect();
    }
    vec![(
        kind.registry().into(),
        doc.ini_value(kind.registry(), id).unwrap_or_default(),
    )]
}
pub fn write_fields(
    doc: &mut MapDocument,
    kind: Kind,
    id: &str,
    changes: &[(String, Vec<u8>)],
) -> Result<()> {
    if kind == Kind::Global {
        ensure!(
            id.parse::<usize>()
                .is_ok_and(|n| n < crate::campaign::GLOBAL_COUNT),
            "全局变量须为 0～49"
        );
    }
    doc.atomic(|doc| {
        for (key, value) in changes {
            let (s, k) = if kind.section_definition() || kind == Kind::Basic {
                (id, key.as_str())
            } else {
                (key.as_str(), id)
            };
            doc.set_ini(
                s,
                k,
                if value.is_empty() {
                    None
                } else {
                    Some(value.clone())
                },
            )?;
        }
        Ok(())
    })
}
pub fn identifier(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 23
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "ID 须为 1～23 位 ASCII 字母、数字或下划线"
    );
    ensure!(
        ![
            "Map",
            "Basic",
            "Houses",
            "Triggers",
            "Events",
            "Actions",
            "Tags",
            "CellTags",
            "TaskForces",
            "ScriptTypes",
            "TeamTypes",
            "VariableNames",
            "AITriggerTypes",
            "AITriggerTypesEnable",
            "Units",
            "Infantry",
            "Structures",
            "Aircraft",
            "Terrain",
            "Smudge",
            "Waypoints",
            "Lighting",
            "Colors",
            "Countries",
            "IsoMapPack5",
            "OverlayPack",
            "OverlayDataPack",
            "Preview",
            "PreviewPack",
            "Digest"
        ]
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(id)),
        "ID 与保留 section 冲突"
    );
    Ok(())
}
fn put(doc: &mut MapDocument, section: &str, key: &str, value: impl AsRef<str>) -> Result<()> {
    doc.set_ini(section, key, Some(value.as_ref().as_bytes().to_vec()))
}
fn register(doc: &mut MapDocument, kind: Kind, id: &str) -> Result<()> {
    let keys: BTreeSet<_> = doc
        .ini_entries(kind.registry())
        .into_iter()
        .map(|(_, k, _)| k)
        .collect();
    let n = (0u32..)
        .find(|n| !keys.contains(&n.to_string()))
        .context("注册表已满")?;
    put(doc, kind.registry(), &n.to_string(), id)
}
pub fn next_id(doc: &MapDocument, kind: Kind) -> String {
    if kind == Kind::Variable {
        return (0..100)
            .find(|n| doc.ini_value("VariableNames", &n.to_string()).is_none())
            .unwrap_or(0)
            .to_string();
    }
    let prefix = match kind {
        Kind::House => "EDHOUSE",
        Kind::Trigger => "EDTR",
        Kind::Tag => "EDTAG",
        Kind::TaskForce => "EDTF",
        Kind::Script => "EDSC",
        Kind::Team => "EDTM",
        Kind::AiTrigger => "EDAI",
        _ => "ED",
    };
    (1..)
        .map(|n| format!("{prefix}{n:04}"))
        .find(|id| {
            Kind::ALL.into_iter().all(|k| !ids(doc, k).contains(id))
                && doc.ini_entries(id).is_empty()
        })
        .unwrap()
}
pub fn create(
    doc: &mut MapDocument,
    kind: Kind,
    id: &str,
    owner: &str,
    cell: Option<Cell>,
) -> Result<()> {
    let cell_id = if kind == Kind::CellTag {
        Some(cell_key(doc, cell.context("先在地图选择格子")?)?)
    } else {
        None
    };
    let id = cell_id.as_deref().unwrap_or(id);
    identifier(id)?;
    ensure!(
        !ids(doc, kind).iter().any(|old| same_id(kind, old, id)),
        "ID 已存在"
    );
    doc.atomic(|doc| {
        if kind.section_definition() {
            ensure!(doc.ini_entries(id).is_empty(), "section 已存在，不能覆盖");
            register(doc, kind, id)?;
            put(doc, id, "Name", id)?;
        }
        match kind {
            Kind::Basic => anyhow::bail!("任务设置无需新建"),
            Kind::House => {
                for (key, value) in [
                    ("Country", "Americans"),
                    ("Color", "Gold"),
                    ("PlayerControl", "no"),
                    ("IQ", "0"),
                    ("Credits", "100"),
                    ("TechLevel", "10"),
                    ("Allies", id),
                ] {
                    put(doc, id, key, value)?;
                }
            }
            Kind::Trigger => {
                put(
                    doc,
                    "Triggers",
                    id,
                    format!("{owner},<none>,{id},0,1,1,1,0"),
                )?;
                put(doc, "Events", id, "1,13,0,30")?;
                put(doc, "Actions", id, "1,67,0,0,0,0,0,0,A")?;
                let tag = next_id(doc, Kind::Tag);
                put(doc, "Tags", &tag, format!("0,{id},{id}"))?;
            }
            Kind::Tag => {
                let trigger = ids(doc, Kind::Trigger)
                    .first()
                    .cloned()
                    .context("先创建触发器")?;
                put(doc, "Tags", id, format!("0,{id},{trigger}"))?;
            }
            Kind::CellTag => {
                let at = cell.context("先在地图选择格子")?;
                let tag = ids(doc, Kind::Tag).first().cloned().context("先创建 Tag")?;
                bind_cell(doc, at, &tag)?;
            }
            Kind::TaskForce => {
                put(doc, id, "Group", "-1")?;
                put(doc, id, "0", "2,E1")?;
            }
            Kind::Script => {
                put(doc, id, "0", "3,0")?;
            }
            Kind::Team => {
                let script = ids(doc, Kind::Script)
                    .first()
                    .cloned()
                    .context("先创建脚本")?;
                let task = ids(doc, Kind::TaskForce)
                    .first()
                    .cloned()
                    .context("先创建特遣部队")?;
                for (k, v) in [
                    ("House", owner.to_owned()),
                    ("Script", script),
                    ("TaskForce", task),
                    ("Waypoint", "A".into()),
                    ("Max", "1".into()),
                    ("Priority", "5".into()),
                    ("VeteranLevel", "1".into()),
                    ("TechLevel", "0".into()),
                    ("Group", "-1".into()),
                    ("Autocreate", "no".into()),
                    ("Full", "yes".into()),
                    ("Recruiter", "no".into()),
                    ("Prebuild", "no".into()),
                    ("Reinforce", "yes".into()),
                    ("Suicide", "no".into()),
                ] {
                    put(doc, id, k, v)?;
                }
            }
            Kind::Variable => {
                let n: u32 = id.parse().context("变量 ID 须为 0～99")?;
                ensure!(n < 100, "变量 ID 须为 0～99");
                put(doc, "VariableNames", id, format!("Flag{n},0"))?;
            }
            Kind::AiTrigger => {
                let team = ids(doc, Kind::Team)
                    .first()
                    .cloned()
                    .context("先创建小队")?;
                put(
                    doc,
                    "AITriggerTypes",
                    id,
                    crate::ai_triggers::default_row(id, &team),
                )?;
                put(doc, "AITriggerTypesEnable", id, "yes")?;
            }
            Kind::Global => anyhow::bail!(
                "全局变量固定为 0～49，无需创建；只能编辑注释，初值由场景/跨关状态决定"
            ),
        }
        Ok(())
    })
}
pub(crate) fn cell_stride(doc: &MapDocument) -> u32 {
    if doc
        .ini_text("Basic", "NewINIFormat")
        .parse::<u32>()
        .unwrap_or(0)
        < 4
    {
        128
    } else {
        1000
    }
}
pub fn cell_key(doc: &MapDocument, at: Cell) -> Result<String> {
    ensure!(
        u32::from(at[0]) < cell_stride(doc) && doc.terrain(at).is_some(),
        "格子超出地图或旧格式坐标范围"
    );
    Ok((u32::from(at[1]) * cell_stride(doc) + u32::from(at[0])).to_string())
}
pub fn cell_at(doc: &MapDocument, key: &str) -> Option<Cell> {
    let n: u32 = key.parse().ok()?;
    let at = [
        u16::try_from(n % cell_stride(doc)).ok()?,
        u16::try_from(n / cell_stride(doc)).ok()?,
    ];
    doc.terrain(at).map(|_| at)
}
pub fn bind_cell(doc: &mut MapDocument, at: Cell, tag: &str) -> Result<()> {
    ensure!(ids(doc, Kind::Tag).iter().any(|id| id == tag), "Tag 不存在");
    let key = cell_key(doc, at)?;
    put(doc, "CellTags", &key, tag)
}

/// Immutable installation definitions, supplied by the scene layer. Map records win.
#[derive(Clone, Debug, Default)]
pub struct ExternalDefinitions {
    pub teams: BTreeSet<String>,
    pub scripts: BTreeSet<String>,
    pub task_forces: BTreeSet<String>,
    pub ai_triggers: BTreeSet<String>,
    pub ai_trigger_rows: std::collections::BTreeMap<String, String>,
    pub buildings: BTreeSet<String>,
    pub object_types: BTreeSet<String>,
    pub countries: BTreeSet<String>,
    pub source: String,
    pub error: Option<String>,
}
impl ExternalDefinitions {
    pub fn contains(&self, kind: Kind, id: &str) -> bool {
        match kind {
            Kind::Team => self.teams.iter().any(|key| same_id(kind, key, id)),
            Kind::Script => self.scripts.iter().any(|key| same_id(kind, key, id)),
            Kind::TaskForce => self.task_forces.iter().any(|key| same_id(kind, key, id)),
            Kind::AiTrigger => self.ai_triggers.iter().any(|key| same_id(kind, key, id)),
            _ => false,
        }
    }
    pub fn candidates(&self, doc: &MapDocument, kind: Kind) -> Vec<(String, bool)> {
        let local: BTreeSet<_> = ids(doc, kind).into_iter().collect();
        let mut choices: BTreeSet<_> = local.clone();
        let fixed = match kind {
            Kind::Team => Some(&self.teams),
            Kind::Script => Some(&self.scripts),
            Kind::TaskForce => Some(&self.task_forces),
            Kind::AiTrigger => Some(&self.ai_triggers),
            _ => None,
        };
        if let Some(fixed) = fixed {
            choices.extend(
                fixed
                    .iter()
                    .filter(|id| !local.iter().any(|key| same_id(kind, key, id)))
                    .cloned(),
            );
        }
        choices
            .into_iter()
            .map(|id| {
                let external = !local.contains(&id);
                (id, external)
            })
            .collect()
    }
}

/// Native definition factories ignore ASCII case; House roster/physical INI
/// lookups keep their own exact-name semantics.
pub fn same_id(kind: Kind, a: &str, b: &str) -> bool {
    if matches!(
        kind,
        Kind::Trigger | Kind::Tag | Kind::Team | Kind::Script | Kind::TaskForce | Kind::AiTrigger
    ) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub from: (Kind, String),
    pub field: String,
    pub to: (Kind, String),
    pub cell: Option<Cell>,
}
pub fn references(doc: &MapDocument) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut add = |from: (Kind, String), field: &str, to: (Kind, String), cell| {
        if !to.1.is_empty() && !matches!(to.1.as_str(), "<none>" | "None" | "none" | "-1") {
            out.push(Reference {
                from,
                field: field.into(),
                to,
                cell,
            });
        }
    };
    add(
        (Kind::Basic, "Basic".into()),
        "Player",
        (Kind::House, doc.ini_text("Basic", "Player")),
        None,
    );
    for id in ids(doc, Kind::House) {
        for ally in doc.ini_text(&id, "Allies").split(',') {
            add(
                (Kind::House, id.clone()),
                "Allies",
                (Kind::House, ally.trim().into()),
                None,
            );
        }
    }
    for id in ids(doc, Kind::Trigger) {
        let f = csv(&doc.ini_text("Triggers", &id));
        add(
            (Kind::Trigger, id.clone()),
            "House",
            (Kind::House, f.first().cloned().unwrap_or_default()),
            None,
        );
        add(
            (Kind::Trigger, id.clone()),
            "Linked",
            (Kind::Trigger, f.get(1).cloned().unwrap_or_default()),
            None,
        );
        if let Ok(rows) = decode_list(&doc.ini_text("Actions", &id), false) {
            for row in rows {
                let target = match row.get(1).map(String::as_str) {
                    Some("1" | "5") => Some(Kind::Team),
                    Some("2") => Some(Kind::Trigger),
                    Some("3") => Some(Kind::Tag),
                    _ => None,
                };
                if let Some(kind) = target {
                    let value = row[2].clone();
                    if value.len() >= 3 {
                        add((Kind::Trigger, id.clone()), "Action", (kind, value), None);
                    }
                }
                if matches!(row[0].as_str(), "56" | "57") {
                    add(
                        (Kind::Trigger, id.clone()),
                        "Local",
                        (Kind::Variable, row[2].clone()),
                        None,
                    );
                }
                if matches!(row[0].as_str(), "28" | "29") {
                    add(
                        (Kind::Trigger, id.clone()),
                        "Global",
                        (Kind::Global, row[2].clone()),
                        None,
                    );
                }
            }
        }
        if let Ok(rows) = decode_list(&doc.ini_text("Events", &id), true) {
            for row in rows {
                if row[1] == "1" && row[2].len() >= 3 {
                    add(
                        (Kind::Trigger, id.clone()),
                        "Event Team",
                        (Kind::Team, row[2].clone()),
                        None,
                    );
                }
                if matches!(row[0].as_str(), "36" | "37") {
                    add(
                        (Kind::Trigger, id.clone()),
                        "Event Local",
                        (Kind::Variable, row[2].clone()),
                        None,
                    );
                }
                if matches!(row[0].as_str(), "27" | "28") {
                    add(
                        (Kind::Trigger, id.clone()),
                        "Event Global",
                        (Kind::Global, row[2].clone()),
                        None,
                    );
                }
            }
        }
    }
    for id in ids(doc, Kind::Script) {
        for (_, value) in fields(doc, Kind::Script, &id)
            .into_iter()
            .filter(|(key, _)| key.parse::<usize>().is_ok())
        {
            let f = csv(&String::from_utf8_lossy(&value));
            if f.len() == 2 && matches!(f[0].as_str(), "12" | "38") {
                add(
                    (Kind::Script, id.clone()),
                    "Script Global",
                    (Kind::Global, f[1].clone()),
                    None,
                );
            }
        }
    }
    for id in ids(doc, Kind::Tag) {
        let f = csv(&doc.ini_text("Tags", &id));
        add(
            (Kind::Tag, id),
            "Trigger",
            (Kind::Trigger, f.get(2).cloned().unwrap_or_default()),
            None,
        );
    }
    for id in ids(doc, Kind::CellTag) {
        add(
            (Kind::CellTag, id.clone()),
            "Tag",
            (Kind::Tag, doc.ini_text("CellTags", &id)),
            cell_at(doc, &id),
        );
    }
    for id in ids(doc, Kind::Team) {
        for (k, kind) in [
            ("House", Kind::House),
            ("Script", Kind::Script),
            ("TaskForce", Kind::TaskForce),
            ("Tag", Kind::Tag),
        ] {
            add(
                (Kind::Team, id.clone()),
                k,
                (kind, doc.ini_text(&id, k)),
                None,
            );
        }
    }
    for id in ids(doc, Kind::AiTrigger) {
        if let Ok(f) = crate::ai_triggers::decode(&doc.ini_text("AITriggerTypes", &id)) {
            for (index, label) in [(1, "主小队"), (14, "次小队")] {
                add(
                    (Kind::AiTrigger, id.clone()),
                    label,
                    (Kind::Team, f[index].clone()),
                    None,
                );
            }
        }
    }
    for (&id, r) in &doc.records() {
        let f = r.fields();
        let col = match r.section.as_str() {
            "Structures" => Some(6),
            "Infantry" => Some(8),
            "Units" | "Aircraft" => Some(7),
            _ => None,
        };
        if let Some(col) = col {
            add(
                (Kind::CellTag, format!("@{id}:{}:{}", r.section, r.key)),
                "对象 House",
                (Kind::House, f.first().cloned().unwrap_or_default()),
                r.cell(),
            );
            add(
                (Kind::CellTag, format!("@{id}:{}:{}", r.section, r.key)),
                "对象 Tag",
                (Kind::Tag, f.get(col).cloned().unwrap_or_default()),
                r.cell(),
            );
        }
    }
    out
}
pub fn delete(doc: &mut MapDocument, kind: Kind, id: &str) -> Result<()> {
    ensure!(
        kind != Kind::Global,
        "全局变量编号固定，清空注释不会删除运行时变量"
    );
    let incoming: Vec<_> = references(doc)
        .into_iter()
        .filter(|r| {
            r.to.0 == kind
                && same_id(kind, &r.to.1, id)
                && !(r.from.0 == kind && same_id(kind, &r.from.1, id))
        })
        .collect();
    ensure!(
        incoming.is_empty(),
        "仍被 {} 项引用，请先迁移引用：{}",
        incoming.len(),
        incoming
            .iter()
            .map(|r| format!("{} {} / {}", r.from.0.label(), r.from.1, r.field))
            .collect::<Vec<_>>()
            .join("；")
    );
    doc.atomic(|doc| {
        if kind.section_definition() {
            for (target, _, _) in doc
                .ini_entries(kind.registry())
                .into_iter()
                .filter(|(_, _, v)| v == id.as_bytes())
            {
                doc.set(target, None)?;
            }
            for (target, _, _) in doc.ini_entries(id) {
                doc.set(target, None)?;
            }
        } else {
            ensure!(kind != Kind::Basic, "不能删除任务设置");
            for section in if kind == Kind::Trigger {
                vec!["Triggers", "Events", "Actions"]
            } else if kind == Kind::AiTrigger {
                vec!["AITriggerTypes", "AITriggerTypesEnable"]
            } else {
                vec![kind.registry()]
            } {
                doc.set_ini(section, id, None)?;
            }
        }
        Ok(())
    })
}
pub fn csv(text: &str) -> Vec<String> {
    text.split(',').map(|s| s.trim().to_owned()).collect()
}
pub fn decode_list(text: &str, event: bool) -> Result<Vec<Vec<String>>> {
    let f = csv(text);
    ensure!(
        f.iter().all(|field| !field.is_empty()),
        "空字段会被原生解析器跳过；请在原文中修正后再使用表单"
    );
    let count: usize = f
        .first()
        .context("缺少数量")?
        .parse()
        .context("列表数量无效")?;
    ensure!(count <= 40, "事件/动作数量过多");
    let mut cursor = 1;
    let mut rows = Vec::new();
    for _ in 0..count {
        let n = if event {
            if f.get(cursor + 1).is_some_and(|n| n == "2") {
                4
            } else {
                3
            }
        } else {
            8
        };
        ensure!(cursor + n <= f.len(), "事件/动作记录截断");
        ensure!(
            f[cursor].parse::<i32>().is_ok() && f[cursor + 1].parse::<i32>().is_ok(),
            "事件/动作 ID 或参数类型不是整数"
        );
        rows.push(f[cursor..cursor + n].to_vec());
        cursor += n;
    }
    ensure!(
        cursor == f.len(),
        "存在未识别尾字段，使用原文编辑以保留它们"
    );
    Ok(rows)
}
pub fn encode_list(rows: &[Vec<String>]) -> String {
    std::iter::once(rows.len().to_string())
        .chain(rows.iter().flatten().cloned())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn action(kind: i32, operand: &str) -> Vec<String> {
    let parameter = schema::opcode(false, kind).map_or(0, |s| s.parameter_type);
    vec![
        kind.to_string(),
        parameter.to_string(),
        operand.into(),
        "0".into(),
        "0".into(),
        "0".into(),
        "0".into(),
        "A".into(),
    ]
}
#[derive(Clone, Debug)]
pub struct Issue {
    pub kind: Kind,
    pub id: String,
    pub message: String,
    pub cell: Option<Cell>,
}
#[path = "mission_graph.rs"]
pub mod graph;
#[path = "mission_schema.rs"]
pub mod schema;
#[path = "mission_validation.rs"]
mod validation;
pub use validation::{Diagnostic, Severity, diagnostics};

/// Map-only diagnostics retained for callers without an installation context.
pub fn issues(doc: &MapDocument) -> Vec<Issue> {
    diagnostics(doc, None)
        .into_iter()
        .map(|d| d.issue)
        .collect()
}
/// Convert the current map and add an independent, uniquely named mission graph.
pub fn seed(doc: &mut MapDocument) -> Result<String> {
    doc.atomic(|doc| {
        for (id, country, color, control) in [
            ("Americans", "Americans", "Gold", "yes"),
            ("Russians", "Russians", "Red", "no"),
        ] {
            if !ids(doc, Kind::House).contains(&id.into()) {
                create(doc, Kind::House, id, "Americans", None)?;
                for (k, v) in [
                    ("Country", country),
                    ("Color", color),
                    ("PlayerControl", control),
                ] {
                    put(doc, id, k, v)?;
                }
            }
        }
        for (k, v) in [
            ("Player", "Americans"),
            ("MultiplayerOnly", "no"),
            ("HomeCell", "0"),
            ("EndOfGame", "yes"),
            ("OneTimeOnly", "yes"),
            ("SkipScore", "yes"),
        ] {
            put(doc, "Basic", k, v)?;
        }
        let tf = next_id(doc, Kind::TaskForce);
        create(doc, Kind::TaskForce, &tf, "Russians", None)?;
        let script = next_id(doc, Kind::Script);
        create(doc, Kind::Script, &script, "Russians", None)?;
        let team = next_id(doc, Kind::Team);
        create(doc, Kind::Team, &team, "Russians", None)?;
        put(doc, &team, "Script", script)?;
        put(doc, &team, "TaskForce", tf)?;
        put(doc, &team, "Waypoint", "B")?;
        let variable = next_id(doc, Kind::Variable);
        create(doc, Kind::Variable, &variable, "Americans", None)?;
        let start = next_id(doc, Kind::Trigger);
        create(doc, Kind::Trigger, &start, "Americans", None)?;
        put(doc, "Events", &start, "1,13,0,2")?;
        put(
            doc,
            "Actions",
            &start,
            encode_list(&[action(7, &team), action(56, &variable)]),
        )?;
        let win = next_id(doc, Kind::Trigger);
        create(doc, Kind::Trigger, &win, "Americans", None)?;
        put(doc, "Events", &win, format!("1,36,0,{variable}"))?;
        put(doc, "Actions", &win, encode_list(&[action(67, "0")]))?;
        // Give each side a persistent structure so automatic defeat cannot race
        // the trigger demonstration. Existing authored records are retained.
        for (owner, wp, kind) in [("Americans", "0", "GAPOWR"), ("Russians", "1", "NAPOWR")] {
            let at = doc
                .records()
                .values()
                .find(|r| r.section == "Waypoints" && r.key == wp)
                .and_then(|r| r.cell())
                .context("示例任务需要航点 0 和 1")?;
            doc.add_record("Structures", at, owner, kind)?;
        }
        Ok(start)
    })
}

#[cfg(test)]
#[path = "mission_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mission_authoring_tests.rs"]
mod authoring_tests;
