//! Rule references, lifecycle and native event-list semantics.
use super::*;

pub(super) fn action_house(
    house: &ActionHouse,
    mode: MissionMode,
    catalog: &Catalog,
) -> Result<String> {
    let index = match house {
        ActionHouse::Country { id } => catalog.resource_index(
            &Reference {
                kind: "country".into(),
                id: id.clone(),
            },
            crate::catalog::ResourceKind::Country,
        )?,
        ActionHouse::StartPlayer { id } => {
            ensure!(
                mode == MissionMode::Multiplayer
                    && *id < u32::from(MAX_PLAYERS)
                    && catalog.waypoints.contains(id),
                "动作玩家起点须为多人地图已有的 0～29 号航点"
            );
            // HouseClass::GetPlayerAtFromString510FB0, shared selectors117B..1198.
            0x117B + *id as i32
        }
        ActionHouse::RetainedHouse { id } => {
            ensure!(id == "event", "保留所属方引用无效");
            8997
        }
    };
    Ok(index.to_string())
}

pub(super) struct Graph<'a> {
    pub ids: BTreeMap<String, String>,
    pub roots: BTreeMap<&'a str, &'a str>,
}
impl<'a> Graph<'a> {
    pub fn new(plan: &'a Mission) -> Result<Self> {
        let mut ids = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for rule in &plan.rules {
            identifier(&rule.id)?;
            ensure!(
                ids.insert(rule.id.clone(), physical(&plan.id, &rule.id, "TR"))
                    .is_none(),
                "重复规则 {}",
                rule.id
            );
            definitions.insert(rule.id.as_str(), rule);
            ensure!(
                rule.difficulties.iter().collect::<BTreeSet<_>>().len() == rule.difficulties.len(),
                "规则 {} 的难度重复",
                rule.id
            );
        }
        let mut parents = BTreeMap::new();
        for rule in &plan.rules {
            if let Some(next) = &rule.linked {
                named(&ids, next, "rule")?;
                ensure!(
                    parents.insert(next.id.as_str(), rule.id.as_str()).is_none(),
                    "关联规则 {} 有多个上游，会创建重复活动实例",
                    next.id
                );
                ensure!(
                    rule.once == definitions[next.id.as_str()].once,
                    "关联规则的 once 必须一致：{} → {}",
                    rule.id,
                    next.id
                );
            }
        }
        let mut roots = BTreeMap::new();
        for rule in &plan.rules {
            let mut root = rule.id.as_str();
            let mut visited = BTreeSet::new();
            while let Some(parent) = parents.get(root) {
                ensure!(visited.insert(root), "规则关联存在循环：{}", rule.id);
                root = parent;
            }
            roots.insert(rule.id.as_str(), root);
        }
        // Force22 runs synchronously and is not protected by Tag reentrancy.
        // Reject recursive/exponential graphs instead of relying on runtime truncation.
        fn force_cost<'a>(
            id: &'a str,
            defs: &BTreeMap<&'a str, &'a Rule>,
            path: &mut BTreeSet<&'a str>,
            cache: &mut BTreeMap<&'a str, (usize, usize)>,
            external: &BTreeSet<&str>,
        ) -> Result<(usize, usize)> {
            if let Some(cost) = cache.get(id) {
                return Ok(*cost);
            }
            ensure!(path.insert(id), "强制触发存在递归：{id}");
            ensure!(path.len() <= 32, "强制触发嵌套超过 32 层：{id}");
            let rule = defs
                .get(id)
                .with_context(|| format!("未定义的 rule：{id}"))?;
            let mut depth = 1;
            let mut actions = rule.actions.len();
            for action in &rule.actions {
                let target = match action {
                    Action::EnableRule { rule }
                    | Action::DisableRule { rule }
                    | Action::ForceRule { rule } => Some(rule),
                    _ => None,
                };
                if let Some(target) = target {
                    let name = target.check("rule")?;
                    if external.contains(name) {
                        ensure!(
                            !matches!(action, Action::ForceRule { .. }),
                            "不能验证已有原生规则的递归预算，不支持 forceRule：{name}"
                        );
                        continue;
                    }
                    ensure!(defs.contains_key(name), "未定义的 rule：{name}");
                    if matches!(action, Action::ForceRule { .. }) {
                        let (child_depth, child_actions) =
                            force_cost(name, defs, path, cache, external)?;
                        depth = depth.max(child_depth + 1);
                        actions += child_actions;
                        ensure!(
                            depth <= 32 && actions <= 4096,
                            "强制触发链超过引擎深度/动作预算：{id}"
                        );
                    }
                }
            }
            path.remove(id);
            cache.insert(id, (depth, actions));
            Ok((depth, actions))
        }
        let mut cache = BTreeMap::new();
        let external = plan
            .native_definitions
            .iter()
            .filter(|d| d.kind == "rule")
            .map(|d| d.id.as_str())
            .collect();
        for rule in &plan.rules {
            force_cost(
                &rule.id,
                &definitions,
                &mut BTreeSet::new(),
                &mut cache,
                &external,
            )?;
        }
        Ok(Self { ids, roots })
    }
}

pub(super) fn variable(
    reference: &VariableReference,
    global: bool,
    doc: &MapDocument,
) -> Result<String> {
    let (kind, limit) = if global {
        ("globalFlag", 50)
    } else {
        ("localFlag", 100)
    };
    ensure!(
        reference.kind == kind && reference.id < limit,
        "{kind} 引用编号/类别无效：{}",
        reference.id
    );
    if !global {
        ensure!(
            doc.ini_value("VariableNames", &reference.id.to_string())
                .is_some(),
            "局部变量 {} 尚无 VariableNames 定义；新标记请使用 m.flag()",
            reference.id
        );
    }
    Ok(reference.id.to_string())
}

pub(super) struct Events<'a> {
    pub doc: &'a MapDocument,
    pub catalog: &'a Catalog,
    pub flags: &'a BTreeMap<String, String>,
    pub teams: &'a BTreeMap<String, String>,
    pub mode: MissionMode,
    pub house_ids: &'a BTreeMap<String, String>,
    pub house_indices: &'a BTreeMap<String, usize>,
    pub attachments: &'a mut Vec<Attachment>,
    pub bound: &'a mut BTreeMap<String, String>,
    pub tag: &'a str,
}
impl Events<'_> {
    pub fn encode(&mut self, event: &Event) -> Result<Vec<Vec<String>>> {
        let mut leaves = Vec::new();
        Self::flatten(event, 0, &mut leaves)?;
        ensure!(
            (1..=32).contains(&leaves.len()),
            "每条规则需要 1～32 个叶事件"
        );
        ensure!(
            leaves
                .iter()
                .filter(|e| matches!(e, Event::EnabledTime { .. } | Event::RandomTime { .. }))
                .count()
                <= 1,
            "原生 Trigger 只有一个独立计时器，all 中只能有一个 sinceEnabled/randomTime"
        );
        let objects: BTreeSet<_> = leaves
            .iter()
            .filter_map(|event| event_object(event).map(|object| object.id.as_str()))
            .collect();
        ensure!(
            objects.len() <= 1,
            "同一规则不能区分多个附着对象通知；请使用独立规则和标记"
        );
        leaves.into_iter().map(|event| self.leaf(event)).collect()
    }
    fn flatten<'a>(event: &'a Event, depth: usize, output: &mut Vec<&'a Event>) -> Result<()> {
        ensure!(
            depth <= 16 && output.len() < 32,
            "条件组合嵌套超过 16 层或事件超过 32 个"
        );
        if let Event::All { events } = event {
            ensure!(!events.is_empty(), "all 不能是空条件");
            for event in events {
                Self::flatten(event, depth + 1, output)?;
            }
        } else {
            output.push(event);
        }
        Ok(())
    }
    fn leaf(&mut self, event: &Event) -> Result<Vec<String>> {
        let (opcode, value) = match event {
            Event::DamagedByHouse { object, house } => {
                ensure!(
                    self.mode == MissionMode::SinglePlayer,
                    "指定伤害来源 House 仅支持单人索引投影"
                );
                self.bind_object(object, self.tag)?;
                let house = named(self.house_ids, house, "house")?;
                let index = self
                    .house_indices
                    .get(&house)
                    .context("伤害来源 House 未在地图 Houses 注册")?;
                (44, index.to_string())
            }
            Event::BuildingBuilt { building }
            | Event::BuildingExists { building }
            | Event::BuildingAbsent { building } => {
                let opcode = match event {
                    Event::BuildingBuilt { .. } => 19,
                    Event::BuildingExists { .. } => 32,
                    _ => 57,
                };
                (
                    opcode,
                    self.catalog
                        .resource_index(building, crate::catalog::ResourceKind::Building)?
                        .to_string(),
                )
            }
            Event::VehicleBuilt { r#type }
            | Event::InfantryBuilt { r#type }
            | Event::AircraftBuilt { r#type }
            | Event::SpyDisguisedAsInfantry { r#type } => {
                let (opcode, registry) = match event {
                    Event::VehicleBuilt { .. } => (20, "vehicles"),
                    Event::InfantryBuilt { .. } => (21, "infantry"),
                    Event::AircraftBuilt { .. } => (22, "aircraft"),
                    _ => (54, "infantry"),
                };
                let id = self
                    .catalog
                    .resource_id(r#type, crate::catalog::ResourceKind::Techno)?;
                let order = self
                    .catalog
                    .registry_order
                    .get(registry)
                    .with_context(|| format!("{registry} 缺少原生类型注册顺序"))?;
                let positions: Vec<_> = order
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| name.eq_ignore_ascii_case(id))
                    .map(|(index, _)| index)
                    .collect();
                ensure!(
                    positions.len() == 1,
                    "科技类型 {id} 不在 {registry} 原生数组或有歧义"
                );
                (opcode, i32::try_from(positions[0])?.to_string())
            }
            Event::RandomTime { seconds: s } => {
                ensure!(
                    *s <= 95_443_717,
                    "随机时间须为 0～95443717 游戏秒，避免计时帧溢出"
                );
                (51, s.to_string())
            }
            Event::CreditsAtLeast { amount } | Event::CreditsAtMost { amount } => {
                ensure!(*amount <= i32::MAX as u32, "资金阈值超出原生整数范围");
                (
                    if matches!(event, Event::CreditsAtLeast { .. }) {
                        12
                    } else {
                        52
                    },
                    amount.to_string(),
                )
            }
            Event::BuildingsLost { count } | Event::UnitsLost { count } => {
                ensure!(*count <= i32::MAX as u32, "损失数量超出原生整数范围");
                (
                    if matches!(event, Event::BuildingsLost { .. }) {
                        15
                    } else {
                        16
                    },
                    count.to_string(),
                )
            }
            Event::NoFactories => (17, "0".into()),
            Event::AmbientAtMost { value } => (45, value.to_string()),
            Event::AmbientAtLeast { value } => (46, value.to_string()),
            Event::AnyCratePickedUp => (50, "0".into()),
            Event::TeamLeftMap { team } => {
                return Ok(vec![
                    "23".into(),
                    "1".into(),
                    named(self.teams, team, "team")?,
                ]);
            }
            Event::CountryDiscovered { country }
            | Event::CountryUnitsDestroyed { country }
            | Event::CountryBuildingsDestroyed { country }
            | Event::CountryAllDestroyed { country }
            | Event::CountryNavyDestroyed { country }
            | Event::CountryLandDestroyed { country }
            | Event::LowPower { country }
            | Event::FullPower { country }
            | Event::CrossedHorizontal { country }
            | Event::CrossedVertical { country }
            | Event::EnteredOrOverflown { country }
            | Event::SpyDisguisedAsCountry { country } => {
                let opcode = match event {
                    Event::CountryDiscovered { .. } => 5,
                    Event::CountryUnitsDestroyed { .. } => 9,
                    Event::CountryBuildingsDestroyed { .. } => 10,
                    Event::CountryAllDestroyed { .. } => 11,
                    Event::CountryNavyDestroyed { .. } => 55,
                    Event::CountryLandDestroyed { .. } => 56,
                    Event::LowPower { .. } => 30,
                    Event::FullPower { .. } => 58,
                    Event::CrossedHorizontal { .. } => 25,
                    Event::CrossedVertical { .. } => 26,
                    Event::EnteredOrOverflown { .. } => 59,
                    _ => 53,
                };
                (
                    opcode,
                    self.catalog
                        .resource_index(country, crate::catalog::ResourceKind::Country)?
                        .to_string(),
                )
            }
            event
                if event_object(event).is_some()
                    && !matches!(event, Event::NearWaypoint { .. } | Event::Destroyed { .. }) =>
            {
                let object = event_object(event).unwrap();
                self.bind_object(object, self.tag)?;
                let opcode = match event {
                    Event::Discovered { .. } => 4,
                    Event::Attacked { .. } => 6,
                    Event::DestroyedAny { .. } => 29,
                    Event::Selected { .. } => {
                        ensure!(
                            self.mode == MissionMode::SinglePlayer,
                            "对象选择事件仅在单人模式发布通知"
                        );
                        33
                    }
                    Event::SpotlightDetected { .. } => 35,
                    Event::FirstDamagedBySource { .. } => 38,
                    Event::HalfHealthBySource { .. } => 39,
                    Event::RedHealthBySource { .. } => 40,
                    Event::FirstDamaged { .. } => 41,
                    Event::HalfHealth { .. } => 42,
                    Event::RedHealth { .. } => 43,
                    Event::DestroyedOrRemoved { .. } => 48,
                    Event::PickedUpCrate { .. } => 49,
                    _ => unreachable!(),
                };
                (opcode, "0".into())
            }
            Event::All { .. } => unreachable!("flattened"),
            Event::Any { .. } => anyhow::bail!("any 仅支持一次性规则的顶层 when，不能嵌入 all"),
            Event::ScenarioTime { seconds: s } => (47, seconds(*s)?),
            Event::EnabledTime { seconds: s } => (13, seconds(*s)?),
            Event::CountdownElapsed => (14, "0".into()),
            Event::Entered { country } | Event::EnteredZone { country } => (
                if matches!(event, Event::Entered { .. }) {
                    1
                } else {
                    24
                },
                self.catalog
                    .resource_index(country, crate::catalog::ResourceKind::Country)?
                    .to_string(),
            ),
            Event::TagNotification => (8, "0".into()),
            Event::BridgeDestroyed => (31, "0".into()),
            Event::NearWaypoint {
                object,
                waypoint: wp,
            } => {
                self.bind_object(object, self.tag)?;
                let (_, record, _) = super::object(self.doc, &object.id)?;
                ensure!(
                    matches!(record.section.as_str(), "Units" | "Infantry" | "Aircraft"),
                    "nearWaypoint 必须绑定移动对象"
                );
                waypoint(wp, self.catalog)?;
                (34, wp.id.to_string())
            }
            Event::TechnoExists { r#type, .. } | Event::TechnoAbsent { r#type } => {
                let type_id = self
                    .catalog
                    .resource_id(r#type, crate::catalog::ResourceKind::Techno)?;
                ensure!(
                    type_id.is_ascii()
                        && type_id.len() <= 24
                        && !type_id.contains([',', ';', '\r', '\n']),
                    "科技类型 ID 超出原生 24 字节范围：{type_id}"
                );
                let (opcode, count) = match event {
                    Event::TechnoExists { count, .. } => {
                        ensure!(
                            *count > 0 && *count <= i32::MAX as u32,
                            "科技类型数量须为 1～2147483647"
                        );
                        (60, *count)
                    }
                    _ => (61, 0),
                };
                return Ok(vec![
                    opcode.to_string(),
                    "2".into(),
                    count.to_string(),
                    type_id.into(),
                ]);
            }
            Event::FlagSet { flag } => (36, named(self.flags, flag, "flag")?),
            Event::FlagClear { flag } => (37, named(self.flags, flag, "flag")?),
            Event::LocalSet { flag } => (36, variable(flag, false, self.doc)?),
            Event::LocalClear { flag } => (37, variable(flag, false, self.doc)?),
            Event::GlobalSet { flag } => (27, variable(flag, true, self.doc)?),
            Event::GlobalClear { flag } => (28, variable(flag, true, self.doc)?),
            Event::Destroyed { object: reference } => {
                self.bind_object(reference, self.tag)?;
                (7, "0".into())
            }
            _ => unreachable!("object notification encoded above"),
        };
        Ok(vec![opcode.to_string(), "0".into(), value])
    }
    pub fn bind_object(&mut self, reference: &Reference, tag: &str) -> Result<()> {
        let name = reference.check("object")?;
        ensure!(
            self.catalog.objects.iter().any(|o| o == name),
            "对象 {name} 不存在"
        );
        if let Some(existing) = self.bound.get(name) {
            ensure!(
                existing == tag,
                "对象 {name} 已被其他规则绑定；请将规则关联到同一 Tag 链"
            );
        } else {
            let (_, record, column) = object(self.doc, name)?;
            let before = record
                .fields()
                .get(column)
                .cloned()
                .context("对象缺少 Tag 字段")?;
            ensure!(
                before.eq_ignore_ascii_case("none") || before.eq_ignore_ascii_case("<none>"),
                "对象 {name} 已有 Tag {before}，不能覆盖"
            );
            self.bound.insert(name.into(), tag.into());
            self.attachments.push(Attachment {
                object: name.into(),
                type_id: record.type_id(),
                before,
                after: tag.into(),
            });
        }
        Ok(())
    }
}

fn event_object(event: &Event) -> Option<&Reference> {
    match event {
        Event::DamagedByHouse { object, .. }
        | Event::Destroyed { object }
        | Event::NearWaypoint { object, .. }
        | Event::Discovered { object }
        | Event::Attacked { object }
        | Event::DestroyedAny { object }
        | Event::Selected { object }
        | Event::SpotlightDetected { object }
        | Event::FirstDamagedBySource { object }
        | Event::HalfHealthBySource { object }
        | Event::RedHealthBySource { object }
        | Event::FirstDamaged { object }
        | Event::HalfHealth { object }
        | Event::RedHealth { object }
        | Event::DestroyedOrRemoved { object }
        | Event::PickedUpCrate { object } => Some(object),
        _ => None,
    }
}
