//! Object property backed by ordinary, shared, one-shot map triggers.
use crate::{
    document::{MapDocument, Record, Target, Value},
    mission::{self, Kind},
};
use anyhow::{Context, Result, ensure};

pub fn tag_column(section: &str) -> Option<usize> {
    match section {
        "Structures" => Some(6),
        "Infantry" => Some(8),
        "Units" | "Aircraft" => Some(7),
        _ => None,
    }
}

fn header(slot: u8) -> String {
    format!("Neutral,<none>,Editor startup owner {slot},0,1,1,1,0")
}
fn action(slot: u8) -> String {
    // House510F60 selects the actual occupant of start A..H, including AI.
    format!("1,14,0,{},0,0,0,0,A", 4475 + u16::from(slot))
}
fn tag_value(id: &str, slot: u8) -> String {
    format!("0,Editor startup owner {slot},{id}")
}
fn managed_slot(doc: &MapDocument, tag: &str) -> Option<u8> {
    if !tag.starts_with("EDSTART") {
        return None;
    }
    (0..8).find(|&slot| {
        doc.ini_text("Tags", tag) == tag_value(tag, slot)
            && doc.ini_text("Triggers", tag) == header(slot)
            && doc.ini_text("Events", tag) == "1,13,0,0"
            && doc.ini_text("Actions", tag) == action(slot)
    })
}
pub fn get(doc: &MapDocument, record: &Record) -> Option<u8> {
    let column = tag_column(&record.section)?;
    managed_slot(doc, record.fields().get(column)?)
}
pub fn available(doc: &MapDocument, record: &Record) -> bool {
    let Some(column) = tag_column(&record.section) else {
        return false;
    };
    record.fields().get(column).is_some_and(|tag| {
        tag.eq_ignore_ascii_case("none")
            || tag.eq_ignore_ascii_case("<none>")
            || managed_slot(doc, tag).is_some()
    })
}

/// Leaves commit ownership to the caller, so the property and graph undo together.
/// Custom tags are never replaced. Only exact generated graphs are managed.
pub fn set(doc: &mut MapDocument, record_id: u64, slot: Option<u8>) -> Result<()> {
    ensure!(
        slot.is_none_or(|slot| slot < 8),
        "开局归属仅支持玩家起点 0～7"
    );
    let record = doc.records().remove(&record_id).context("对象不存在")?;
    let column = tag_column(&record.section).context("该对象不支持所属方")?;
    ensure!(
        available(doc, &record),
        "对象已有关联标签，请通过任务触发器设置开局归属"
    );
    let old_slot = get(doc, &record);
    if old_slot == slot {
        return Ok(());
    }
    if let Some(slot) = slot {
        ensure!(
            doc.records()
                .values()
                .any(|r| r.section == "Waypoints" && r.key == slot.to_string()),
            "航点 {slot} 不存在"
        );
    }
    let old_tag = record.fields()[column].clone();
    doc.atomic(|doc| {
        let tag = if let Some(slot) = slot {
            let existing = mission::ids(doc, Kind::Tag)
                .into_iter()
                .find(|tag| managed_slot(doc, tag) == Some(slot));
            if let Some(tag) = existing {
                tag
            } else {
                let tag = (1u32..)
                    .map(|n| format!("EDSTART{n:04}"))
                    .find(|id| {
                        Kind::ALL.into_iter().all(|kind| {
                            !mission::ids(doc, kind)
                                .iter()
                                .any(|old| old.eq_ignore_ascii_case(id))
                        }) && ["Events", "Actions"].into_iter().all(|section| {
                            !doc.ini_entries(section)
                                .iter()
                                .any(|(_, key, _)| key.eq_ignore_ascii_case(id))
                        }) && doc.ini_entries(id).is_empty()
                    })
                    .context("无法分配触发器 ID")?;
                for (section, value) in [
                    ("Triggers", header(slot)),
                    ("Events", "1,13,0,0".into()),
                    ("Actions", action(slot)),
                    ("Tags", tag_value(&tag, slot)),
                ] {
                    doc.set_ini(section, &tag, Some(value.into_bytes()))?;
                }
                tag
            }
        } else {
            "None".into()
        };
        let mut edits = vec![(column, tag)];
        if slot.is_some() {
            // Noncampaign FromName only sees participant names plus Neutral/Special.
            // A country ID such as Americans would fail before object construction.
            edits.push((0, "Neutral".into()));
        }
        doc.set(
            Target::Record(record_id),
            Some(Value::Record(record.patched(&edits)?)),
        )?;
        if old_slot.is_some() {
            let referenced = |doc: &MapDocument, kind| {
                mission::references(doc)
                    .iter()
                    .any(|r| r.to.0 == kind && r.to.1.eq_ignore_ascii_case(&old_tag))
            };
            if !referenced(doc, Kind::Tag) {
                mission::delete(doc, Kind::Tag, &old_tag)?;
                if !referenced(doc, Kind::Trigger) {
                    mission::delete(doc, Kind::Trigger, &old_tag)?;
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
