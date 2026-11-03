//! References never take ownership of an existing native definition.
use super::*;

pub(super) fn existing(
    doc: &MapDocument,
    plan: &Mission,
    catalog: &Catalog,
    kind: &str,
) -> Result<BTreeMap<String, String>> {
    ensure!(plan.native_definitions.len() <= 1024, "原生定义引用过多");
    let mut result = BTreeMap::new();
    let mut used = BTreeSet::new();
    for reference in &plan.native_definitions {
        identifier(&reference.id)?;
        ensure!(
            matches!(
                reference.kind.as_str(),
                "team" | "script" | "taskForce" | "rule" | "tag"
            ),
            "未知原生引用类别"
        );
        ensure!(
            used.insert((&reference.kind, &reference.id)),
            "重复原生定义引用"
        );
        if reference.kind != kind {
            continue;
        }
        let (registry, generated) = match kind {
            "team" => (Kind::Team, plan.teams.iter().any(|x| x.id == reference.id)),
            "script" => (
                Kind::Script,
                plan.scripts.iter().any(|x| x.id == reference.id),
            ),
            "taskForce" => (
                Kind::TaskForce,
                plan.task_forces.iter().any(|x| x.id == reference.id),
            ),
            "rule" => (
                Kind::Trigger,
                plan.rules.iter().any(|x| x.id == reference.id),
            ),
            "tag" => (Kind::Tag, plan.tags.iter().any(|x| x.id == reference.id)),
            _ => unreachable!(),
        };
        ensure!(!generated, "原生引用与任务定义重名：{}", reference.id);
        let local = mission::ids(doc, registry);
        let matches: Vec<_> = local
            .iter()
            .chain(catalog.native_definitions.get(kind).into_iter().flatten())
            .filter(|id| id.eq_ignore_ascii_case(&reference.native))
            .collect();
        let names: BTreeSet<_> = matches.into_iter().collect();
        ensure!(
            names.len() == 1,
            "原生 {kind} {} 不存在或存在大小写歧义",
            reference.native
        );
        result.insert(
            reference.id.clone(),
            names.into_iter().next().unwrap().clone(),
        );
    }
    Ok(result)
}

pub(super) fn aliases(
    doc: &MapDocument,
    plan: &Mission,
    old: &[ManagedAlias],
) -> Result<Vec<ManagedAlias>> {
    ensure!(plan.aliases.len() <= 2048, "别名超过容量");
    let mut ids = BTreeSet::new();
    let mut result = Vec::new();
    for alias in &plan.aliases {
        identifier(&alias.id)?;
        ensure!(
            ids.insert((&alias.kind, &alias.id)),
            "重复逻辑别名：{}",
            alias.id
        );
        let value = match alias.kind.as_str() {
            "object" => {
                let (_, record, _) = object(doc, &alias.target)?;
                record.fields().join(",")
            }
            "waypoint" => {
                let n = alias.target.parse::<u32>().context("航点别名须为数字")?;
                let records: Vec<_> = doc
                    .records()
                    .into_values()
                    .filter(|r| r.section == "Waypoints" && r.key.parse::<u32>() == Ok(n))
                    .collect();
                ensure!(records.len() == 1, "航点别名不存在或有歧义");
                records[0].fields().join(",")
            }
            _ => anyhow::bail!("别名仅支持 object/waypoint"),
        };
        if let Some(previous) = old
            .iter()
            .find(|p| p.kind == alias.kind && p.id == alias.id && p.target == alias.target)
        {
            ensure!(
                previous.value == value,
                "别名 {} 的原始记录已变化或重排，请重新选择目标并更换别名 ID",
                alias.id
            );
        }
        result.push(ManagedAlias {
            kind: alias.kind.clone(),
            id: alias.id.clone(),
            target: alias.target.clone(),
            value,
        });
    }
    Ok(result)
}

pub(super) fn mappings(plan: &Mission) -> Result<Vec<SourceMapping>> {
    ensure!(plan.origins.len() <= 4096, "源码来源条目过多");
    let mut origins = BTreeMap::new();
    for origin in &plan.origins {
        ensure!(
            origin.module.len() <= 1024
                && !origin.module.contains(['\0', '\r', '\n'])
                && origins
                    .insert(
                        (origin.kind.clone(), origin.id.clone()),
                        origin.module.clone()
                    )
                    .is_none(),
            "源码来源无效或重复"
        );
    }
    let mut result = Vec::new();
    let mut add = |kind: &str, id: &str, prefix: &str| {
        result.push(SourceMapping {
            kind: kind.into(),
            logical: id.into(),
            native: physical(&plan.id, id, prefix),
            module: origins.remove(&(kind.to_owned(), id.to_owned())),
        });
    };
    for x in &plan.rules {
        add("rule", &x.id, "TR");
    }
    for x in &plan.teams {
        add("team", &x.id, "TM");
    }
    for x in &plan.scripts {
        add("script", &x.id, "DS");
    }
    for x in &plan.task_forces {
        add("taskForce", &x.id, "FT");
    }
    for x in &plan.tags {
        add("tag", &x.id, "ET");
    }
    ensure!(origins.is_empty(), "源码来源引用未定义的项目");
    Ok(result)
}
