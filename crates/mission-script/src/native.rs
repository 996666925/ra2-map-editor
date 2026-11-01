use anyhow::{Context, Result, ensure};
use ra2_map_document::{
    document::{MapDocument, Record, Target, Value},
    mission::{self, Kind},
    startup_owner,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub version: u32,
    pub id: String,
    #[serde(default)]
    pub mode: MissionMode,
    #[serde(default)]
    pub player: String,
    pub teams: Vec<Team>,
    pub flags: Vec<Flag>,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MissionMode {
    #[default]
    SinglePlayer,
    Multiplayer,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TeamOwner {
    House { id: String },
    StartPlayer { id: u32 },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub kind: String,
    pub id: String,
}
impl Reference {
    fn check(&self, kind: &str) -> Result<&str> {
        ensure!(
            self.kind == kind,
            "引用 {} 应为 {kind}，实际为 {}",
            self.id,
            self.kind
        );
        Ok(&self.id)
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    pub kind: String,
    pub id: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub r#type: Reference,
    pub count: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub id: String,
    pub house: TeamOwner,
    pub units: Vec<Unit>,
    pub orders: Vec<Order>,
    pub waypoint: Waypoint,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Order {
    MoveTo { waypoint: Waypoint },
    AttackWaypoint { waypoint: Waypoint },
    Patrol { waypoint: Waypoint },
    Guard { seconds: u32 },
    Hunt,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flag {
    pub id: String,
    pub initial: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub when: Event,
    pub once: bool,
    pub actions: Vec<Action>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    ScenarioTime { seconds: u32 },
    EnabledTime { seconds: u32 },
    Destroyed { object: Reference },
    FlagSet { flag: Reference },
    FlagClear { flag: Reference },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Reinforce { team: Reference, at: Waypoint },
    Recruit { team: Reference },
    Message { text: Reference },
    SetFlag { flag: Reference },
    ClearFlag { flag: Reference },
    WinPlayer,
    LosePlayer,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Catalog {
    pub houses: Vec<String>,
    pub waypoints: Vec<u32>,
    pub units: Vec<String>,
    pub objects: Vec<String>,
    pub strings: Vec<String>,
    /// Authoring-only descriptions, grouped by MapSchema collection and original ID.
    #[serde(skip)]
    pub descriptions: BTreeMap<String, BTreeMap<String, String>>,
}
impl Catalog {
    pub fn from_document(doc: &MapDocument, units: Vec<String>, strings: Vec<String>) -> Self {
        let records = doc.records();
        let mut result = Self {
            houses: mission::ids(doc, Kind::House),
            waypoints: records
                .values()
                .filter(|r| r.section == "Waypoints")
                .filter_map(|r| r.key.parse().ok())
                .collect(),
            objects: records
                .values()
                .filter(|r| startup_owner::tag_column(&r.section).is_some())
                .map(|r| format!("{}:{}", r.section, r.key))
                .collect(),
            units,
            strings,
            descriptions: BTreeMap::new(),
        };
        result.houses.extend(["Neutral".into(), "Special".into()]);
        result.houses.sort();
        result.houses.dedup();
        result.waypoints.sort();
        result.waypoints.dedup();
        result.units.sort();
        result.units.dedup();
        result.objects.sort();
        result.objects.dedup();
        result.strings.sort();
        result.strings.dedup();
        result
    }
    pub fn typescript(&self) -> Result<String> {
        let mut source = "// Generated from the current map and resource profile. Refresh in the editor.\n// 当前地图的引用枚举；请通过编辑器刷新，不要手工修改。\n\n".to_owned();
        let schema = serde_json::to_value(self)?;
        let mut collections = Vec::new();
        for (collection, name, label) in [
            ("houses", "House", "所属方"),
            ("waypoints", "Waypoint", "航点"),
            ("units", "Unit", "单位类型"),
            ("objects", "MapObject", "地图对象"),
            ("strings", "Text", "文本标签"),
        ] {
            source.push_str(&format!(
                "/** 当前地图可用的{label}。成员值保留原始 ID 或编号。 */\nexport enum {name} {{\n"
            ));
            let values = schema[collection].as_array().expect("catalog collection");
            let members = enum_members(values, collection == "waypoints");
            for (id, member) in values.iter().zip(members) {
                let literal = serde_json::to_string(id)?;
                let key = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
                let description = self
                    .descriptions
                    .get(collection)
                    .and_then(|items| items.get(&key));
                let description = description
                    .cloned()
                    .unwrap_or_else(|| format!("{label}：{key}"));
                // Resource text must not terminate a comment or inject JSDoc tags/source lines.
                let description = description
                    .replace("*/", "* /")
                    .replace(['\r', '\n', '\u{2028}', '\u{2029}'], " ");
                source.push_str(&format!(
                    "  /** {description} */\n  {member} = {literal},\n"
                ));
            }
            source.push_str("}\n\n");
            // Enum types already enumerate exactly the available IDs. Avoid
            // inferring a thousands-element tuple for large CSF/object lists.
            let array = if values.is_empty() {
                "[] as readonly never[]".to_owned()
            } else {
                // Each member already appears in the enum. Referencing every
                // member again in a contextual array forces repeated union
                // assignability checks for large CSF catalogs.
                let filter = if collection == "waypoints" {
                    ".filter(key => isNaN(Number(key)))"
                } else {
                    ""
                };
                format!(
                    "Object.keys({name}){filter}.map(key => {name}[key as keyof typeof {name}]) as readonly {name}[]"
                )
            };
            collections.push(format!("  {collection}: {array}"));
        }
        source.push_str(
            "/** 当前地图的类型安全引用目录，由上述枚举组成。 */\nexport const map = {\n",
        );
        source.push_str(&collections.join(",\n"));
        source.push_str("\n} as const;\n");
        Ok(source)
    }

    /// Resolve the generated enum member without duplicating collision handling in templates.
    pub fn enum_member(&self, collection: &str, id: &str) -> Result<String> {
        let schema = serde_json::to_value(self)?;
        let values = schema[collection].as_array().context("未知的引用目录")?;
        values
            .iter()
            .zip(enum_members(values, collection == "waypoints"))
            .find(|(value, _)| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
                    == id
            })
            .map(|(_, member)| member)
            .context("引用不在当前目录中")
    }
}

fn enum_members(values: &[serde_json::Value], numeric: bool) -> Vec<String> {
    let mut used = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let id = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            let mut base: String = id
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            if numeric {
                base.insert_str(0, "WP");
            } else if base.is_empty()
                || base.starts_with(|ch: char| ch.is_ascii_digit())
                || base == "__proto__"
            {
                base.insert(0, '_');
            }
            let mut member = base.clone();
            let mut suffix = 2;
            while !used.insert(member.clone()) {
                member = format!("{base}_{suffix}");
                suffix += 1;
            }
            member
        })
        .collect()
}

const META: &str = "EditorMissionScript";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Field {
    section: String,
    key: String,
    value: String,
    before: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Attachment {
    object: String,
    type_id: String,
    before: String,
    after: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    mission: String,
    fields: Vec<Field>,
    attachments: Vec<Attachment>,
    flags: BTreeMap<String, u32>,
}

/// Immutable prepared map bytes are applied only to the exact input revision.
pub struct Build {
    pub document: u64,
    pub state: u64,
    pub bytes: Vec<u8>,
    pub field_count: usize,
    pub rule_count: usize,
    pub team_count: usize,
}
impl Build {
    pub fn apply(&self, doc: &mut MapDocument) -> Result<bool> {
        ensure!(
            (doc.id, doc.state_id) == (self.document, self.state),
            "地图已变化，请重新构建任务"
        );
        doc.replace_bytes(self.bytes.clone(), "应用 TypeScript 任务")
    }
}

fn object(doc: &MapDocument, id: &str) -> Result<(u64, Record, usize)> {
    let (section, key) = id.split_once(':').context("对象引用必须是 section:key")?;
    let column = startup_owner::tag_column(section).context("对象不支持 Tag")?;
    let matches: Vec<_> = doc
        .records()
        .into_iter()
        .filter(|(_, r)| r.section == section && r.key == key)
        .collect();
    ensure!(matches.len() == 1, "对象 {id} 不存在或有歧义");
    let (record_id, record) = matches.into_iter().next().unwrap();
    ensure!(
        doc.base
            .raw
            .entries(section)
            .filter(|(_, r)| r.key.as_deref() == Some(key))
            .count()
            <= 1,
        "对象 {id} 存在重复记录，请先消歧"
    );
    Ok((record_id, record, column))
}

fn read_manifest(doc: &MapDocument) -> Result<Manifest> {
    let entries = doc.ini_fields(META);
    if entries.is_empty() {
        return Ok(Manifest::default());
    }
    ensure!(
        doc.ini_text(META, "Version") == "1",
        "不支持的任务脚本元数据版本"
    );
    let count: usize = doc
        .ini_text(META, "Chunks")
        .parse()
        .context("任务归属清单损坏")?;
    ensure!(count <= 32768, "任务归属清单过大");
    let mut hex = String::new();
    for index in 0..count {
        hex.push_str(&doc.ini_text(META, &format!("Data{index}")));
    }
    ensure!(
        hex.len().is_multiple_of(2) && hex.is_ascii(),
        "任务归属清单编码损坏"
    );
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()?;
    let result: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        result.version == 1 && result.fields.len() <= 16384 && result.attachments.len() <= 1024,
        "任务归属清单无效"
    );
    for field in &result.fields {
        let valid = match field.section.as_str() {
            "Basic" => matches!(field.key.as_str(), "Player" | "MultiplayerOnly"),
            "VariableNames" => field.key.parse::<u8>().is_ok_and(|n| n < 100),
            "Triggers" | "Events" | "Actions" | "Tags" | "TaskForces" | "ScriptTypes"
            | "TeamTypes" => field.key.starts_with("TS"),
            s => s.starts_with("TS") && s.len() == 20,
        };
        ensure!(
            valid,
            "任务归属清单包含非托管字段 {}/{}",
            field.section,
            field.key
        );
    }
    Ok(result)
}
fn restore(doc: &mut MapDocument, old: &Manifest) -> Result<()> {
    for attachment in &old.attachments {
        let (id, record, column) = object(doc, &attachment.object)?;
        ensure!(
            record.type_id() == attachment.type_id
                && record.fields().get(column) == Some(&attachment.after),
            "对象 {} 的任务绑定已被修改，请先处理冲突",
            attachment.object
        );
        doc.set(
            Target::Record(id),
            Some(Value::Record(
                record.patched(&[(column, attachment.before.clone())])?,
            )),
        )?;
    }
    // Extra fields added by another tool must not become orphaned on rebuild.
    let generated_sections: BTreeSet<_> = old
        .fields
        .iter()
        .filter(|f| f.section.starts_with("TS"))
        .map(|f| &f.section)
        .collect();
    for section in generated_sections {
        ensure!(
            doc.ini_entries(section).iter().all(|(_, key, _)| old
                .fields
                .iter()
                .any(|f| &f.section == section && &f.key == key)),
            "生成项 {section} 存在手工新增字段，请先处理冲突"
        );
    }
    for field in &old.fields {
        ensure!(
            doc.ini_value(&field.section, &field.key).as_deref() == Some(field.value.as_bytes()),
            "生成项 {}/{} 已被手工修改，请先处理冲突",
            field.section,
            field.key
        );
        doc.set_ini(
            &field.section,
            &field.key,
            field.before.as_ref().map(|s| s.as_bytes().to_vec()),
        )?;
    }
    Ok(())
}

fn identifier(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "逻辑名称须为 1～64 位 ASCII 字母、数字或下划线：{id}"
    );
    Ok(())
}
fn physical(mission: &str, logical: &str, kind: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for b in mission.bytes().chain(*b"/").chain(logical.bytes()) {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("TS{kind}{hash:016X}")
}
fn seconds(value: u32) -> Result<String> {
    ensure!(value <= i32::MAX as u32 / 15, "时间超出逻辑帧范围");
    Ok(value.to_string())
}
fn waypoint(value: &Waypoint, catalog: &Catalog) -> Result<String> {
    ensure!(
        value.kind == "waypoint" && catalog.waypoints.contains(&value.id),
        "航点 {} 不存在或类型错误",
        value.id
    );
    mission::schema::waypoint_token(value.id)
}
fn named(map: &BTreeMap<String, String>, reference: &Reference, kind: &str) -> Result<String> {
    map.get(reference.check(kind)?)
        .cloned()
        .with_context(|| format!("未定义的 {kind}：{}", reference.id))
}
fn action_row(opcode: i32, value: Option<String>, tail: Option<String>) -> Vec<String> {
    let mut row = mission::schema::row(false, opcode).expect("known action schema");
    if let Some(value) = value {
        row[2] = value;
    }
    if let Some(tail) = tail {
        row[7] = tail;
    }
    row
}

pub fn prepare(doc: &MapDocument, plan: &Mission, catalog: &Catalog) -> Result<Build> {
    ensure!(plan.version == 1, "不支持的任务 SDK 版本");
    identifier(&plan.id)?;
    let trigger_owner = match plan.mode {
        MissionMode::SinglePlayer => {
            ensure!(
                catalog.houses.contains(&plan.player),
                "玩家 House {} 不存在",
                plan.player
            );
            plan.player.as_str()
        }
        MissionMode::Multiplayer => {
            ensure!(
                plan.player.is_empty(),
                "多人任务不能指定固定的 player House"
            );
            "Neutral"
        }
    };
    ensure!(
        plan.rules.len() <= 1024 && plan.teams.len() <= 256 && plan.flags.len() <= 100,
        "任务定义超过容量"
    );
    let old = read_manifest(doc)?;
    let mut scratch = doc.fork();
    restore(&mut scratch, &old)?;
    let mut desired = BTreeMap::<(String, String), String>::new();
    let mut put = |section: &str, key: &str, value: String| -> Result<()> {
        ensure!(
            value.len() <= 480,
            "生成字段 {section}/{key} 超过 480 字节，请拆分规则"
        );
        ensure!(
            desired
                .insert((section.into(), key.into()), value)
                .is_none(),
            "生成 ID 或字段冲突：{section}/{key}"
        );
        Ok(())
    };
    if plan.mode == MissionMode::SinglePlayer {
        put("Basic", "Player", plan.player.clone())?;
    }
    put(
        "Basic",
        "MultiplayerOnly",
        if plan.mode == MissionMode::Multiplayer {
            "yes"
        } else {
            "no"
        }
        .into(),
    )?;
    let mut flags = BTreeMap::new();
    if !plan.flags.is_empty() {
        for trigger in mission::ids(&scratch, Kind::Trigger) {
            for (section, is_event) in [("Events", true), ("Actions", false)] {
                let rows = mission::decode_list(&scratch.ini_text(section, &trigger), is_event)?;
                ensure!(
                    rows.iter().all(|r| r
                        .first()
                        .and_then(|op| op.parse().ok())
                        .is_some_and(|op| mission::schema::opcode(is_event, op).is_some())),
                    "触发器 {trigger} 含未知扩展；无法安全自动分配局部标记"
                );
            }
        }
        for script in mission::ids(&scratch, Kind::Script) {
            ensure!(
                scratch
                    .ini_fields(&script)
                    .iter()
                    .filter(|(k, _)| k.parse::<usize>().is_ok())
                    .all(|(_, v)| String::from_utf8_lossy(v)
                        .split(',')
                        .next()
                        .and_then(|s| s.parse().ok())
                        .is_some_and(|op| mission::schema::scripts::opcode(op).is_some())),
                "脚本 {script} 含未知扩展；无法安全自动分配局部标记"
            );
        }
    }
    let mut used: BTreeSet<u32> = mission::ids(&scratch, Kind::Variable)
        .iter()
        .filter_map(|n| n.parse().ok())
        .collect();
    used.extend(
        mission::references(&scratch)
            .iter()
            .filter(|r| r.to.0 == Kind::Variable)
            .filter_map(|r| r.to.1.parse::<u32>().ok()),
    );
    for flag in &plan.flags {
        identifier(&flag.id)?;
        ensure!(!flags.contains_key(&flag.id), "重复标记 {}", flag.id);
        let slot = old
            .flags
            .get(&flag.id)
            .copied()
            .filter(|n| !used.contains(n))
            .or_else(|| (0..100).find(|n| !used.contains(n)))
            .context("局部变量 0～99 已满")?;
        used.insert(slot);
        flags.insert(flag.id.clone(), slot.to_string());
        put(
            "VariableNames",
            &slot.to_string(),
            format!("{}_{},{}", plan.id, flag.id, u8::from(flag.initial)),
        )?;
    }
    let mut teams = BTreeMap::new();
    for team in &plan.teams {
        identifier(&team.id)?;
        ensure!(!teams.contains_key(&team.id), "重复小队 {}", team.id);
        let owner = match &team.house {
            TeamOwner::House { id } => {
                ensure!(
                    catalog.houses.contains(id),
                    "小队 {} 的 House 无效",
                    team.id
                );
                id.clone()
            }
            TeamOwner::StartPlayer { id } => {
                ensure!(
                    plan.mode == MissionMode::Multiplayer,
                    "按起点选择玩家仅支持多人任务"
                );
                ensure!(
                    *id < 8 && catalog.waypoints.contains(id),
                    "小队 {} 的玩家起点必须是地图已有的 0～7 号航点",
                    team.id
                );
                format!("<Player @ {}>", char::from(b'A' + *id as u8))
            }
        };
        ensure!(
            (1..=6).contains(&team.units.len()) && (1..=50).contains(&team.orders.len()),
            "小队 {} 需要 1～6 种成员、1～50 行行为",
            team.id
        );
        let tf = physical(&plan.id, &team.id, "TF");
        let sc = physical(&plan.id, &team.id, "SC");
        let tm = physical(&plan.id, &team.id, "TM");
        put("TaskForces", &tf, tf.clone())?;
        put("ScriptTypes", &sc, sc.clone())?;
        put("TeamTypes", &tm, tm.clone())?;
        put(&tf, "Name", team.id.clone())?;
        put(&tf, "Group", "-1".into())?;
        let mut types = BTreeSet::new();
        for (index, unit) in team.units.iter().enumerate() {
            let id = unit.r#type.check("unit")?;
            ensure!(
                catalog.units.iter().any(|u| u == id)
                    && types.insert(id)
                    && unit.count > 0
                    && unit.count <= i32::MAX as u32,
                "小队 {} 的成员 {} 不存在、重复或数量无效",
                team.id,
                id
            );
            put(&tf, &index.to_string(), format!("{},{id}", unit.count))?;
        }
        put(&sc, "Name", team.id.clone())?;
        for (index, order) in team.orders.iter().enumerate() {
            let (opcode, operand) = match order {
                Order::MoveTo { waypoint: wp } => {
                    waypoint(wp, catalog)?;
                    (3, wp.id.to_string())
                }
                Order::AttackWaypoint { waypoint: wp } => {
                    waypoint(wp, catalog)?;
                    (1, wp.id.to_string())
                }
                Order::Patrol { waypoint: wp } => {
                    waypoint(wp, catalog)?;
                    (16, wp.id.to_string())
                }
                Order::Guard { seconds: s } => (5, seconds(*s)?),
                Order::Hunt => (11, "15".into()),
            };
            put(&sc, &index.to_string(), format!("{opcode},{operand}"))?;
        }
        for (key, value) in [
            ("Name", team.id.clone()),
            ("House", owner),
            ("TaskForce", tf),
            ("Script", sc),
            ("Waypoint", waypoint(&team.waypoint, catalog)?),
        ] {
            put(&tm, key, value)?;
        }
        for (key, value) in [
            ("Max", "1"),
            ("Priority", "5"),
            ("VeteranLevel", "1"),
            ("TechLevel", "0"),
            ("Group", "-1"),
            ("Autocreate", "no"),
            ("Full", "yes"),
            ("Recruiter", "no"),
            ("Prebuild", "no"),
            ("Reinforce", "yes"),
            ("Suicide", "no"),
        ] {
            put(&tm, key, value.into())?;
        }
        teams.insert(team.id.clone(), tm);
    }
    let mut attachments = Vec::new();
    let mut rule_ids = BTreeSet::new();
    let mut bound_objects = BTreeSet::new();
    for rule in &plan.rules {
        identifier(&rule.id)?;
        ensure!(rule_ids.insert(&rule.id), "重复规则 {}", rule.id);
        ensure!(
            rule.once && (1..=64).contains(&rule.actions.len()),
            "规则 {} 仅支持 once:true 和 1～64 个动作",
            rule.id
        );
        let tr = physical(&plan.id, &rule.id, "TR");
        let tag = physical(&plan.id, &rule.id, "TG");
        let event = match &rule.when {
            Event::ScenarioTime { seconds: s } => vec!["47".into(), "0".into(), seconds(*s)?],
            Event::EnabledTime { seconds: s } => vec!["13".into(), "0".into(), seconds(*s)?],
            Event::FlagSet { flag } => vec!["36".into(), "0".into(), named(&flags, flag, "flag")?],
            Event::FlagClear { flag } => {
                vec!["37".into(), "0".into(), named(&flags, flag, "flag")?]
            }
            Event::Destroyed { object: reference } => {
                let name = reference.check("object")?;
                ensure!(
                    catalog.objects.iter().any(|o| o == name)
                        && bound_objects.insert(name.to_owned()),
                    "对象 {name} 不存在或已被其他规则绑定"
                );
                let (_, record, column) = object(&scratch, name)?;
                let before = record
                    .fields()
                    .get(column)
                    .cloned()
                    .context("对象缺少 Tag 字段")?;
                ensure!(
                    before.eq_ignore_ascii_case("none") || before.eq_ignore_ascii_case("<none>"),
                    "对象 {name} 已有 Tag {before}，不能覆盖"
                );
                attachments.push(Attachment {
                    object: name.into(),
                    type_id: record.type_id(),
                    before,
                    after: tag.clone(),
                });
                vec!["7".into(), "0".into(), "0".into()]
            }
        };
        let mut actions = Vec::new();
        for action in &rule.actions {
            actions.push(match action {
                Action::Reinforce { team, at } => action_row(
                    80,
                    Some(named(&teams, team, "team")?),
                    Some(waypoint(at, catalog)?),
                ),
                Action::Recruit { team } => action_row(4, Some(named(&teams, team, "team")?), None),
                Action::Message { text } => {
                    let label = text.check("text")?;
                    ensure!(
                        label.len() <= 31
                            && label.is_ascii()
                            && !label.contains([',', ';', '\r', '\n'])
                            && catalog.strings.iter().any(|s| s == label),
                        "CSF 标签 {label} 不存在或超出原生字段范围"
                    );
                    action_row(11, Some(label.into()), None)
                }
                Action::SetFlag { flag } => {
                    action_row(56, Some(named(&flags, flag, "flag")?), None)
                }
                Action::ClearFlag { flag } => {
                    action_row(57, Some(named(&flags, flag, "flag")?), None)
                }
                Action::WinPlayer | Action::LosePlayer => {
                    ensure!(
                        plan.mode == MissionMode::SinglePlayer,
                        "winPlayer/losePlayer 仅支持单人任务"
                    );
                    action_row(
                        if matches!(action, Action::WinPlayer) {
                            67
                        } else {
                            68
                        },
                        None,
                        None,
                    )
                }
            });
        }
        put(
            "Triggers",
            &tr,
            format!("{},<none>,{},0,1,1,1,0", trigger_owner, rule.id),
        )?;
        put("Events", &tr, mission::encode_list(&[event]))?;
        put("Actions", &tr, mission::encode_list(&actions))?;
        put("Tags", &tag, format!("0,{},{}", rule.id, tr))?;
    }
    let mut fields = Vec::new();
    for (section, _) in desired.keys().filter(|(s, _)| s.starts_with("TS")) {
        ensure!(
            scratch.ini_entries(section).is_empty(),
            "生成 section {section} 与手工记录冲突"
        );
    }
    for ((section, key), value) in desired {
        let previous = scratch.ini_value(&section, &key);
        if section != "Basic" {
            ensure!(previous.is_none(), "生成项与手工记录冲突：{section}/{key}");
            ensure!(
                !scratch
                    .ini_entries(&section)
                    .iter()
                    .any(|(_, k, _)| k.eq_ignore_ascii_case(&key)),
                "生成项大小写冲突：{section}/{key}"
            );
        }
        scratch.set_ini(&section, &key, Some(value.as_bytes().to_vec()))?;
        fields.push(Field {
            section,
            key,
            value,
            before: previous.map(|v| String::from_utf8_lossy(&v).into_owned()),
        });
    }
    for attachment in &attachments {
        let (id, record, column) = object(&scratch, &attachment.object)?;
        scratch.set(
            Target::Record(id),
            Some(Value::Record(
                record.patched(&[(column, attachment.after.clone())])?,
            )),
        )?;
    }
    for r in mission::references(&scratch) {
        if old
            .fields
            .iter()
            .any(|f| f.section == r.to.0.registry() && (f.key == r.to.1 || f.value == r.to.1))
        {
            ensure!(
                mission::ids(&scratch, r.to.0)
                    .iter()
                    .any(|id| mission::same_id(r.to.0, id, &r.to.1)),
                "删除的生成项仍被 {} {} 引用：{}",
                r.from.0.label(),
                r.from.1,
                r.to.1
            );
        }
    }
    let manifest = Manifest {
        version: 1,
        mission: plan.id.clone(),
        flags: flags
            .into_iter()
            .map(|(id, n)| (id, n.parse().unwrap()))
            .collect(),
        fields,
        attachments,
    };
    for (_, key, _) in scratch.ini_entries(META) {
        scratch.set_ini(META, &key, None)?;
    }
    let hex: String = serde_json::to_vec(&manifest)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let chunks: Vec<_> = hex.as_bytes().chunks(400).collect();
    scratch.set_ini(META, "Version", Some(b"1".to_vec()))?;
    scratch.set_ini(META, "Chunks", Some(chunks.len().to_string().into_bytes()))?;
    for (index, bytes) in chunks.iter().enumerate() {
        scratch.set_ini(META, &format!("Data{index}"), Some(bytes.to_vec()))?;
    }
    // Restoring/reinserting a managed field can move it past unmanaged keys.
    // Once validation confirms the same product, preserve the current layout.
    let bytes = if manifest == old {
        doc.snapshot().bytes()?
    } else {
        scratch.snapshot().bytes()?
    };
    // Reparse before publishing so invalid maps never reach the live document.
    let mut validation = doc.fork();
    validation.replace_bytes(bytes.clone(), "验证任务产物")?;
    Ok(Build {
        document: doc.id,
        state: doc.state_id,
        bytes,
        field_count: manifest.fields.len(),
        rule_count: plan.rules.len(),
        team_count: plan.teams.len(),
    })
}
