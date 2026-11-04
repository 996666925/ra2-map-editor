//! Shared and inline TeamType dependencies use the same validation and encoding.
use super::*;

/// Script17 uses the process array. Project the actual final document layout:
/// pending manual keys and generated keys can interleave when serialized.
pub(super) fn script_indices(
    doc: &MapDocument,
    plan: &Mission,
    catalog: &Catalog,
) -> Result<BTreeMap<String, usize>> {
    let Some(fixed) = &catalog.fixed_scripts else {
        return Ok(BTreeMap::new());
    };
    let generated: BTreeSet<_> = plan
        .scripts
        .iter()
        .map(|s| physical(&plan.id, &s.id, "DS"))
        .chain(
            plan.teams
                .iter()
                .filter(|t| t.orders.is_some())
                .map(|t| physical(&plan.id, &t.id, "SC")),
        )
        .collect();
    let mut projected = doc.fork();
    for id in generated {
        ensure!(
            projected.ini_value("ScriptTypes", &id).is_none()
                && projected.ini_entries(&id).is_empty(),
            "生成脚本 {id} 与手工定义冲突"
        );
        projected.set_ini("ScriptTypes", &id, Some(id.as_bytes().to_vec()))?;
        projected.set_ini(&id, "Name", Some(b"projection".to_vec()))?;
    }
    // TeamType ReadINI allocates Script references before the explicit lists
    // (6F1496, then ScriptTypes 691970). Project the final physical TeamTypes
    // layout as well, so unrelated new teams cannot shift Script17's operand.
    let mut shared = references::existing(doc, plan, catalog, "script")?;
    for script in &plan.scripts {
        shared.insert(script.id.clone(), physical(&plan.id, &script.id, "DS"));
    }
    for team in &plan.teams {
        let id = physical(&plan.id, &team.id, "TM");
        let script = match (&team.orders, &team.script) {
            (Some(_), None) => physical(&plan.id, &team.id, "SC"),
            (None, Some(reference)) => named(&shared, reference, "script")?,
            _ => anyhow::bail!("小队 {} 必须且只能选择 orders 或 script", team.id),
        };
        projected.set_ini("TeamTypes", &id, Some(id.as_bytes().to_vec()))?;
        projected.set_ini(&id, "Script", Some(script.into_bytes()))?;
    }
    crate_lottery::project_scripts(plan, &mut projected)?;
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&projected.snapshot().bytes()?)?;
    let local = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    let mut indices = BTreeMap::new();
    let mut canonical = BTreeSet::new();
    for id in catalog
        .fixed_team_scripts
        .iter()
        .cloned()
        .chain(
            local
                .team_types
                .iter()
                .filter_map(|t| t.section().read_name("Script", 0x80).map(str::to_owned)),
        )
        .chain(fixed.iter().cloned())
        .chain(local.scripts.into_iter().map(|s| s.id))
    {
        if id.is_empty() || id.eq_ignore_ascii_case("none") || id.eq_ignore_ascii_case("<none>") {
            continue;
        }
        if canonical.insert(id.to_ascii_uppercase()) {
            indices.insert(id, indices.len());
        }
    }
    Ok(indices)
}

pub(super) fn task_force(
    id: &str,
    name: &str,
    units: &[Unit],
    group: i32,
    catalog: &Catalog,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    ensure!(
        (1..=6).contains(&units.len()),
        "特遣部队 {name} 需要 1～6 种成员"
    );
    put("TaskForces", id, id.into())?;
    put(id, "Name", name.into())?;
    put(id, "Group", group.to_string())?;
    let mut types = BTreeSet::new();
    for (index, unit) in units.iter().enumerate() {
        let type_id = unit.r#type.check("unit")?;
        ensure!(
            catalog.units.iter().any(|u| u == type_id)
                && types.insert(type_id)
                && unit.count > 0
                && unit.count <= i32::MAX as u32,
            "特遣部队 {name} 的成员 {type_id} 不存在、重复或数量无效"
        );
        put(id, &index.to_string(), format!("{},{type_id}", unit.count))?;
    }
    Ok(())
}

pub(super) struct ScriptContext<'a> {
    pub doc: &'a MapDocument,
    pub catalog: &'a Catalog,
    pub mode: MissionMode,
    pub flags: &'a BTreeMap<String, String>,
    pub scripts: &'a BTreeMap<String, String>,
    pub script_indices: &'a BTreeMap<String, usize>,
}
pub(super) fn script(
    id: &str,
    name: &str,
    orders: &[Order],
    context: &ScriptContext<'_>,
    put: &mut impl FnMut(&str, &str, String) -> Result<()>,
) -> Result<()> {
    let catalog = context.catalog;
    ensure!(
        orders.len() <= 256,
        "行为脚本 {name} 的标签/命令总数超过 256"
    );
    let mut labels = BTreeMap::new();
    let mut count = 0;
    for order in orders {
        if let Order::Label { label } = order {
            identifier(label)?;
            ensure!(
                labels.insert(label.as_str(), count + 1).is_none(),
                "脚本 {name} 重复标签 {label}"
            );
        } else {
            count += 1;
        }
    }
    ensure!(
        (1..=50).contains(&count),
        "行为脚本 {name} 需要 1～50 行行为"
    );
    put("ScriptTypes", id, id.into())?;
    put(id, "Name", name.into())?;
    ensure!(
        labels.values().all(|line| *line <= count),
        "脚本 {name} 的标签必须指向其后的实际命令，不能位于末尾"
    );
    for (index, order) in orders
        .iter()
        .filter(|order| !matches!(order, Order::Label { .. }))
        .enumerate()
    {
        let (opcode, operand) = match order {
            Order::FollowFriendlies => (10, "0".into()),
            Order::IdleAnimation { animation } => {
                ensure!(*animation <= 1, "步兵 idle 变体须为 0 或 1");
                (13, animation.to_string())
            }
            Order::Scout => {
                ensure!(
                    context.mode == MissionMode::SinglePlayer,
                    "Script22 探索迷雾仅支持单人路径"
                );
                (22, "0".into())
            }
            Order::FireSale => (30, "0".into()),
            Order::CenterCamera { velocity } => {
                ensure!((0..=4).contains(velocity), "镜头速度须为 0～4 原生档位");
                (34, velocity.to_string())
            }
            Order::AttackEnemyBuilding { building, mode }
            | Order::MoveToEnemyBuilding { building, mode }
            | Order::MoveToOwnBuilding { building, mode }
            | Order::ChronoshiftToBuilding { building, mode } => {
                ensure!(*mode <= 3, "建筑选择模式须为 0～3");
                let index =
                    catalog.resource_index(building, crate::catalog::ResourceKind::Building)?;
                ensure!(index <= 65535, "建筑类型序号超出打包字段范围");
                let opcode = match order {
                    Order::AttackEnemyBuilding { .. } => 46,
                    Order::MoveToEnemyBuilding { .. } => 47,
                    Order::MoveToOwnBuilding { .. } => 58,
                    _ => 56,
                };
                (opcode, ((i32::from(*mode) << 16) | index).to_string())
            }
            Order::PlayAnimation { animation, loops } => {
                let index =
                    catalog.resource_index(animation, crate::catalog::ResourceKind::Animation)?;
                ensure!(index <= 65535, "动画注册序号超出打包字段范围");
                (
                    51,
                    (((u32::from(*loops) << 16) | index as u32) as i32).to_string(),
                )
            }
            // The pinned TeamClass VM's action 52 (0x006E9D42..0x006E9D5B)
            // uses the executing team's first member and advances immediately.
            Order::TalkBubble { bubble } => {
                ensure!(*bubble <= 3, "对话气泡须为 0～3");
                (52, bubble.to_string())
            }
            Order::GatherAtEnemyBase => (53, "0".into()),
            Order::RegroupAtBase => (54, "0".into()),
            Order::IronCurtain => (55, "0".into()),
            Order::ChronoshiftToQuarry { quarry } => {
                ensure!(*quarry <= 11, "Quarry 须为 0～11");
                (57, quarry.to_string())
            }
            Order::AttackWaypointBuilding { waypoint: wp } => {
                waypoint(wp, catalog)?;
                (59, wp.id.to_string())
            }
            Order::EnterGrinder => (60, "0".into()),
            Order::EnterTankBunker => (61, "0".into()),
            Order::EnterBioReactor => (62, "0".into()),
            Order::EnterBattleBunker => (63, "0".into()),
            Order::EnterNearestOccupiable => (64, "0".into()),
            Order::ChangeCountry { country } => (
                20,
                catalog
                    .resource_index(country, crate::catalog::ResourceKind::Country)?
                    .to_string(),
            ),
            Order::ChangeScript { script } => {
                let target = named(context.scripts, script, "script")?;
                let index = context
                    .script_indices
                    .get(&target)
                    .context("缺少固定 AIMD 脚本注册顺序，不能编码 changeScript")?;
                (17, index.to_string())
            }
            Order::Label { .. } => unreachable!("labels do not occupy native lines"),
            Order::Jump { label } => (
                6,
                labels
                    .get(label.as_str())
                    .with_context(|| format!("脚本 {name} 未定义标签 {label}"))?
                    .to_string(),
            ),
            Order::Attack { quarry } => {
                ensure!(*quarry <= 11, "Quarry 须为 0～11");
                (0, quarry.to_string())
            }
            Order::MoveToCell { x, y } => {
                context.doc.check_cell([*x, *y])?;
                ensure!(
                    *x < 128 && context.doc.terrain([*x, *y]).is_some(),
                    "小队格子命令须使用 x<128 的有效地图格"
                );
                (4, (u32::from(*y) * 128 + u32::from(*x)).to_string())
            }
            Order::Mission { mission } => {
                ensure!(*mission <= 31, "Mission 须为 0～31");
                (11, mission.to_string())
            }
            Order::Face { direction } => {
                ensure!(*direction <= 7, "方向须为 0～7");
                (42, direction.to_string())
            }
            Order::Unload { mode } => {
                ensure!(*mode <= 3, "卸载模式须为 0～3");
                (8, mode.to_string())
            }
            Order::SpyWaypoint { waypoint: wp } => {
                waypoint(wp, catalog)?;
                (15, wp.id.to_string())
            }
            Order::WaitForever => (2, "0".into()),
            Order::Deploy => (9, "0".into()),
            Order::Scatter => (21, "0".into()),
            Order::Panic => (19, "0".into()),
            Order::Calm => (41, "0".into()),
            Order::Load => (14, "0".into()),
            Order::WaitLoaded => (43, "0".into()),
            Order::Next => (24, "0".into()),
            Order::Succeed => (49, "0".into()),
            Order::KillMembers => (31, "0".into()),
            Order::RemoveMembers => (37, "0".into()),
            Order::WinPlayer | Order::LosePlayer => {
                ensure!(
                    context.mode == MissionMode::SinglePlayer,
                    "脚本 winPlayer/losePlayer 仅支持单人任务"
                );
                (
                    if matches!(order, Order::WinPlayer) {
                        7
                    } else {
                        23
                    },
                    "0".into(),
                )
            }
            Order::SetFlag { flag } => (39, named(context.flags, flag, "flag")?),
            Order::ClearFlag { flag } => (40, named(context.flags, flag, "flag")?),
            Order::SetLocal { flag } => (39, rules::variable(flag, false, context.doc)?),
            Order::ClearLocal { flag } => (40, rules::variable(flag, false, context.doc)?),
            Order::SetGlobal { flag } => (12, rules::variable(flag, true, context.doc)?),
            Order::ClearGlobal { flag } => (38, rules::variable(flag, true, context.doc)?),
            Order::PlaySound { sound } => (
                25,
                catalog
                    .resource_index(sound, crate::catalog::ResourceKind::Sound)?
                    .to_string(),
            ),
            Order::PlayMovie { movie } => (
                26,
                catalog
                    .resource_index(movie, crate::catalog::ResourceKind::Movie)?
                    .to_string(),
            ),
            Order::PlayMusic { theme } => (
                27,
                catalog
                    .resource_index(theme, crate::catalog::ResourceKind::Theme)?
                    .to_string(),
            ),
            Order::Flash { frames } => (50, frames.to_string()),
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
        put(id, &index.to_string(), format!("{opcode},{operand}"))?;
    }
    Ok(())
}
