//! Copy/rename known mission edges using byte-preserving, atomic transactions.
use super::*;
use crate::document::{Target, Value};
use std::collections::BTreeMap;

type Names = BTreeMap<(Kind, String), String>;
fn supported(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Trigger | Kind::Tag | Kind::Team | Kind::Script | Kind::TaskForce | Kind::AiTrigger
    )
}
fn available(doc: &MapDocument, kind: Kind, id: &str) -> Result<()> {
    identifier(id)?;
    ensure!(
        id.len() >= 3,
        "任务定义 ID 至少 3 字节，避免被原生解释为注册序号"
    );
    ensure!(
        !Kind::ALL
            .into_iter()
            .any(|k| ids(doc, k).iter().any(|x| x.eq_ignore_ascii_case(id)))
            && doc.ini_entries(id).is_empty(),
        "目标 ID 或 section 已存在"
    );
    ensure!(supported(kind), "该记录暂不支持复制或重命名");
    Ok(())
}
fn preflight(doc: &MapDocument) -> Result<()> {
    for kind in [
        Kind::Trigger,
        Kind::Tag,
        Kind::Team,
        Kind::Script,
        Kind::TaskForce,
        Kind::AiTrigger,
    ] {
        let mut seen = BTreeSet::new();
        for (_, key, value) in doc.ini_entries(kind.registry()) {
            let id = if kind.section_definition() {
                std::str::from_utf8(&value)?.trim().to_owned()
            } else {
                key
            };
            ensure!(
                seen.insert(id.to_ascii_uppercase()),
                "存在重复或大小写冲突的定义，先消歧"
            );
            if kind.section_definition() {
                let entries = doc.ini_entries(&id);
                let keys: BTreeSet<_> = entries.iter().map(|r| &r.1).collect();
                ensure!(keys.len() == entries.len(), "定义含重复字段，先消歧");
            }
        }
    }
    let mut sections = vec![
        "Triggers".to_owned(),
        "Events".into(),
        "Actions".into(),
        "Tags".into(),
        "CellTags".into(),
        "AITriggerTypes".into(),
        "AITriggerTypesEnable".into(),
    ];
    for kind in [Kind::Team, Kind::Script, Kind::TaskForce] {
        sections.extend(ids(doc, kind));
    }
    for section in sections {
        let bodies = doc
            .base
            .raw
            .lines
            .iter()
            .filter(|line| {
                line.section == section
                    && doc.base.raw.bytes[line.bytes.clone()]
                        .trim_ascii_start()
                        .starts_with(b"[")
            })
            .count();
        ensure!(bodies <= 1, "存在重复 section {section}，请先消歧");
        let entries = doc.ini_entries(&section);
        let keys: BTreeSet<_> = entries.iter().map(|r| r.1.as_str()).collect();
        ensure!(
            keys.len() == entries.len(),
            "存在重复 key {section}，请先消歧"
        );
    }
    for id in ids(doc, Kind::Trigger) {
        for (section, event) in [("Events", true), ("Actions", false)] {
            let rows = decode_list(&doc.ini_text(section, &id), event)
                .with_context(|| format!("{section}/{id} 不能安全解析"))?;
            for row in rows {
                let reference = if event {
                    row[1] == "1"
                } else {
                    matches!(row[1].as_str(), "1" | "2" | "3" | "5")
                };
                ensure!(
                    !reference || row[2].len() >= 3 || matches!(row[2].as_str(), "-1"),
                    "存在短引用/注册序号；定义排序变化可能改变其目标，请先改用显式 ID"
                );
            }
        }
    }
    // Changing ScriptTypes can shift Script17's ordinal, including map overrides.
    for id in ids(doc, Kind::Script) {
        ensure!(
            !fields(doc, Kind::Script, &id)
                .iter()
                .any(|(k, v)| k.parse::<usize>().is_ok()
                    && v.split(|b| *b == b',')
                        .next()
                        .is_some_and(|s| s.trim_ascii() == b"17")),
            "存在脚本切换注册序号（动作 17），请先核实并迁移为稳定图后操作"
        );
    }
    Ok(())
}
fn lookup<'a>(names: &'a Names, kind: Kind, raw: &[u8]) -> Option<&'a str> {
    let value = std::str::from_utf8(raw).ok()?.trim();
    names
        .iter()
        .find(|((k, id), _)| *k == kind && same_id(kind, id, value))
        .map(|(_, v)| v.as_str())
}
fn column(bytes: &mut Vec<u8>, index: usize, kind: Kind, names: &Names) {
    let mut fields: Vec<_> = bytes.split(|b| *b == b',').map(<[u8]>::to_vec).collect();
    if let Some(f) = fields.get_mut(index)
        && let Some(new) = lookup(names, kind, f)
    {
        *f = new.as_bytes().to_vec();
        *bytes = fields.join(&b',');
    }
}
fn rewrite(
    section: &str,
    key: &str,
    bytes: &mut Vec<u8>,
    names: &Names,
    kind: Option<Kind>,
) -> Result<()> {
    match section {
        "AITriggerTypes" => {
            crate::ai_triggers::decode(&String::from_utf8_lossy(bytes))
                .context("AITriggerTypes 含未知格式，不能安全迁移小队引用")?;
            column(bytes, 1, Kind::Team, names);
            column(bytes, 14, Kind::Team, names);
        }
        "Triggers" => column(bytes, 1, Kind::Trigger, names),
        "Tags" => column(bytes, 2, Kind::Trigger, names),
        "CellTags" => {
            if let Some(new) = lookup(names, Kind::Tag, bytes) {
                *bytes = new.as_bytes().to_vec();
            }
        }
        "Events" | "Actions" => {
            let event = section == "Events";
            let rows = decode_list(&String::from_utf8_lossy(bytes), event)?;
            let mut cursor = 1;
            for row in rows {
                let target = if event {
                    if row[1] == "1" {
                        Some(Kind::Team)
                    } else {
                        None
                    }
                } else {
                    match row[1].as_str() {
                        "1" | "5" => Some(Kind::Team),
                        "2" => Some(Kind::Trigger),
                        "3" => Some(Kind::Tag),
                        _ => None,
                    }
                };
                if let Some(target) = target
                    && row[2].len() >= 3
                {
                    column(bytes, cursor + 2, target, names);
                }
                cursor += row.len();
            }
        }
        _ => {
            if kind == Some(Kind::Team) {
                let target = match key {
                    "Script" => Some(Kind::Script),
                    "TaskForce" => Some(Kind::TaskForce),
                    "Tag" => Some(Kind::Tag),
                    _ => None,
                };
                if let Some(target) = target
                    && let Some(new) = lookup(names, target, bytes)
                {
                    *bytes = new.as_bytes().to_vec();
                }
            }
        }
    }
    Ok(())
}
fn copy_one(doc: &mut MapDocument, kind: Kind, id: &str, new: &str, names: &Names) -> Result<()> {
    if kind.section_definition() {
        register(doc, kind, new)?;
        for (_, key, mut bytes) in doc.ini_entries(id) {
            rewrite(id, &key, &mut bytes, names, Some(kind))?;
            doc.set_ini(new, &key, Some(bytes))?;
        }
    } else {
        for section in if kind == Kind::Trigger {
            vec!["Triggers", "Events", "Actions"]
        } else if kind == Kind::AiTrigger {
            vec!["AITriggerTypes", "AITriggerTypesEnable"]
        } else {
            vec![kind.registry()]
        } {
            let Some(mut bytes) = doc.ini_value(section, id) else {
                ensure!(section == "AITriggerTypesEnable", "源定义缺失");
                continue;
            };
            rewrite(section, id, &mut bytes, names, Some(kind))?;
            doc.set_ini(section, new, Some(bytes))?;
        }
    }
    Ok(())
}
/// Clone the outgoing local graph (and a trigger's tags); installation definitions
/// remain shared. Existing map cells/objects continue using the original graph.
pub fn copy(
    doc: &mut MapDocument,
    kind: Kind,
    id: &str,
    new: &str,
    dependencies: bool,
) -> Result<Vec<(Kind, String, String)>> {
    available(doc, kind, new)?;
    preflight(doc)?;
    ensure!(ids(doc, kind).iter().any(|x| x == id), "源记录不存在");
    doc.atomic(|doc| {
        let mut names = Names::from([((kind, id.to_owned()), new.to_owned())]);
        let mut queue = vec![(kind, id.to_owned())];
        let references = references(doc);
        while let Some(current) = queue.pop() {
            let targets = references
                .iter()
                .filter_map(|r| {
                    if dependencies && r.from == current {
                        Some(r.to.clone())
                    } else if current.0 == Kind::Trigger && r.to == current && r.from.0 == Kind::Tag
                    {
                        Some(r.from.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for (k, value) in targets {
                if !supported(k) {
                    continue;
                }
                let Some(canonical) = ids(doc, k).into_iter().find(|x| same_id(k, x, &value))
                else {
                    continue;
                };
                let target = (k, canonical);
                if names.contains_key(&target) {
                    continue;
                }
                let prefix = next_id(doc, k);
                let next = (0u32..)
                    .map(|n| {
                        if n == 0 {
                            prefix.clone()
                        } else {
                            format!("{prefix}_{n}")
                        }
                    })
                    .find(|candidate| {
                        !names.values().any(|x| x.eq_ignore_ascii_case(candidate))
                            && available(doc, k, candidate).is_ok()
                    })
                    .context("无法分配依赖 ID")?;
                names.insert(target.clone(), next);
                queue.push(target);
            }
        }
        for ((k, old), new) in &names {
            copy_one(doc, *k, old, new, &names)?;
        }
        Ok(names
            .into_iter()
            .map(|((k, old), new)| (k, old, new))
            .collect())
    })
}
/// Migrate all known inbound references and preserve opaque CSV bytes.
pub fn rename(doc: &mut MapDocument, kind: Kind, id: &str, new: &str) -> Result<()> {
    available(doc, kind, new)?;
    preflight(doc)?;
    ensure!(ids(doc, kind).iter().any(|x| x == id), "源记录不存在");
    doc.atomic(|doc| {
        let names = Names::from([((kind, id.to_owned()), new.to_owned())]);
        copy_one(doc, kind, id, new, &names)?;
        for k in [
            Kind::Trigger,
            Kind::Tag,
            Kind::CellTag,
            Kind::Team,
            Kind::AiTrigger,
        ] {
            for current in ids(doc, k) {
                for (field, bytes) in fields(doc, k, &current) {
                    let section = if k.section_definition() {
                        current.as_str()
                    } else {
                        field.as_str()
                    };
                    let key = if k.section_definition() {
                        field.as_str()
                    } else {
                        current.as_str()
                    };
                    let mut updated = bytes.clone();
                    rewrite(section, key, &mut updated, &names, Some(k))?;
                    if updated != bytes {
                        doc.set_ini(section, key, Some(updated))?;
                    }
                }
            }
        }
        for (record_id, record) in doc.records() {
            let col = match record.section.as_str() {
                "Structures" => 6,
                "Infantry" => 8,
                "Units" | "Aircraft" => 7,
                _ => continue,
            };
            let mut value = record.value.clone();
            column(&mut value, col, Kind::Tag, &names);
            if value != record.value {
                let mut r = record.clone();
                r.value = value;
                doc.set(Target::Record(record_id), Some(Value::Record(r)))?;
            }
        }
        if kind.section_definition() {
            // Reuse the original registry slot so existing ordinal references to
            // Team/Script/TaskForce retain their order after a rename.
            let slots: Vec<_> = doc
                .ini_entries(kind.registry())
                .into_iter()
                .filter(|(_, _, v)| v == id.as_bytes() || v == new.as_bytes())
                .collect();
            for (_, key, v) in slots {
                if v == id.as_bytes() {
                    doc.set_ini(kind.registry(), &key, Some(new.as_bytes().to_vec()))?;
                } else {
                    doc.set_ini(kind.registry(), &key, None)?;
                }
            }
            for (_, key, _) in doc.ini_entries(id) {
                doc.set_ini(id, &key, None)?;
            }
        } else {
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
