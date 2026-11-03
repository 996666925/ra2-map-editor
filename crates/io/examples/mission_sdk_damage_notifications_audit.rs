//! TS damage notifications from actual combat and source-free weapon detonations.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use vera20k_core::{
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope},
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn build(
    install: &mut Install,
    root: &Path,
    damage: i32,
    sourced: bool,
    friendly: bool,
) -> Result<Vec<u8>> {
    std::fs::create_dir_all(root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    // Reverse the Country order, so event44 cannot accidentally use CountryIdx.
    for house in ["Russians", "Americans"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    doc.add_record(
        "Units",
        [40, 60],
        if friendly { "Americans" } else { "Russians" },
        "HTK",
    )?;
    doc.add_record("Units", [82, 82], "Russians", "HTK")?;
    if sourced {
        doc.add_record("Units", [36, 60], "Americans", "MTNK")?;
    }
    for (section, key, value) in [
        ("HTK", "Strength", "300"),
        ("HTK", "Speed", "0"),
        ("HTK", "Armor", "none"),
        ("HTK", "Primary", "none"),
        ("HTK", "Secondary", "none"),
        ("MTNK", "Primary", "SDKHealthHit"),
        ("MTNK", "Secondary", "none"),
        ("SDKHealthHit", "Projectile", "InvisibleLow"),
        ("SDKHealthHit", "Speed", "100"),
        ("SDKHealthHit", "ROF", "30"),
        ("SDKHealthHit", "Range", "8"),
        ("SDKHealthHit", "Warhead", "SDKHealthWH"),
        (
            "SDKHealthWH",
            "Verses",
            "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
        ),
        ("SDKHealthWH", "CellSpread", "0.5"),
        ("SDKHealthWH", "PercentAtMax", "1"),
        ("SDKHealthWH", "AnimList", "EXPLOSML"),
        ("EXPLOSML", "Damage", "0"),
        ("General", "ConditionRed", "25%"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    doc.set_ini(
        "SDKHealthHit",
        "Damage",
        Some(damage.to_string().into_bytes()),
    )?;
    doc.commit("damage notification fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    let source=r#"import {defineMission,event,action} from './sdk';
import {map,House,Country,MapObject,Weapon} from './map.generated';
export default defineMission(map,{id:'damage_notifications',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const target=m.object(MapObject.Units_0);
 const names=['first_source','yellow_source','red_source','first_any','yellow_any','red_any','attacked','source_house','wrong_house','destroyed','latch_probe'] as const;
 const conditions=[event.firstDamagedBySource(target),event.halfHealthBySource(target),event.redHealthBySource(target),
   event.firstDamaged(target),event.halfHealth(target),event.redHealth(target),event.attacked(target),
   event.damagedByHouse(target,m.house(House.Americans)),event.damagedByHouse(target,m.house(House.Russians)),event.destroyedAny(target),event.firstDamagedBySource(target)] as const;
 for(let i=0;i<names.length;i++) {
   const id=names[i]!;
   m.rule(id,{when:conditions[i]!,once:false,...(i+1<names.length?{linked:m.ruleRef(names[i+1]!)}:{}),
     actions:[action.message(m.defineText(id,id)),action.setGlobal(m.globalFlag(i)),
       ...(id==='latch_probe'?[]:[action.disableRule(m.ruleRef(id))])]});
 }
 __DETONATIONS__
});"#.replace("__DETONATIONS__",if sourced {""}else{r#"
 const at=m.createWaypoint(30,40,60);
 for(let i=0;i<5;i++) m.rule('hit_'+i,{when:event.sinceScenarioStart(1+i*2),once:true,
   actions:[action.explosionAt(m.weapon(Weapon.SDKHealthHit),at)]});
"#});
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok(bytes)
}

fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    damage: i32,
    sourced: bool,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let target = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (40, 60))
        .context("target")?
        .0;
    let witness = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (82, 82))
        .context("witness")?
        .0;
    let texts = mission_script::declared_texts(bytes)?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("source owner")?;
    let source_house_index = runtime
        .simulation
        .houses()
        .order()
        .iter()
        .position(|id| *id == owner)
        .context("source House ordinal")?;
    let country_index = runtime
        .resources
        .rules
        .country_index("Americans")
        .context("source Country ordinal")?
        .0;
    let command = if sourced {
        let actor = runtime
            .simulation
            .entities()
            .iter_sorted()
            .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "MTNK")
            .context("attacker")?
            .0;
        Some(
            runtime
                .simulation
                .encode_megamission_record(
                    owner,
                    &Command::Attack {
                        attacker_id: actor,
                        target_id: target,
                    },
                )
                .context("ordinary attack command")?,
        )
    } else {
        None
    };
    let mut observations = Vec::new();
    let mut messages = Vec::new();
    let mut damage_rows = Vec::new();
    let mut last_health = 300;
    let mut witness_unchanged = true;
    let mut checkpoint_state = None;
    for frame in 0..240 {
        let commands = if frame == 0 {
            command
                .clone()
                .map(|c| vec![CommandEnvelope::synchronized(c, 0)])
                .unwrap_or_default()
        } else {
            vec![]
        };
        let output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let health = sim.entities().get(target).map_or(0, |e| e.health.current());
        let row = json!({"tick":tick,"health":health,"cell":sim.object_coords_cell(target),"flags":sim.global_variables()[..11],"projectiles":sim.projectiles().len()});
        if health != last_health {
            damage_rows.push(row.clone());
            last_health = health;
        }
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("authored message label")?,"health":health}));
            }
        }
        witness_unchanged &= sim.entities().get(witness).is_some_and(|e| {
            e.health.current() == 300 && (e.position.rx(), e.position.ry()) == (82, 82)
        });
        observations.push(row);
        if restore && checkpoint_state.is_none() && (0..300).contains(&health) {
            let before = json!({"health":health,"cell":sim.object_coords_cell(target),
                "owner":sim.entities().get(target).map(|e|e.owner()),"flags":sim.global_variables().as_slice()});
            checkpoint::restore_checkpoint(
                &mut runtime,
                bytes,
                &root.join("after-first-damage.bin"),
            )?;
            let sim = &runtime.simulation;
            let after = json!({"health":sim.entities().get(target).map_or(0,|e|e.health.current()),
                "cell":sim.object_coords_cell(target),"owner":sim.entities().get(target).map(|e|e.owner()),
                "flags":sim.global_variables().as_slice()});
            checkpoint_state =
                Some(json!({"tick":tick,"preserved":before==after,"before":before,"after":after}));
        }
    }
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let hit_count = (300 + damage - 1) / damage;
    let mut checks = BTreeMap::from([
        (
            "real_damage_and_death",
            last_health == 0 && damage_rows.len() == hit_count as usize,
        ),
        (
            "correct_damage_packets",
            damage_rows
                .iter()
                .enumerate()
                .all(|(i, row)| row["health"] == 0.max(300 - damage * (i as i32 + 1))),
        ),
        ("remote_witness_unchanged", witness_unchanged),
        (
            "source_house_differs_from_country_ordinal",
            source_house_index != usize::from(country_index),
        ),
        (
            "first_damage_requires_source_when_requested",
            count("first_source") == usize::from(sourced),
        ),
        (
            "first_damage_rule_disables_after_first_callback",
            count("first_any") == 1,
        ),
        (
            "damage_by_house_matches_actual_source",
            count("source_house") == usize::from(sourced),
        ),
        (
            "wrong_source_house_never_matches",
            count("wrong_house") == 0,
        ),
        ("one_destroy_notification", count("destroyed") == 1),
    ]);
    let yellow = usize::from(damage != 240);
    let red = usize::from(damage != 75);
    checks.insert(
        "yellow_threshold_and_red_override",
        count("yellow_any") == yellow && count("yellow_source") == if sourced { yellow } else { 0 },
    );
    checks.insert(
        "strict_red_threshold",
        count("red_any") == red && count("red_source") == if sourced { red } else { 0 },
    );
    if restore {
        checks.insert(
            "checkpoint_health_cell_owner_and_flags_preserved",
            checkpoint_state
                .as_ref()
                .is_some_and(|v| v["preserved"] == true),
        );
    }
    let has_health = |event: &str, health: i32| {
        messages
            .iter()
            .any(|m| m["event"] == event && m["health"] == health)
    };
    checks.insert(
        "source_attack_notification",
        count("attacked") == usize::from(sourced),
    );
    checks.insert(
        "first_damage_at_first_packet",
        has_health("first_any", 300 - damage),
    );
    checks.insert(
        "yellow_callback_at_strict_crossing",
        yellow == 0 || has_health("yellow_any", if damage == 75 { 75 } else { 120 }),
    );
    checks.insert(
        "red_callback_at_strict_crossing",
        red == 0 || has_health("red_any", 60),
    );
    checks.insert(
        "death_callback_after_fatal_packet",
        has_health("destroyed", 0),
    );
    checks.insert(
        "repeat_tag_retains_damage_latch",
        if sourced {
            count("latch_probe") > 1
                && has_health("latch_probe", 240)
                && has_health("latch_probe", 180)
                && has_health("latch_probe", 120)
        } else {
            count("latch_probe") == 0
        },
    );
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    if let Some(state) = &checkpoint_state {
        std::fs::write(
            root.join("checkpoint-state.json"),
            serde_json::to_vec_pretty(state)?,
        )?;
    }
    Ok(
        json!({"passed":passed,"checks":checks,"damage":damage,"sourced":sourced,"restored":restore,
        "source_house_index":source_house_index,"source_country_index":country_index,
        "damage_rows":damage_rows,"messages":messages}),
    )
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output directory must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for (name, damage, sourced, friendly) in [
        ("combat", 60, true, false),
        ("friendly", 60, true, true),
        ("source_free", 60, false, false),
        ("leap", 240, false, false),
        ("boundary", 75, false, false),
    ] {
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, damage, sourced, friendly)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            damage,
            sourced,
            false,
        )?;
        if damage == 60 {
            let restored = audit(
                &install,
                &bytes,
                &folder.join("restored"),
                damage,
                sourced,
                true,
            )?;
            let without_ticks = |values: &Value| {
                values
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["event"] != "latch_probe")
                    .map(|row| {
                        let mut row = row.clone();
                        row.as_object_mut().unwrap().remove("tick");
                        row
                    })
                    .collect::<Vec<_>>()
            };
            let agreement = without_ticks(&continuous["messages"])
                == without_ticks(&restored["messages"])
                && continuous["damage_rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| &r["health"])
                    .eq(restored["damage_rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| &r["health"]));
            let same_ticks = continuous["damage_rows"] == restored["damage_rows"]
                && continuous["messages"] == restored["messages"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"continuous_observations_match":agreement,"continuous_ticks_match":same_ticks}));
        }
        cases.push(json!({"case":name,"result":continuous}));
    }
    let passed = cases.iter().all(|case| {
        case["result"]["passed"] == true
            && case
                .get("continuous_observations_match")
                .is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS shared repeating Tag damage events from encoded player Attack commands and TS source-free weapon detonations. Map Rules fix damage/thresholds and hold targets stationary. Observers disable themselves after first notification, except latch_probe, which deliberately remains enabled. Checkpoint checks health/cell/owner/flags immediately and later ordered observer events/health; probe timing/counts, firing ticks, GUI saves, presentation and full internal state are not claimed identical.",
        "timing":"Headless snapshot load retains the current process main RNG. Later native firing jitter may shift damage frames; continuous_ticks_match records this separately from observed health/event order."}))?,
    )?;
    ensure!(
        passed,
        "damage notification audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
