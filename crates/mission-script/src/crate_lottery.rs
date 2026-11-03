//! Paid supply building with native presence/absence triggers and prize teams.
//! Ordinary crate rules are never changed. No engine extensions.
use super::*;
use vera20k_core::rules::powerups::POWERUP_NAMES;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrateLotteryRules {
    pub ticket: Reference,
    pub rewards: Vec<Reference>,
    pub ui_name: Reference,
    pub reward_at: Waypoint,
    #[serde(default)]
    pub prerequisites: Option<Vec<Reference>>,
    #[serde(default)]
    pub cost: Option<u32>,
    #[serde(default)]
    pub strength: Option<u32>,
}

// Only these rule fields may be restored from a mission ownership manifest.
// Type sections may live in retail rules rather than in the map's VehicleTypes.
pub(super) fn is_managed_field(section: &str, key: &str) -> bool {
    match section {
        "Powerups" => POWERUP_NAMES.contains(&key),
        "CrateRules" => key == "UnitCrateType",
        _ => {
            !section.is_empty()
                && section.len() <= 63
                && section
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && matches!(
                    key,
                    "BuildLimit"
                        | "Owner"
                        | "RequiredHouses"
                        | "ForbiddenHouses"
                        | "CrateGoodie"
                        | "CarriesCrate"
                        | "IsTrain"
                        | "Crewed"
                        | "Explodes"
                        | "Cost"
                        | "PlaceAsCrate"
                        | "UIName"
                        | "Wall"
                        | "TechLevel"
                        | "BuildCat"
                        | "IsBaseDefense"
                        | "GuardRange"
                        | "BaseNormal"
                        | "Unsellable"
                        | "Prerequisite"
                        | "Strength"
                        | "Armor"
                        | "CrateBeneath"
                        | "CrateBeneathIsMoney"
                        | "LeaveRubble"
                        | "Power"
                        | "Primary"
                        | "Secondary"
                        | "ElitePrimary"
                        | "EliteSecondary"
                        | "Immune"
                        | "Capturable"
                        | "CanBeOccupied"
                        | "Repairable"
                        | "ClickRepairable"
                        | "DeathWeapon"
                )
        }
    }
}

pub(super) fn write(
    options: &CrateLotteryRules,
    catalog: &Catalog,
    reward_ids: &BTreeMap<String, String>,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let ticket = catalog.resource_id(&options.ticket, crate::catalog::ResourceKind::Building)?;
    ensure!(
        is_managed_field(ticket, "CrateBeneath") && ticket.len() <= 24,
        "补给箱建筑 ID 须为不超过 24 字节的原生类型名"
    );
    let label = options.ui_name.check("text")?;
    ensure!(
        catalog.strings.iter().any(|id| id == label),
        "补给箱 UIName 不在文本目录中"
    );
    let vehicles = catalog
        .registry_order
        .get("vehicles")
        .context("奖励车辆缺少原生目录，请重新加载资源并刷新 SDK 类型")?;
    ensure!(
        !options.rewards.is_empty() && options.rewards.len() <= 64,
        "奖励车辆池须有 1～64 项"
    );
    let mut rewards = BTreeSet::new();
    for reference in &options.rewards {
        let id = if reference.kind == "rewardUnit" {
            reward_ids
                .get(&reference.id)
                .with_context(|| format!("奖励类型 {} 未定义", reference.id))?
                .as_str()
        } else {
            reference.check("unit")?
        };
        ensure!(
            reward_ids.values().any(|unit| unit == id)
                || (catalog.units.iter().any(|unit| unit == id)
                    && vehicles
                        .iter()
                        .filter(|name| name.eq_ignore_ascii_case(id))
                        .count()
                        == 1),
            "奖励 {id} 须为唯一车辆类型，不接受步兵或飞机"
        );
        ensure!(
            rewards.insert(id.to_ascii_lowercase()),
            "奖励车辆重复：{id}"
        );
    }
    if let Some(cost) = options.cost {
        ensure!(
            cost > 0 && cost <= i32::MAX as u32,
            "cost 须为 1～2147483647 credits"
        );
        put(ticket, "Cost", cost.to_string())?;
    }
    let strength = options.strength.unwrap_or(20);
    ensure!(
        strength > 0 && strength <= i32::MAX as u32,
        "strength 须为 1～2147483647"
    );
    put(ticket, "Strength", strength.to_string())?;
    put(ticket, "UIName", label.into())?;
    for (key, value) in [
        ("CrateBeneath", "no"),
        ("BuildLimit", "1"),
        ("Crewed", "no"),
        ("CrateBeneathIsMoney", "no"),
        ("LeaveRubble", "no"),
        ("Wall", "no"),
        ("TechLevel", "1"),
        ("BuildCat", "Infrastructure"),
        ("IsBaseDefense", "no"),
        ("BaseNormal", "no"),
        ("Unsellable", "yes"),
        ("Power", "0"),
        ("Armor", "wood"),
        ("Primary", "<none>"),
        ("Secondary", "<none>"),
        ("ElitePrimary", "<none>"),
        ("EliteSecondary", "<none>"),
        ("Explodes", "no"),
        ("DeathWeapon", "<none>"),
        ("Immune", "no"),
        ("Capturable", "no"),
        ("CanBeOccupied", "no"),
        ("Repairable", "no"),
        ("ClickRepairable", "no"),
    ] {
        put(ticket, key, value.into())?;
    }
    if let Some(prerequisites) = &options.prerequisites {
        ensure!(prerequisites.len() <= 16, "建造前置最多 16 项");
        let ids = prerequisites
            .iter()
            .map(|r| catalog.resource_id(r, crate::catalog::ResourceKind::Building))
            .collect::<Result<Vec<_>>>()?;
        let value = if ids.is_empty() {
            "<none>".into()
        } else {
            ids.join(",")
        };
        ensure!(value.len() <= 127, "建造前置超过原生读取范围");
        put(ticket, "Prerequisite", value)?;
    }
    waypoint(&options.reward_at, catalog)?;
    Ok(())
}

// Allocate through the ordinary local-variable allocator, including conflict checks.
pub(super) fn with_flags(plan: &Mission) -> Result<Option<Mission>> {
    if plan.crate_lottery.is_none() {
        return Ok(None);
    }
    ensure!(
        plan.mode == MissionMode::SinglePlayer,
        "crateLottery 拆毁发奖目前仅支持单人任务"
    );
    let mut value = serde_json::to_value(plan)?;
    for name in ["armed", "pending"] {
        let id = physical(&plan.id, name, "LF");
        ensure!(
            !plan.flags.iter().any(|flag| flag.id == id),
            "抽奖标记与任务声明冲突"
        );
        value["flags"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id":id,"initial":false}));
    }
    Ok(Some(serde_json::from_value(value)?))
}

fn runtime_id(plan: &Mission, name: &str, kind: &str) -> String {
    physical(&plan.id, &format!("lottery_{name}"), kind)
}

/// Script17 operands must see the lottery's TeamType-owned Script allocation.
pub(super) fn project_scripts(plan: &Mission, doc: &mut MapDocument) -> Result<()> {
    let Some(options) = &plan.crate_lottery else {
        return Ok(());
    };
    let script = runtime_id(plan, "prize", "SC");
    doc.set_ini("ScriptTypes", &script, Some(script.as_bytes().to_vec()))?;
    doc.set_ini(&script, "Name", Some(b"lottery_release".to_vec()))?;
    for index in 0..options.rewards.len() {
        let team = runtime_id(plan, &format!("prize_{index}"), "TM");
        doc.set_ini("TeamTypes", &team, Some(team.as_bytes().to_vec()))?;
        doc.set_ini(&team, "Script", Some(script.as_bytes().to_vec()))?;
    }
    Ok(())
}

pub(super) fn write_runtime(
    plan: &Mission,
    doc: &MapDocument,
    houses_doc: &MapDocument,
    catalog: &Catalog,
    flags: &BTreeMap<String, String>,
    reward_ids: &BTreeMap<String, String>,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let Some(options) = &plan.crate_lottery else {
        return Ok(());
    };
    let ticket = options.ticket.check("building")?;
    ensure!(
        !doc.records()
            .values()
            .any(|record| record.section == "Structures"
                && record.type_id().eq_ignore_ascii_case(ticket)),
        "抽奖 ticket 类型须专用，地图不能预放该建筑：{ticket}"
    );
    let ini =
        vera20k_core::rules::ini_parser::IniFile::from_bytes(&houses_doc.snapshot().bytes()?)?;
    let roster = vera20k_core::map::houses::parse_house_roster(&ini, &[], None);
    let player = roster
        .houses
        .iter()
        .find(|house| house.name == plan.player)
        .context("抽奖玩家须在地图 Houses 中注册")?;
    let default_country = catalog
        .fixed_country_indices
        .iter()
        .find(|(_, index)| **index == 0)
        .map(|(name, _)| name.as_str());
    let resolve = |token: &str| {
        catalog
            .country_names
            .get(&token.to_ascii_lowercase())
            .map(String::as_str)
            .or_else(|| {
                catalog
                    .countries
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(token))
                    .map(String::as_str)
            })
    };
    let country = player
        .country
        .as_deref()
        .or(default_country)
        .and_then(resolve)
        .context("抽奖玩家 Country 无有效资源映射")?;
    ensure!(
        roster
            .houses
            .iter()
            .filter(|house| house
                .country
                .as_deref()
                .or(default_country)
                .and_then(resolve)
                == Some(country))
            .count()
            == 1,
        "抽奖玩家不能与其他 House 共享 Country；原生触发器无法区分"
    );
    for (key, value) in [
        ("Owner", country),
        ("RequiredHouses", country),
        ("ForbiddenHouses", "<none>"),
    ] {
        put(ticket, key, value.into())?;
    }
    ensure!(
        plan.teams.len() + options.rewards.len() <= 256,
        "抽奖奖励与任务小队总数超过 256"
    );
    let armed = flags[&physical(&plan.id, "armed", "LF")].clone();
    let pending = flags[&physical(&plan.id, "pending", "LF")].clone();
    let id = |name: &str, kind: &str| runtime_id(plan, name, kind);
    let control = |opcode, target: String| action_row(opcode, Some(target), None);
    let flag_event = |opcode: i32, slot: &str| vec![opcode.to_string(), "0".into(), slot.into()];
    let building_index = catalog
        .resource_index(&options.ticket, crate::catalog::ResourceKind::Building)?
        .to_string();
    let level = |value: &str| {
        let mut row = mission::schema::row(false, 106).unwrap();
        row[2] = ticket.into();
        row[7] = value.into();
        row
    };
    let mut rule = |name: &str,
                    enabled: bool,
                    events: Vec<Vec<String>>,
                    actions: Vec<Vec<String>>|
     -> Result<()> {
        let trigger = id(name, "TR");
        let tag = id(name, "TG");
        let events = mission::encode_list(&events);
        let actions = mission::encode_list(&actions);
        ensure!(
            events.len() < 512 && actions.len() < 512,
            "抽奖辅助规则超过原生读取容量"
        );
        put(
            "Triggers",
            &trigger,
            format!(
                "{country},<none>,lottery_{name},{},1,1,1,0",
                u8::from(!enabled)
            ),
        )?;
        put("Tags", &tag, format!("2,lottery_{name},{trigger}"))?;
        put("Events", &trigger, events)?;
        put("Actions", &trigger, actions)?;
        Ok(())
    };
    // BuildLimit prevents two on-map tickets. While armed or awarding, the
    // type is also hidden to keep later production from masking the transition.
    rule(
        "watch",
        true,
        vec![
            flag_event(37, &armed),
            flag_event(37, &pending),
            flag_event(32, &building_index),
        ],
        vec![control(56, armed.clone()), level("-1")],
    )?;
    rule(
        "open",
        true,
        vec![flag_event(36, &armed), flag_event(57, &building_index)],
        vec![
            control(57, armed),
            control(56, pending.clone()),
            control(22, id("reset_0", "TR")),
        ],
    )?;
    // A small fan-out tree resets every random timer without exceeding the
    // native 512-byte action-list reader for a large reward pool.
    let branches: Vec<_> = (0..options.rewards.len())
        .map(|i| id(&format!("prize_{i}"), "TR"))
        .collect();
    let chunks: Vec<_> = branches.chunks(4).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        let mut actions: Vec<_> = chunk
            .iter()
            .map(|target| control(53, target.clone()))
            .collect();
        if index + 1 < chunks.len() {
            actions.push(control(22, id(&format!("reset_{}", index + 1), "TR")));
        }
        rule(
            &format!("reset_{index}"),
            true,
            vec![flag_event(36, &pending), flag_event(37, &pending)],
            actions,
        )?;
    }
    for (index, _) in options.rewards.iter().enumerate() {
        let team = id(&format!("prize_{index}"), "TM");
        // First expired random timer consumes the pending flag synchronously.
        // Equal-second ties follow native Tag iteration order, not equal odds.
        rule(
            &format!("prize_{index}"),
            false,
            vec![flag_event(36, &pending), flag_event(51, "1")],
            vec![
                control(57, pending.clone()),
                action_row(
                    107,
                    Some(team),
                    Some(waypoint(&options.reward_at, catalog)?),
                ),
                level("1"),
            ],
        )?;
    }
    // Reuse validated TaskForce output, privately admitting only our generated
    // reward types here; public normal Team references keep their original catalog.
    let mut prize_catalog = catalog.clone();
    prize_catalog.units.extend(reward_ids.values().cloned());
    let script = id("prize", "SC");
    put("ScriptTypes", &script, script.clone())?;
    put(&script, "Name", "lottery_release".into())?;
    put(&script, "0", "37,0".into())?; // Remove members from Team, preserving units.
    for (index, reward) in options.rewards.iter().enumerate() {
        let native = if reward.kind == "rewardUnit" {
            &reward_ids[&reward.id]
        } else {
            &reward.id
        };
        let tf = id(&format!("prize_{index}"), "TF");
        definitions::task_force(
            &tf,
            "lottery_prize",
            &[Unit {
                r#type: Reference {
                    kind: "unit".into(),
                    id: native.clone(),
                },
                count: 1,
            }],
            -1,
            &prize_catalog,
            put,
        )?;
        let team = id(&format!("prize_{index}"), "TM");
        put("TeamTypes", &team, team.clone())?;
        for (key, value) in [
            ("Name", "lottery_prize".into()),
            ("House", country.into()),
            ("TaskForce", tf),
            ("Script", script.clone()),
            ("Waypoint", waypoint(&options.reward_at, catalog)?),
            ("Max", "1".into()),
            ("Full", "no".into()),
            ("Autocreate", "no".into()),
            ("Recruiter", "no".into()),
            ("Reinforce", "yes".into()),
            ("VeteranLevel", "1".into()),
            ("TechLevel", "0".into()),
            ("AreTeamMembersRecruitable", "no".into()),
            ("Priority", "5".into()),
            ("Group", "-1".into()),
        ] {
            put(&team, key, value)?;
        }
    }
    Ok(())
}
