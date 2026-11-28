//! Original YR deployment using a one-shot native mission graph.
use crate::{
    document::{MapDocument, Record, Target},
    mission::{self, Kind},
    startup_owner,
};
use anyhow::{Context, Result, ensure};

pub const MAX_COUNT: u16 = 255;

fn valid_infantry_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn put(doc: &mut MapDocument, section: &str, key: &str, value: &str) -> Result<()> {
    doc.set_ini(section, key, Some(value.as_bytes().to_vec()))
}

pub fn infantry(doc: &MapDocument, record: &Record) -> Option<String> {
    deployment(doc, record).map(|deployment| deployment.infantry)
}

pub fn has_legacy_script(doc: &MapDocument, record: &Record) -> bool {
    legacy_script(doc, record).is_some()
}

const SECTION: &str = "EditorStartupGarrison";
const PREFIX: &str = "EDGAR";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Deployment {
    id: String,
    waypoint: u32,
    infantry: String,
    count: u16,
}

fn parse(value: &str) -> Option<Deployment> {
    let [id, waypoint, infantry, count]: [&str; 4] =
        value.split(',').collect::<Vec<_>>().try_into().ok()?;
    let ordinal = id.strip_prefix(PREFIX)?.parse::<u32>().ok()?;
    let waypoint = waypoint.parse().ok()?;
    let count = count.parse().ok()?;
    (id == format!("{PREFIX}{ordinal:04}")
        && (u32::from(crate::new_map::MAX_PLAYERS)..=mission::schema::MAX_WAYPOINT_INDEX)
            .contains(&waypoint)
        && (1..=MAX_COUNT).contains(&count)
        && valid_infantry_id(infantry))
    .then(|| Deployment {
        id: id.into(),
        waypoint,
        infantry: infantry.into(),
        count,
    })
}

fn deployment(doc: &MapDocument, record: &Record) -> Option<Deployment> {
    if record.section != "Structures" {
        return None;
    }
    parse(&doc.ini_text(SECTION, &record.key))
}

fn team_house(doc: &MapDocument, record: &Record) -> Result<String> {
    if let Some(slot) = startup_owner::get(doc, record) {
        ensure!(slot < 8, "开局驻军支持原版玩家起点 0～7");
        return Ok(format!("<Player @ {}>", char::from(b'A' + slot)));
    }
    let owner = record
        .fields()
        .into_iter()
        .next()
        .context("建筑缺少所属方")?;
    ensure!(!owner.is_empty() && owner.is_ascii(), "建筑所属方无效");
    Ok(owner)
}

// Script 64 seeks the nearest eligible structure, including neutral buildings,
// but advances even when it finds none. Eligibility can change during startup
// (for example, an empty civilian building reverting ownership), so retry.
const ENTER_ORDER: &str = "64,0";
const RETRY_DELAY: &str = "5,1";
const RETRY: &str = "6,1";

fn other_team_references(doc: &MapDocument, team: &str, field: &str, definition: &str) -> bool {
    mission::ids(doc, Kind::Team).iter().any(|other| {
        other != team
            && doc
                .ini_text(other, field)
                .trim()
                .eq_ignore_ascii_case(definition)
    })
}

fn legacy_script(doc: &MapDocument, record: &Record) -> Option<String> {
    let deployment = deployment(doc, record)?;
    let script = format!("{}S", deployment.id);
    let team = format!("{}T", deployment.id);
    let entries = doc.physical_ini_entries(&script);
    (doc.ini_text(&team, "Script") == script
        && doc.ini_text(&team, "Name") == name(&deployment.id)
        && entries.len() == 3
        && doc.ini_text(&script, "Name") == name(&deployment.id)
        && matches!(doc.ini_text(&script, "0").as_str(), "63,0" | "64,0")
        && doc.ini_text(&script, "1") == "2,0"
        && !other_team_references(doc, &team, "Script", &script))
    .then_some(script)
}

pub fn update_legacy_script(doc: &mut MapDocument, record: &Record) -> Result<bool> {
    let Some(script) = legacy_script(doc, record) else {
        return Ok(false);
    };
    put(doc, &script, "0", ENTER_ORDER)?;
    put(doc, &script, "1", RETRY_DELAY)?;
    put(doc, &script, "2", RETRY)?;
    Ok(true)
}

fn name(id: &str) -> String {
    format!("Startup garrison {id}")
}

fn allocate(doc: &MapDocument) -> Result<String> {
    let ids: Vec<_> = Kind::ALL
        .into_iter()
        .flat_map(|kind| mission::ids(doc, kind))
        .collect();
    (1u32..)
        .map(|n| format!("{PREFIX}{n:04}"))
        .find(|id| {
            ["T", "S", "F", "R", "G"].into_iter().all(|suffix| {
                let candidate = format!("{id}{suffix}");
                !ids.iter().any(|id| id.eq_ignore_ascii_case(&candidate))
                    && doc.ini_entries(&candidate).is_empty()
                    && doc.ini_value("Actions", &candidate).is_none()
                    && doc.ini_value("Events", &candidate).is_none()
            })
        })
        .context("无法分配驻军任务 ID")
}

/// Creates or updates a one-shot reinforcement at the building's waypoint.
/// Existing object tags (including startup ownership) are independent.
pub fn set(doc: &mut MapDocument, record_id: u64, garrison: Option<(&str, u16)>) -> Result<()> {
    let record = doc.record(record_id).cloned().context("建筑不存在")?;
    ensure!(record.section == "Structures", "只有建筑支持开局驻军");
    let existing = deployment(doc, &record);
    doc.atomic(|doc| {
        let Some((infantry, count)) = garrison else {
            if let Some(existing) = existing {
                remove(doc, &record, &existing)?;
            }
            return Ok(());
        };
        ensure!(valid_infantry_id(infantry), "驻军步兵 ID 无效");
        ensure!((1..=MAX_COUNT).contains(&count), "驻军人数超出范围");
        let house = team_house(doc, &record)?;
        let cell = record.cell().context("建筑坐标无效")?;
        let (id, waypoint) = if let Some(existing) = existing {
            let id = &existing.id;
            let team = format!("{id}T");
            ensure!(
                !other_team_references(doc, &team, "Script", &format!("{id}S"))
                    && !other_team_references(doc, &team, "TaskForce", &format!("{id}F")),
                "开局驻军的脚本或特遣部队被其他小队共用，请先解除引用"
            );
            (existing.id, existing.waypoint)
        } else {
            ensure!(
                doc.ini_value(SECTION, &record.key).is_none(),
                "驻军配置无效"
            );
            let id = allocate(doc)?;
            let waypoint = doc.insert_record(Record {
                section: "Waypoints".into(),
                key: String::new(),
                value: (u32::from(cell[1]) * 1000 + u32::from(cell[0]))
                    .to_string()
                    .into_bytes(),
                coordinate_radix: 1000,
            })?;
            let waypoint = doc
                .record(waypoint)
                .context("驻军航点不存在")?
                .key
                .parse()?;
            for (suffix, kind) in [
                ("F", Kind::TaskForce),
                ("S", Kind::Script),
                ("T", Kind::Team),
            ] {
                let key = format!("{id}{suffix}");
                mission::create(doc, kind, &key, &house, None)?;
                put(doc, &key, "Name", &name(&id))?;
            }
            let trigger = format!("{id}R");
            put(
                doc,
                "Triggers",
                &trigger,
                &format!("Neutral,<none>,{},0,1,1,1,0", name(&id)),
            )?;
            put(doc, "Events", &trigger, "1,13,0,1")?;
            put(doc, "Actions", &trigger, &format!("1,7,1,{id}T,0,0,0,0,A"))?;
            put(
                doc,
                "Tags",
                &format!("{id}G"),
                &format!("0,{},{}", name(&id), trigger),
            )?;
            (id, waypoint)
        };
        let team = format!("{id}T");
        let script = format!("{id}S");
        let force = format!("{id}F");
        let token = mission::schema::waypoint_token(waypoint)?;
        for (key, value) in [
            ("House", house.as_str()),
            ("Script", &script),
            ("TaskForce", &force),
            ("Waypoint", &token),
            ("Reinforce", "no"),
            ("AreTeamMembersRecruitable", "no"),
        ] {
            put(doc, &team, key, value)?;
        }
        put(doc, &script, "0", ENTER_ORDER)?;
        put(doc, &script, "1", RETRY_DELAY)?;
        put(doc, &script, "2", RETRY)?;
        put(doc, &force, "0", &format!("{count},{infantry}"))?;
        put(
            doc,
            SECTION,
            &record.key,
            &format!("{id},{waypoint},{infantry},{count}"),
        )?;
        Ok(())
    })
}

fn remove(doc: &mut MapDocument, record: &Record, deployment: &Deployment) -> Result<()> {
    let id = &deployment.id;
    for (kind, suffix) in [
        (Kind::Tag, "G"),
        (Kind::Trigger, "R"),
        (Kind::Team, "T"),
        (Kind::Script, "S"),
        (Kind::TaskForce, "F"),
    ] {
        mission::delete(doc, kind, &format!("{id}{suffix}"))?;
    }
    doc.set_ini(SECTION, &record.key, None)?;
    let key = deployment.waypoint.to_string();
    let waypoint = doc
        .record_iter()
        .find_map(|(id, r)| (r.section == "Waypoints" && r.key == key).then_some(id));
    if let Some(waypoint) = waypoint {
        doc.set(Target::Record(waypoint), None)?;
    }
    Ok(())
}

pub(crate) fn owns_waypoint(doc: &MapDocument, record: &Record) -> bool {
    record.section == "Waypoints"
        && record.key.parse::<u32>().is_ok_and(|waypoint| {
            doc.ini_entries(SECTION).iter().any(|(_, _, bytes)| {
                parse(&String::from_utf8_lossy(bytes))
                    .is_some_and(|deployment| deployment.waypoint == waypoint)
            })
        })
}

/// Structural resize patches spatial rows in bulk. Clean up the mission rows
/// of clipped buildings using their original keys, before publishing the plan.
pub(crate) fn remove_clipped(doc: &mut MapDocument, clipped: &[Record]) -> Result<()> {
    for record in clipped {
        if let Some(deployment) = deployment(doc, record) {
            remove(doc, record, &deployment)?;
        }
    }
    Ok(())
}

/// Keep generated references in the same history entry as spatial changes.
pub(crate) fn record_change(
    doc: &mut MapDocument,
    old: &Record,
    new: Option<&Record>,
) -> Result<()> {
    ensure!(
        new == Some(old) || !owns_waypoint(doc, old),
        "该航点由开局驻军自动管理"
    );
    let Some(deployment) = deployment(doc, old) else {
        return Ok(());
    };
    let Some(new) = new.filter(|new| {
        new.section == old.section && new.key == old.key && new.type_id() == old.type_id()
    }) else {
        return doc.atomic(|doc| remove(doc, old, &deployment));
    };
    let old_fields = old.fields();
    let new_fields = new.fields();
    let owner_changed =
        old_fields.first() != new_fields.first() || old_fields.get(6) != new_fields.get(6);
    let moved = new.cell() != old.cell();
    if !owner_changed && !moved {
        return Ok(());
    }
    doc.atomic(|doc| {
        if owner_changed {
            let house = team_house(doc, new)?;
            put(doc, &format!("{}T", deployment.id), "House", &house)?;
        }
        if moved {
            // Temporarily release the waypoint ownership guard for this move.
            let value = doc.ini_value(SECTION, &old.key).context("驻军配置不存在")?;
            doc.set_ini(SECTION, &old.key, None)?;
            doc.waypoint(deployment.waypoint, new.cell().context("建筑坐标无效")?)?;
            doc.set_ini(SECTION, &old.key, Some(value))?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Baseline, Value},
        test_support::document,
    };
    use vera20k_core::map::map_file;

    #[test]
    fn deployment_exports_native_rows_and_roundtrips_per_building() {
        let mut doc = document();
        let bunker = doc
            .add_record("Structures", [2, 2], "Americans", "NABNKR")
            .unwrap();
        let other = doc
            .add_record("Structures", [4, 4], "Americans", "NABNKR")
            .unwrap();
        let before = doc.record(bunker).unwrap().clone();
        doc.commit("place buildings");
        set(&mut doc, bunker, Some(("E2", 5))).unwrap();
        doc.commit("startup garrison");
        assert_eq!(doc.record(bunker), Some(&before));
        assert!(infantry(&doc, doc.record(other).unwrap()).is_none());
        let deployment = deployment(&doc, &before).unwrap();
        let id = deployment.id;
        let trigger = format!("{id}R");
        let bytes = doc.snapshot().bytes().unwrap();
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        assert_eq!(map.events[&trigger].conditions()[0].kind, 13);
        assert_eq!(map.events[&trigger].conditions()[0].value, 1);
        assert_eq!(map.actions[&trigger].entries()[0].kind, 7);
        let reopened = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
        assert_eq!(infantry(&reopened, &before).as_deref(), Some("E2"));
        assert_eq!(reopened.ini_text(&format!("{id}T"), "House"), "Americans");
        assert_eq!(reopened.ini_text(&format!("{id}F"), "0"), "5,E2");
        let script = format!("{id}S");
        assert_eq!(reopened.ini_text(&script, "0"), ENTER_ORDER);
        assert_eq!(reopened.ini_text(&script, "1"), RETRY_DELAY);
        assert_eq!(reopened.ini_text(&script, "2"), RETRY);
        let mut legacy = reopened;
        put(&mut legacy, &script, "0", "64,0").unwrap();
        put(&mut legacy, &script, "1", "2,0").unwrap();
        legacy.set_ini(&script, "2", None).unwrap();
        legacy.commit("older generated script");
        assert!(has_legacy_script(&legacy, &before));
        assert!(update_legacy_script(&mut legacy, &before).unwrap());
        legacy.commit("update script");
        assert_eq!(legacy.ini_text(&script, "0"), ENTER_ORDER);
        assert_eq!(legacy.ini_text(&script, "1"), RETRY_DELAY);
        assert_eq!(legacy.ini_text(&script, "2"), RETRY);
        assert!(legacy.undo());
        assert_eq!(legacy.ini_text(&script, "0"), "64,0");
        assert_eq!(legacy.ini_text(&script, "1"), "2,0");
        put(&mut legacy, &script, "1", "5,0").unwrap();
        assert!(!update_legacy_script(&mut legacy, &before).unwrap());
        assert!(doc.undo());
        assert!(infantry(&doc, &before).is_none());
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }

    #[test]
    fn ownership_change_and_legacy_update_leave_shared_scripts_intact() {
        let mut doc = document();
        let building = doc
            .add_record("Structures", [2, 2], "Americans", "NABNKR")
            .unwrap();
        set(&mut doc, building, Some(("E1", 5))).unwrap();
        let record = doc.record(building).unwrap().clone();
        let deployment = deployment(&doc, &record).unwrap();
        let script = format!("{}S", deployment.id);
        put(&mut doc, &script, "0", "63,0").unwrap();
        mission::create(&mut doc, Kind::Team, "OTHERTEAM", "Americans", None).unwrap();
        put(&mut doc, "OTHERTEAM", "Script", &script.to_lowercase()).unwrap();

        assert!(!has_legacy_script(&doc, &record));
        assert!(!update_legacy_script(&mut doc, &record).unwrap());
        let before = doc.snapshot().bytes().unwrap();
        assert!(set(&mut doc, building, Some(("E2", 5))).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);

        put(&mut doc, "OTHERTEAM", "Script", "None").unwrap();
        let force = format!("{}F", deployment.id);
        put(&mut doc, "OTHERTEAM", "TaskForce", &force.to_lowercase()).unwrap();
        let before = doc.snapshot().bytes().unwrap();
        assert!(set(&mut doc, building, Some(("E2", 5))).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);

        startup_owner::set(&mut doc, building, Some(0)).unwrap();
        assert_eq!(doc.ini_text(&script, "0"), "63,0");
        assert_eq!(
            doc.ini_text(&format!("{}T", deployment.id), "House"),
            "<Player @ A>"
        );
    }

    #[test]
    fn region_move_uses_the_building_position_without_copying_a_private_waypoint() {
        use crate::region::{Clipboard, RegionLayers};
        let mut doc = document();
        let building = doc
            .add_record("Structures", [2, 2], "Neutral", "NABNKR")
            .unwrap();
        set(&mut doc, building, Some(("E1", 5))).unwrap();
        let clip = Clipboard::capture(
            &doc,
            &[[2, 2]].into(),
            RegionLayers {
                waypoints: true,
                objects: [true, false, false, false, false, false],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(clip.records.len(), 2);
        assert!(
            clip.records
                .iter()
                .any(|(_, record)| record.section == "Waypoints" && record.key == "0")
        );
        let plan = clip.plan(&doc, [4, 4]).unwrap();
        plan.apply(&mut doc, false).unwrap();
        let building = doc.record(building).unwrap();
        let deployment = deployment(&doc, building).unwrap();
        assert_eq!(building.cell(), Some([4, 4]));
        assert!(doc.record_iter().any(|(_, r)| r.section == "Waypoints"
            && r.key == deployment.waypoint.to_string()
            && r.cell() == Some([4, 4])));
    }

    #[test]
    fn deployment_follows_ownership_and_movement_and_cleans_up_on_delete() {
        let mut doc = document();
        let bunker = doc
            .add_record("Structures", [2, 2], "Neutral", "NABNKR")
            .unwrap();
        set(&mut doc, bunker, Some(("E1", 5))).unwrap();
        let deployment = deployment(&doc, doc.record(bunker).unwrap()).unwrap();
        let script = format!("{}S", deployment.id);
        let team = format!("{}T", deployment.id);
        assert_eq!(doc.ini_text(&script, "0"), "64,0");
        startup_owner::set(&mut doc, bunker, Some(0)).unwrap();
        assert_eq!(doc.ini_text(&team, "House"), "<Player @ A>");
        assert_eq!(doc.ini_text(&script, "0"), ENTER_ORDER);
        let moved = doc.record(bunker).unwrap().moved([4, 4]).unwrap();
        doc.set(Target::Record(bunker), Some(Value::Record(moved)))
            .unwrap();
        assert!(doc.record_iter().any(|(_, r)| r.section == "Waypoints"
            && r.key == deployment.waypoint.to_string()
            && r.cell() == Some([4, 4])));
        doc.commit("configured building");
        let before = doc.snapshot().bytes().unwrap();
        doc.set(Target::Record(bunker), None).unwrap();
        doc.commit("delete building");
        for kind in [Kind::Team, Kind::Script, Kind::TaskForce] {
            assert!(mission::ids(&doc, kind).is_empty());
        }
        assert!(doc.ini_entries(SECTION).is_empty());
        assert_eq!(
            doc.record_iter()
                .filter(|(_, r)| r.section == "Waypoints")
                .count(),
            1
        );
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}
