//! Scenario definitions and the references needed for safe rebuild/removal.
use super::*;

pub(super) fn check_removed_waypoints(doc: &MapDocument, removed: &BTreeSet<u32>) -> Result<()> {
    if removed.is_empty() {
        return Ok(());
    }
    let check = |value: &str, letters: bool, source: &str| -> Result<()> {
        if let Some(index) = mission::schema::waypoint_index(value, letters) {
            ensure!(
                !removed.contains(&index),
                "删除的任务航点 {index} 仍被 {source} 引用"
            );
        }
        Ok(())
    };
    for key in ["HomeCell", "AltHomeCell"] {
        check(&doc.ini_text("Basic", key), false, &format!("Basic/{key}"))?;
    }
    for team in mission::ids(doc, Kind::Team) {
        for key in ["Waypoint", "TransportWaypoint"] {
            check(&doc.ini_text(&team, key), true, &format!("{team}/{key}"))?;
        }
    }
    let check_row =
        |row: &[String], schema: &mission::schema::Opcode, source: &str| -> Result<()> {
            for parameter in schema.parameters {
                if let mission::schema::ParameterKind::Waypoint { letters } = parameter.kind {
                    let value = row
                        .get(parameter.index)
                        .context("航点参数缺失，不能安全删除任务航点")?;
                    check(value, letters, source)?;
                }
            }
            Ok(())
        };
    for trigger in mission::ids(doc, Kind::Trigger) {
        for (section, is_event) in [("Events", true), ("Actions", false)] {
            for row in mission::decode_list(&doc.ini_text(section, &trigger), is_event)? {
                let opcode = row[0].parse::<i32>()?;
                let schema = mission::schema::opcode(is_event, opcode).with_context(|| {
                    format!("{section}/{trigger} 含尚未审计的操作码 {opcode}，不能安全删除任务航点")
                })?;
                check_row(&row, schema, &format!("{section}/{trigger}"))?;
            }
        }
    }
    for script in mission::ids(doc, Kind::Script) {
        for (key, value) in doc.ini_fields(&script) {
            if key.parse::<u32>().is_err() {
                continue;
            }
            let row = mission::csv(&String::from_utf8(value)?);
            let opcode = row[0].parse::<i32>()?;
            let schema = mission::schema::scripts::opcode(opcode).with_context(|| {
                format!("{script}/{key} 含尚未审计的命令 {opcode}，不能安全删除任务航点")
            })?;
            check_row(&row, schema, &format!("{script}/{key}"))?;
        }
    }
    Ok(())
}

pub(super) fn write_base_plan(
    house: &str,
    options: &BasePlanOptions,
    doc: &MapDocument,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    ensure!(options.nodes.len() <= 10_000, "基地节点超过 10000 项");
    if mission::ids(doc, Kind::House).iter().any(|id| id == house) {
        let old_nodes = ra2_map_document::base_nodes::read(doc, house)?;
        for index in old_nodes.len()..options.nodes.len() {
            ensure!(
                doc.ini_value(house, &format!("{index:03}")).is_none(),
                "基地节点 {house}/{index:03} 位于原 NodeCount 之外，请先消歧"
            );
        }
    }
    if let Some(percent) = options.percent_built {
        ensure!((0..=100).contains(&percent), "PercentBuilt 必须为 0～100");
        put(house, "PercentBuilt", percent.to_string())?;
    }
    put(house, "NodeCount", options.nodes.len().to_string())?;
    for (index, node) in options.nodes.iter().enumerate() {
        let (value, x, y) = match node {
            BasePlanNode::Building { building, x, y, .. } => {
                catalog.resource_id(building, crate::catalog::ResourceKind::Building)?;
                let node = ra2_map_document::base_nodes::Node::new(&building.id, [*x, *y])?;
                (String::from_utf8(node.raw)?, *x, *y)
            }
            BasePlanNode::Control { control, x, y } => {
                ensure!(matches!(control, -3..=-1), "基地控制点仅支持 -1、-2、-3");
                (format!("{control},{x},{y}"), *x, *y)
            }
        };
        ensure!(
            doc.terrain([x, y]).is_some(),
            "基地节点坐标 ({x},{y}) 超出地图"
        );
        ensure!(
            value.len() < 128,
            "基地节点 {index} 超过原生 127 字节读取范围"
        );
        put(house, &format!("{index:03}"), value)?;
    }
    Ok(())
}
pub(super) fn write_house_presentation(
    house: &str,
    options: &HouseDefinition,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    if let Some(text) = &options.ui_name {
        let label = text.check("text")?;
        ensure!(
            !label.is_empty()
                && label.len() <= 31
                && label.is_ascii()
                && label.trim() == label
                && !label.contains([',', ';', '\r', '\n', '\0'])
                && catalog.strings.iter().any(|s| s == label),
            "House {house} 的 UIName 标签 {label:?} 不存在或超出 31 ASCII 字节范围"
        );
        put(house, "UIName", label.into())?;
    }
    if let Some(edge) = &options.edge {
        let name = match edge {
            HouseEdge::Automatic => "<none>",
            HouseEdge::North => "North",
            HouseEdge::East => "East",
            HouseEdge::South => "South",
            HouseEdge::West => "West",
            HouseEdge::Air => "Air",
        };
        put(house, "Edge", name.into())?;
    }
    Ok(())
}

pub(super) fn is_lighting_key(key: &str) -> bool {
    ra2_map_document::lighting::keys().iter().any(|k| k == key)
}

pub(super) fn write_lighting(
    options: &MissionLightingOptions,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let mut write = |key: &str, value: f64, scale: f64, bias: f64, interval: bool| {
        // ReadDouble scans f32 then widens. Check the actual consumer before
        // conversion, so overflow cannot silently saturate or wrap at ftol.
        let scanned = f64::from(value as f32);
        let units = (scanned * scale + bias).trunc();
        ensure!(
            value.is_finite()
                && scanned.is_finite()
                && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&units),
            "Lighting/{key} 超出原生有限数值范围"
        );
        if interval {
            ensure!(
                value > 0.0 && units >= 1.0,
                "Lighting/{key} 须产生至少 1 逻辑帧的间隔"
            );
        }
        if key == "NukeAmbientChangeRate" {
            ensure!(value.fract() == 0.0, "Lighting/{key} 须为正整数逻辑帧");
        }
        put("Lighting", key, value.to_string())
    };
    for (prefix, profile) in [
        ("", &options.normal),
        ("Ion", &options.ion),
        ("Dominator", &options.dominator),
    ] {
        if let Some(profile) = profile {
            for (component, value, scale) in [
                ("Ambient", profile.ambient, 100.0),
                ("Red", profile.red, 100.0),
                ("Green", profile.green, 100.0),
                ("Blue", profile.blue, 100.0),
                ("Ground", profile.ground, 1000.0),
                ("Level", profile.level, 1000.0),
            ] {
                if let Some(value) = value {
                    write(&format!("{prefix}{component}"), value, scale, 0.01, false)?;
                }
            }
        }
    }
    if let Some(value) = options.dominator_ambient_change_rate {
        write("DominatorAmbientChangeRate", value, 1000.0, 0.01, true)?;
    }
    if let Some(value) = options.nuke_ambient_change_rate {
        write("NukeAmbientChangeRate", value, 1.0, 0.0, true)?;
    }
    Ok(())
}

pub(super) fn is_special_flag_key(key: &str) -> bool {
    matches!(
        key,
        "Inert"
            | "FogOfWar"
            | "HarvesterImmune"
            | "DestroyableBridges"
            | "TiberiumGrows"
            | "TiberiumSpreads"
            | "InitialVeteran"
    )
}

pub(super) fn write_special_flags(
    options: &MissionSpecialFlagsOptions,
    mode: MissionMode,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    for (key, enabled) in [
        ("Inert", options.inert),
        ("FogOfWar", options.fog_of_war),
        ("HarvesterImmune", options.harvester_immune),
        ("DestroyableBridges", options.destroyable_bridges),
        ("TiberiumGrows", options.tiberium_grows),
        ("TiberiumSpreads", options.tiberium_spreads),
        ("InitialVeteran", options.initial_veteran),
    ] {
        if let Some(enabled) = enabled {
            ensure!(
                (key == "InitialVeteran") == (mode == MissionMode::Multiplayer),
                "SpecialFlags/{key} 不适用于当前任务模式"
            );
            put(
                "SpecialFlags",
                key,
                if enabled { "yes" } else { "no" }.into(),
            )?;
        }
    }
    Ok(())
}

pub(super) fn is_ranking_key(key: &str) -> bool {
    matches!(
        key,
        "ParTimeEasy"
            | "ParTimeMedium"
            | "ParTimeHard"
            | "UnderParTitle"
            | "UnderParMessage"
            | "OverParTitle"
            | "OverParMessage"
    )
}

pub(super) fn write_ranking(
    options: &MissionRankingOptions,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    if let Some(times) = &options.par_time {
        for (key, seconds) in [
            ("ParTimeEasy", times.easy),
            ("ParTimeMedium", times.normal),
            ("ParTimeHard", times.hard),
        ] {
            if let Some(seconds) = seconds {
                // Native Read_Time uses %02d:%02d:%02d and 60 ticks/second.
                ensure!(seconds <= 359_999, "Ranking/{key} 须为 0～359999 整数秒");
                put(
                    "Ranking",
                    key,
                    format!(
                        "{:02}:{:02}:{:02}",
                        seconds / 3600,
                        seconds / 60 % 60,
                        seconds % 60
                    ),
                )?;
            }
        }
    }
    for (key, text) in [
        ("UnderParTitle", &options.under_par_title),
        ("UnderParMessage", &options.under_par_message),
        ("OverParTitle", &options.over_par_title),
        ("OverParMessage", &options.over_par_message),
    ] {
        if let Some(text) = text {
            let label = text.check("text")?;
            ensure!(
                !label.is_empty()
                    && label.len() <= 30
                    && label.is_ascii()
                    && label.trim() == label
                    && !label.contains([',', ';', '\r', '\n', '\0'])
                    && catalog.strings.iter().any(|s| s == label),
                "Ranking/{key} 的 CSF 标签 {label:?} 不存在或超出 30 字节原生字段范围"
            );
            put("Ranking", key, label.into())?;
        }
    }
    Ok(())
}

pub(super) fn write_basic(
    options: &MissionBasicOptions,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    if let Some(name) = &options.name {
        ensure!(
            !name.trim().is_empty() && name.len() < 64,
            "Basic/Name 须为 1～63 字节"
        );
        put("Basic", "Name", name.clone())?;
    }
    if let Some(author) = &options.author {
        ensure!(author.len() < 128, "Basic/Author 超过 127 字节");
        put("Basic", "Author", author.clone())?;
    }
    for (key, file) in [
        ("NextScenario", &options.next_scenario),
        ("AltNextScenario", &options.alt_next_scenario),
    ] {
        if let Some(file) = file {
            ra2_map_document::campaign::filename(file)?;
            ensure!(
                file.to_ascii_lowercase().ends_with(".map"),
                "Basic/{key} 须为 .map 文件名"
            );
            put("Basic", key, file.clone())?;
        }
    }
    if let Some(percent) = options.carry_over_money_percent {
        ensure!(
            percent.is_finite() && (0.0..=100.0).contains(&percent),
            "资金继承比例须为 0～100 百分比"
        );
        put("Basic", "CarryOverMoney", format!("{percent}%"))?;
    }
    if let Some(cap) = options.carry_over_cap {
        ensure!(cap >= -1, "资金继承上限须为 -1 或非负整数");
        put("Basic", "CarryOverCap", cap.to_string())?;
    }
    for (key, reference, kind) in [
        (
            "Win",
            options.win.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "Lose",
            options.lose.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "Intro",
            options.intro.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "Brief",
            options.brief.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "Action",
            options.action.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "Theme",
            options.theme.as_ref(),
            crate::catalog::ResourceKind::Theme,
        ),
        (
            "PostScore",
            options.post_score.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
        (
            "PreMapSelect",
            options.pre_map_select.as_ref(),
            crate::catalog::ResourceKind::Movie,
        ),
    ] {
        if let Some(reference) = reference {
            catalog.resource_id(reference, kind)?;
            put("Basic", key, reference.id.clone())?;
        }
    }
    for (key, cell) in [
        ("HomeCell", options.home_cell.as_ref()),
        ("AltHomeCell", options.alt_home_cell.as_ref()),
    ] {
        if let Some(cell) = cell {
            waypoint(cell, catalog)?;
            put("Basic", key, cell.id.to_string())?;
        }
    }
    for (key, enabled) in [
        ("EndOfGame", options.end_of_game),
        ("OneTimeOnly", options.one_time_only),
        ("SkipScore", options.skip_score),
        ("IgnoreGlobalAITriggers", options.ignore_global_ai_triggers),
        ("FreeRadar", options.free_radar),
        ("FillSilos", options.fill_silos),
        ("TiberiumGrowthEnabled", options.tiberium_growth_enabled),
        ("IceGrowthEnabled", options.ice_growth_enabled),
        ("VeinGrowthEnabled", options.vein_growth_enabled),
        ("TrainCrate", options.train_crate),
        ("TruckCrate", options.truck_crate),
        ("TimerInherit", options.timer_inherit),
        ("SkipMapSelect", options.skip_map_select),
    ] {
        if let Some(enabled) = enabled {
            put("Basic", key, if enabled { "yes" } else { "no" }.into())?;
        }
    }
    Ok(())
}

pub(super) fn write_rebuilds(doc: &mut MapDocument, plan: &Mission) -> Result<Vec<ManagedRebuild>> {
    let mut managed = Vec::new();
    let mut objects = BTreeSet::new();
    for house in &plan.house_definitions {
        let Some(base) = &house.base_plan else {
            continue;
        };
        let house_id = if house.create {
            physical(&plan.id, &house.id, "HS")
        } else {
            house.id.clone()
        };
        for (index, node) in base.nodes.iter().enumerate() {
            let BasePlanNode::Building {
                building,
                x,
                y,
                rebuild: Some(rebuild),
            } = node
            else {
                continue;
            };
            let node = ra2_map_document::base_nodes::Node::new(&building.id, [*x, *y])?;
            let matches = ra2_map_document::base_nodes::matching_structures(doc, &house_id, &node);
            ensure!(
                matches.len() == 1,
                "基地节点 {house_id}/{index:03} 的 rebuild 须对应唯一的同所属方、同类型、同坐标建筑"
            );
            let record = doc
                .records()
                .remove(&matches[0])
                .context("基地建筑不存在")?;
            let key = format!("Structures:{}", record.key);
            ensure!(
                objects.insert(key.clone()),
                "基地建筑 {key} 重复设置 rebuild"
            );
            let before = record
                .fields()
                .get(8)
                .cloned()
                .context("基地建筑缺少 rebuild 字段")?;
            ra2_map_document::base_nodes::set_rebuild(doc, &house_id, index, *rebuild)?;
            managed.push(ManagedRebuild {
                object: key,
                type_id: record.type_id(),
                before,
                after: u8::from(*rebuild).to_string(),
            });
        }
    }
    Ok(managed)
}

pub(super) fn write_ai_triggers(
    plan: &Mission,
    teams: &BTreeMap<String, String>,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let mut ai_trigger_ids = BTreeSet::new();
    for definition in &plan.ai_triggers {
        identifier(&definition.id)?;
        ensure!(
            ai_trigger_ids.insert(definition.id.clone()),
            "重复 AI 触发器 {}",
            definition.id
        );
        ensure!(
            (-1..=7).contains(&definition.condition),
            "AI 触发器 {} 的 condition 须为 -1～7",
            definition.id
        );
        let id = physical(&plan.id, &definition.id, "AI");
        let mut fields = ra2_map_document::ai_triggers::decode(
            &ra2_map_document::ai_triggers::default_row(&definition.id, "<none>"),
        )?;
        fields[1] = named(teams, &definition.primary, "team")?;
        fields[2] = if let Some(owner) = &definition.owner {
            catalog.resource_id(owner, crate::catalog::ResourceKind::Country)?;
            owner.id.clone()
        } else {
            "<all>".into()
        };
        fields[4] = definition.condition.to_string();
        if let Some(object_type) = &definition.object_type {
            catalog.resource_id(object_type, crate::catalog::ResourceKind::Techno)?;
            fields[5] = object_type.id.clone();
        }
        fields[6] = ra2_map_document::ai_triggers::with_comparison(
            &fields[6],
            definition.amount.unwrap_or(0),
            definition.comparison.unwrap_or(0),
        )?;
        let weight = definition.weight.unwrap_or(40);
        let min_weight = definition.min_weight.unwrap_or(10);
        let max_weight = definition.max_weight.unwrap_or(60);
        ensure!(
            [weight, min_weight, max_weight]
                .into_iter()
                .all(|value| value <= i32::MAX as u32),
            "AI 权重须为 0～2147483647 整数"
        );
        ensure!(
            min_weight <= max_weight,
            "AI 触发器 {} 的 minWeight 大于 maxWeight",
            definition.id
        );
        fields[7] = weight.to_string();
        fields[8] = min_weight.to_string();
        fields[9] = max_weight.to_string();
        if let Some(multiplayer) = definition.multiplayer {
            fields[10] = i32::from(multiplayer).to_string();
        }
        let side = definition.side.unwrap_or(0);
        ensure!(side <= 3, "AI 触发器 {} 的 side 须为 0～3", definition.id);
        fields[12] = side.to_string();
        if let Some(secondary) = &definition.secondary {
            fields[14] = named(teams, secondary, "team")?;
        }
        if let Some(difficulties) = &definition.difficulties {
            let set: BTreeSet<_> = difficulties.iter().copied().collect();
            ensure!(
                set.len() == difficulties.len(),
                "AI 触发器 {} 的 difficulties 有重复项",
                definition.id
            );
            for (index, difficulty) in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
                .into_iter()
                .enumerate()
            {
                fields[15 + index] = i32::from(set.contains(&difficulty)).to_string();
            }
        }
        put("AITriggerTypes", &id, fields.join(","))?;
        // Noncampaign games enable every listed key, even `no`; a new map
        // definition stays disabled in either mode only when the key is absent.
        if definition.enabled.unwrap_or(true) {
            put("AITriggerTypesEnable", &id, "yes".into())?;
        }
    }
    Ok(())
}
