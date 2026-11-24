//! Map-authored production restrictions and the original values needed to remove them.
use crate::document::MapDocument;
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::rules::{
    locomotor_type::LocomotorKind,
    object_type::{BuildCategory, ObjectCategory},
    ruleset::RuleSet,
};

const SETTINGS: &str = "YRMapEditorProduction";
// Keep each original field independent so opaque bytes can round-trip unchanged.
const AI_BUILDINGS: &str = "YRMapEditorProductionAIBuildThis";
const TECH_LEVEL: &str = "TechLevel";
const BUILDING_TECH_LEVEL: &[u8] = b"2147483647";
const FIELDS: [(&str, &str, &[u8]); 3] = [
    ("YRMapEditorProductionTechLevel", TECH_LEVEL, b"-1"),
    ("YRMapEditorProductionBuildLimit", "BuildLimit", b"0"),
    (AI_BUILDINGS, "AIBuildThis", b"no"),
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Restriction {
    Air,
    Naval,
    Land,
    Infantry,
    Buildings,
    Defenses,
    SuperweaponBuildings,
    Spies,
    StolenTech,
}

impl Restriction {
    pub const ALL: [Self; 9] = [
        Self::Air,
        Self::Naval,
        Self::Land,
        Self::Infantry,
        Self::Buildings,
        Self::Defenses,
        Self::SuperweaponBuildings,
        Self::Spies,
        Self::StolenTech,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::Air => "Air",
            Self::Naval => "Naval",
            Self::Land => "LandVehicles",
            Self::Infantry => "Infantry",
            Self::Buildings => "Buildings",
            Self::Defenses => "Defenses",
            Self::SuperweaponBuildings => "SuperweaponBuildings",
            Self::Spies => "Spies",
            Self::StolenTech => "StolenTech",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Air => "禁止生产空军",
            Self::Naval => "禁止生产海军",
            Self::Land => "禁止生产陆地载具",
            Self::Infantry => "禁止生产步兵",
            Self::Buildings => "禁止建造全部建筑",
            Self::Defenses => "禁止建造防御设施",
            Self::SuperweaponBuildings => "禁止建造超级武器设施",
            Self::Spies => "禁止生产间谍",
            Self::StolenTech => "禁止生产偷科技单位",
        }
    }
}

pub type Restrictions = BTreeSet<Restriction>;
pub type Inventory = BTreeMap<String, Restrictions>;

/// Project from the same processed Rules types the engine uses for production.
pub fn inventory(rules: &RuleSet) -> Inventory {
    use Restriction::*;
    let mut inventory = Inventory::new();
    for object in rules.all_objects() {
        let groups = inventory.entry(object.id.clone()).or_default();
        let flies = object.category == ObjectCategory::Aircraft
            || object.considered_aircraft
            || object.jumpjet
            || matches!(
                object.locomotor,
                LocomotorKind::Fly | LocomotorKind::Jumpjet
            );
        match object.category {
            ObjectCategory::Aircraft => {
                groups.insert(Air);
            }
            ObjectCategory::Vehicle if object.naval => {
                groups.insert(Naval);
            }
            ObjectCategory::Vehicle if !flies => {
                groups.insert(Land);
            }
            ObjectCategory::Infantry => {
                groups.insert(Infantry);
            }
            ObjectCategory::Building => {
                groups.insert(Buildings);
                if object.build_cat == Some(BuildCategory::Combat) {
                    groups.insert(Defenses);
                }
                // Mirror the shell's construction gate, including its BuildTech exception.
                if !rules.is_build_tech(&object.id)
                    && object
                        .super_weapon
                        .as_deref()
                        .and_then(|id| rules.super_weapon(id))
                        .is_some_and(|weapon| weapon.disableable_from_shell)
                {
                    groups.insert(SuperweaponBuildings);
                }
            }
            _ => {}
        }
        if flies && object.category != ObjectCategory::Building {
            groups.insert(Air);
        }
        if object.agent {
            groups.insert(Spies);
        }
        if object.requires_stolen_allied_tech
            || object.requires_stolen_soviet_tech
            || object.requires_stolen_third_tech
        {
            groups.insert(StolenTech);
        }
    }
    inventory
}

pub fn read(doc: &MapDocument) -> Restrictions {
    Restriction::ALL
        .into_iter()
        .filter(|item| doc.ini_text(SETTINGS, item.key()) == "yes")
        .collect()
}

fn targets<'a>(
    selected: &Restrictions,
    inventory: &'a Inventory,
) -> (BTreeSet<&'a str>, BTreeSet<&'a str>) {
    let mut wanted = BTreeSet::new();
    let mut buildings = BTreeSet::new();
    for (id, groups) in inventory {
        if !groups.is_disjoint(selected) {
            wanted.insert(id.as_str());
            if groups.contains(&Restriction::Buildings) {
                buildings.insert(id.as_str());
            }
        }
    }
    (wanted, buildings)
}

fn blocked_value<'a>(
    id: &str,
    key: &str,
    default: &'a [u8],
    buildings: &BTreeSet<&str>,
) -> &'a [u8] {
    // Defense and generated-base choosers use signed TechLevel comparisons.
    if key == TECH_LEVEL && buildings.contains(id) {
        BUILDING_TECH_LEVEL
    } else {
        default
    }
}

pub(crate) fn blocked_ai_buildings(doc: &MapDocument) -> BTreeSet<String> {
    doc.ini_entries(AI_BUILDINGS)
        .into_iter()
        .map(|(_, id, _)| id.to_ascii_uppercase())
        .collect()
}

/// Check the actual map overrides as well as the saved switches so a refreshed
/// inventory or direct INI edit can be synchronized without toggling a switch.
pub fn needs_apply(doc: &MapDocument, selected: &Restrictions, inventory: &Inventory) -> bool {
    let (wanted, buildings) = targets(selected, inventory);
    for (section, key, default) in FIELDS {
        let expected = if section == AI_BUILDINGS {
            &buildings
        } else {
            &wanted
        };
        let saved = doc.physical_ini_entries(section);
        if saved.len() != expected.len()
            || saved
                .iter()
                .any(|(_, id, _)| !expected.contains(id.as_str()))
            || expected.iter().any(|id| {
                physical_value(doc, id, key).ok().flatten().as_deref()
                    != Some(blocked_value(id, key, default, &buildings))
            })
        {
            return true;
        }
    }
    let blocked = buildings.into_iter().map(str::to_ascii_uppercase).collect();
    crate::mission::ids(doc, crate::mission::Kind::House)
        .iter()
        .any(|house| crate::base_nodes::has_blocked_active_node(doc, house, &blocked))
}

/// Restrict normal factory qualification and project authored AI base plans
/// without removing any placed objects from the scenario.
pub fn apply(
    doc: &mut MapDocument,
    selected: &Restrictions,
    inventory: Option<&Inventory>,
) -> Result<usize> {
    let (wanted, buildings) = if selected.is_empty() {
        (BTreeSet::new(), BTreeSet::new())
    } else {
        let inventory = inventory.context("当前游戏资源的生产类型不可用")?;
        ensure!(!inventory.is_empty(), "当前游戏资源的生产类型不可用");
        targets(selected, inventory)
    };
    doc.atomic(|doc| {
        let previous_buildings = blocked_ai_buildings(doc);
        for (section, key, default) in FIELDS {
            let expected = if section == AI_BUILDINGS {
                &buildings
            } else {
                &wanted
            };
            let mut retained = BTreeSet::new();
            for (_, id, backup) in doc.physical_ini_entries(section) {
                ensure!(retained.insert(id.clone()), "重复的生产限制备份 {id}/{key}");
                let original = match backup.as_slice() {
                    b"-" => None,
                    [b'+', bytes @ ..] => Some(bytes.to_vec()),
                    _ => anyhow::bail!("无效的生产限制备份 {id}/{key}"),
                };
                let current = physical_value(doc, &id, key)?;
                let current = current.as_deref();
                let blocked = blocked_value(&id, key, default, &buildings);
                // Legacy maps used -1 even for buildings. Preserve the original
                // backup while migrating between the two limits.
                let managed = current == Some(default)
                    || (key == TECH_LEVEL
                        && previous_buildings.contains(&id.to_ascii_uppercase())
                        && current == Some(BUILDING_TECH_LEVEL));
                if !expected.contains(id.as_str()) {
                    if managed {
                        doc.set_ini(&id, key, original)?;
                    }
                    doc.set_ini(section, &id, None)?;
                } else if current != Some(blocked) {
                    if managed {
                        doc.set_ini(&id, key, Some(blocked.to_vec()))?;
                    } else {
                        apply_field(doc, section, &id, key, blocked)?;
                    }
                }
            }
            for &id in expected {
                if !retained.contains(id) {
                    apply_field(
                        doc,
                        section,
                        id,
                        key,
                        blocked_value(id, key, default, &buildings),
                    )?;
                }
            }
        }
        let blocked = blocked_ai_buildings(doc);
        for house in crate::mission::ids(doc, crate::mission::Kind::House) {
            if crate::base_nodes::has_production_nodes(doc, &house, &blocked) {
                let plan = crate::base_nodes::read(doc, &house)
                    .with_context(|| format!("{house} 的基地计划无法应用生产限制"))?;
                crate::base_nodes::write(doc, &house, &plan)?;
            }
        }
        let previous = read(doc);
        for item in Restriction::ALL {
            if selected.contains(&item) != previous.contains(&item) {
                doc.set_ini(
                    SETTINGS,
                    item.key(),
                    selected.contains(&item).then(|| b"yes".to_vec()),
                )?;
            }
        }
        Ok(wanted.len())
    })
}

fn physical_value(doc: &MapDocument, id: &str, key: &str) -> Result<Option<Vec<u8>>> {
    let mut entries = doc
        .physical_ini_entries(id)
        .into_iter()
        .filter(|(_, name, _)| name == key);
    let value = entries.next().map(|(_, _, value)| value);
    ensure!(
        entries.next().is_none(),
        "{id}/{key} 有重复字段，无法保存和恢复生产规则"
    );
    Ok(value)
}

fn apply_field(
    doc: &mut MapDocument,
    section: &str,
    id: &str,
    key: &str,
    blocked: &[u8],
) -> Result<()> {
    let backup = match physical_value(doc, id, key)? {
        Some(value) => [b"+", value.as_slice()].concat(),
        None => b"-".to_vec(),
    };
    doc.set_ini(section, id, Some(backup))?;
    doc.set_ini(id, key, Some(blocked.to_vec()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Restriction::*, *};
    use crate::{document::Baseline, test_support};
    use vera20k_core::{
        map::map_file,
        rules::{
            ini_parser::IniFile,
            native_processing::{RulesLayerKind, RulesLayerStack},
        },
        sim::{
            house_state::HouseState,
            production::{CanBuild, can_build},
            scenario_session::ScenarioGameMode,
            world::Simulation,
        },
    };

    const STOCK: &str = "[AircraftTypes]\n0=PLANE\n[InfantryTypes]\n0=FLIER\n1=SPY\n2=HEIST\n\
        [VehicleTypes]\n0=SHIP\n1=TANK\n[BuildingTypes]\n0=GUN\n1=LAB\n2=SILO\n\
        [SuperWeaponTypes]\n0=WEAPON\n[AI]\nBuildTech=LAB\n\
        [WEAPON]\nType=IronCurtain\nDisableableFromShell=yes\n\
        [PLANE]\nTechLevel=2\nAirportBound=yes\n[FLIER]\nTechLevel=2\nJumpJet=yes\n\
        [SHIP]\nTechLevel=3\nNaval=yes\n[TANK]\nTechLevel=1\n[GUN]\nTechLevel=2\nBuildCat=Combat\n\
        [SILO]\nTechLevel=2\nSuperWeapon=WEAPON\n[LAB]\nTechLevel=2\nSuperWeapon=WEAPON\n\
        [SPY]\nTechLevel=2\nAgent=yes\n[HEIST]\nTechLevel=2\nRequiresStolenAlliedTech=yes\n";

    fn process(bytes: &[u8], ares: bool) -> RuleSet {
        let mut stack = RulesLayerStack::new(IniFile::from_str(STOCK)).with_ares_mix(ares);
        stack.push(
            RulesLayerKind::Scenario,
            IniFile::from_bytes(bytes).unwrap(),
        );
        RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap()
    }

    #[test]
    fn catalog_and_saved_rules_reach_engine_production_checks() {
        let mut doc = test_support::document();
        doc.set_ini("AircraftTypes", "1", Some(b"MAPPLANE".to_vec()))
            .unwrap();
        doc.set_ini("MAPPLANE", "TechLevel", Some(b"4".to_vec()))
            .unwrap();
        doc.set_ini("SHIP", "BuildLimit", Some(b"3".to_vec()))
            .unwrap();
        doc.commit("map rules");
        let before = doc.snapshot().bytes().unwrap();
        let placements = doc.records();
        let inventory = inventory(&process(&before, false));
        for (id, expected) in [
            ("PLANE", vec![Air]),
            ("MAPPLANE", vec![Air]),
            ("FLIER", vec![Air, Infantry]),
            ("SHIP", vec![Naval]),
            ("TANK", vec![Land]),
            ("GUN", vec![Buildings, Defenses]),
            ("LAB", vec![Buildings]),
            ("SILO", vec![Buildings, SuperweaponBuildings]),
            ("SPY", vec![Infantry, Spies]),
            ("HEIST", vec![Infantry, StolenTech]),
        ] {
            assert_eq!(inventory[id], expected.into_iter().collect(), "{id}");
        }
        let selected = [Air, Naval].into_iter().collect();
        assert_eq!(apply(&mut doc, &selected, Some(&inventory)).unwrap(), 4);
        doc.commit("production restrictions");
        let bytes = doc.snapshot().bytes().unwrap();
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut reopened = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
        assert_eq!(read(&reopened), selected);
        assert_eq!(reopened.records(), placements);
        for ares in [false, true] {
            let rules = process(&bytes, ares);
            assert_eq!(rules.ares_form(), ares);
            let mut sim = Simulation::with_seed(1);
            sim.install_session_mode(ScenarioGameMode::Skirmish);
            sim.intern_rule_type_ids(&rules);
            let owner = sim.intern("EditorTest");
            sim.register_house(HouseState::new(owner, 0, None, false, 0, 10));
            for skip_tech in [false, true] {
                for id in ["PLANE", "MAPPLANE", "FLIER", "SHIP"] {
                    let object = rules.object(id).unwrap();
                    assert_eq!((object.tech_level, object.build_limit), (-1, 0), "{id}");
                    assert_eq!(
                        can_build(&sim, &rules, owner, object, skip_tech, true),
                        CanBuild::No,
                        "{id}, Ares={ares}, skip_tech={skip_tech}"
                    );
                }
                if skip_tech || !ares {
                    assert_eq!(
                        can_build(
                            &sim,
                            &rules,
                            owner,
                            rules.object("TANK").unwrap(),
                            skip_tech,
                            true
                        ),
                        CanBuild::Yes
                    );
                }
            }
        }
        apply(
            &mut reopened,
            &[Air, Infantry].into_iter().collect(),
            Some(&inventory),
        )
        .unwrap();
        assert_eq!(reopened.ini_text("FLIER", "BuildLimit"), "0");
        assert_eq!(reopened.ini_text("SHIP", "BuildLimit"), "3");
        assert_eq!(reopened.ini_value("SHIP", "TechLevel"), None);
        apply(&mut reopened, &Restrictions::new(), None).unwrap();
        assert_eq!(reopened.ini_text("MAPPLANE", "TechLevel"), "4");
        assert_eq!(reopened.ini_value("FLIER", "BuildLimit"), None);
        assert!(read(&reopened).is_empty());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }

    #[test]
    fn authored_ai_plan_is_filtered_without_losing_order_or_edits_on_reopen() {
        use crate::{
            base_nodes,
            mission::{self, Kind},
        };
        use vera20k_core::map::houses::parse_house_roster;
        let mut doc = test_support::document();
        let house = "TESTAI";
        mission::create(&mut doc, Kind::House, house, "Americans", None).unwrap();
        let original = [
            base_nodes::Node::parse(b"GUN,16,16,\x81\xfe".to_vec()).unwrap(),
            base_nodes::Node::new("LAB", [18, 18]).unwrap(),
            base_nodes::Node::new("SILO", [19, 19]).unwrap(),
        ];
        base_nodes::write(&mut doc, house, &original).unwrap();
        doc.set_ini("GUN", "TechLevel", Some(b"4".to_vec()))
            .unwrap();
        doc.set_ini("GUN", "AIBuildThis", Some(b"yes".to_vec()))
            .unwrap();
        doc.commit("fixture");
        let before = doc.snapshot().bytes().unwrap();
        let catalog = inventory(&process(&before, false));
        let selected = [Defenses, SuperweaponBuildings].into_iter().collect();
        assert_eq!(apply(&mut doc, &selected, Some(&catalog)).unwrap(), 2);
        assert!(!needs_apply(&doc, &selected, &catalog));
        assert_eq!(base_nodes::read(&doc, house).unwrap(), original);
        assert_eq!(doc.ini_text(house, "NodeCount"), "1");
        assert_eq!(doc.ini_text(house, "000"), "LAB,18,18");
        doc.commit("filter plans");
        let bytes = doc.snapshot().bytes().unwrap();
        let rules = process(&bytes, false);
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        let roster = parse_house_roster(&map.ini, &[], Some(&rules));
        let plan = &roster
            .houses
            .iter()
            .find(|h| h.name == house)
            .unwrap()
            .base_plan
            .nodes;
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].type_or_control,
            rules.object("LAB").unwrap().base_plan_type_index
        );
        let gun = rules.object("GUN").unwrap();
        assert_eq!(gun.tech_level, i32::MAX);
        assert!(!gun.ai_build_this);
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);

        let mut reopened = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
        let changed = [
            original[0].moved([17, 17]).unwrap(),
            original[1].clone(),
            original[2].clone(),
        ];
        base_nodes::write(&mut reopened, house, &changed).unwrap();
        assert_eq!(reopened.ini_text(house, "NodeCount"), "1");
        assert_eq!(base_nodes::read(&reopened, house).unwrap(), changed);
        apply(
            &mut reopened,
            &[SuperweaponBuildings].into_iter().collect(),
            Some(&catalog),
        )
        .unwrap();
        assert_eq!(reopened.ini_text(house, "NodeCount"), "2");
        assert_eq!(reopened.ini_value(house, "000").unwrap(), changed[0].raw);
        assert_eq!(reopened.ini_text("GUN", "TechLevel"), "4");
        assert_eq!(reopened.ini_text("GUN", "AIBuildThis"), "yes");
        apply(&mut reopened, &Restrictions::new(), None).unwrap();
        assert_eq!(reopened.ini_text(house, "NodeCount"), "3");
        assert_eq!(base_nodes::read(&reopened, house).unwrap(), changed);
        assert!(reopened.ini_entries(base_nodes::SUPPRESSED).is_empty());
    }

    #[test]
    fn reapply_updates_a_changed_inventory_and_migrates_legacy_buildings() {
        let mut doc = test_support::document();
        let selected = [Defenses].into_iter().collect();
        let catalog = BTreeMap::from([("NEW".into(), [Buildings, Defenses].into_iter().collect())]);
        doc.set_ini(SETTINGS, "Defenses", Some(b"yes".to_vec()))
            .unwrap();
        for id in ["OLD", "NEW"] {
            doc.set_ini(FIELDS[0].0, id, Some(b"+2".to_vec())).unwrap();
            doc.set_ini(FIELDS[1].0, id, Some(b"-".to_vec())).unwrap();
            doc.set_ini(id, TECH_LEVEL, Some(b"-1".to_vec())).unwrap();
            doc.set_ini(id, "BuildLimit", Some(b"0".to_vec())).unwrap();
        }
        assert!(needs_apply(&doc, &selected, &catalog));
        apply(&mut doc, &selected, Some(&catalog)).unwrap();
        assert_eq!(doc.ini_text("OLD", TECH_LEVEL), "2");
        assert_eq!(doc.ini_value("OLD", "BuildLimit"), None);
        assert_eq!(
            doc.ini_value("NEW", TECH_LEVEL).as_deref(),
            Some(BUILDING_TECH_LEVEL)
        );
        assert!(!needs_apply(&doc, &selected, &catalog));
        apply(&mut doc, &Restrictions::new(), None).unwrap();
        assert_eq!(doc.ini_text("NEW", TECH_LEVEL), "2");
    }

    #[test]
    fn restoring_preserves_manual_edits_and_duplicate_keys_fail_atomically() {
        let mut doc = test_support::document();
        let selected = [Air].into_iter().collect();
        let inventory = BTreeMap::from([("PLANE".into(), [Air].into_iter().collect())]);
        doc.set_ini("PLANE", "TechLevel", Some(b"\x81\xfe".to_vec()))
            .unwrap();
        apply(&mut doc, &selected, Some(&inventory)).unwrap();
        doc.set_ini("PLANE", "BuildLimit", Some(b"5".to_vec()))
            .unwrap();
        apply(&mut doc, &Restrictions::new(), None).unwrap();
        assert_eq!(
            doc.ini_value("PLANE", "TechLevel"),
            Some(b"\x81\xfe".to_vec())
        );
        assert_eq!(doc.ini_text("PLANE", "BuildLimit"), "5");

        let mut bytes = test_support::fixture();
        bytes.extend_from_slice(b"\r\n[PLANE]\r\nBuildLimit=1\r\nBuildLimit=2\r\n");
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
        assert!(apply(&mut doc, &selected, Some(&inventory)).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }
}
