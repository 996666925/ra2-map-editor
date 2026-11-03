//! TS authoring -> native campaign publication -> ordinary frames with real assets.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::{
    campaign as package,
    mission_script::{self, Sources, Worker},
};
use std::{collections::BTreeSet, path::PathBuf};
use vera20k_core::{
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope},
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;

#[path = "support/checkpoint.rs"]
mod checkpoint;
use checkpoint::restore_checkpoint;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(
        args.next()
            .map(PathBuf::from)
            .unwrap_or("target/mission-sdk-audit".into()),
    )?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("soundmd.ini"),
        "[SoundList]\n0=SdkSound\n[SdkSound]\nSounds=sdk_sample\n",
    )?;
    std::fs::write(
        root.join("thememd.ini"),
        "[Themes]\n0=SdkTheme\n[SdkTheme]\nSound=sdk_sample\nNormal=yes\n",
    )?;
    let mut pcm = b"RIFF".to_vec();
    pcm.extend(68u32.to_le_bytes());
    pcm.extend(b"WAVEfmt ");
    pcm.extend(16u32.to_le_bytes());
    for value in [1u16, 1] {
        pcm.extend(value.to_le_bytes());
    }
    for value in [8000u32, 16000] {
        pcm.extend(value.to_le_bytes());
    }
    for value in [2u16, 16] {
        pcm.extend(value.to_le_bytes());
    }
    pcm.extend(b"data");
    pcm.extend(32u32.to_le_bytes());
    pcm.extend([0u8; 32]);
    std::fs::write(root.join("sdk_sample.wav"), pcm)?;
    let mut idx = Vec::new();
    for n in [0x41424147u32, 2, 1] {
        idx.extend(n.to_le_bytes());
    }
    let mut name = [0u8; 16];
    name[..10].copy_from_slice(b"sdk_sample");
    idx.extend(name);
    for n in [0u32, 32, 8000, 4, 0] {
        idx.extend(n.to_le_bytes());
    }
    std::fs::write(root.join("audio.idx"), idx)?;
    std::fs::write(root.join("audio.bag"), [0u8; 32])?;
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for (s, k, v) in [
        ("General", "TeamDelays", "30,30,30"),
        ("Basic", "Player", "Americans"),
        ("Basic", "MultiplayerOnly", "no"),
    ] {
        doc.set_ini(s, k, Some(v.as_bytes().to_vec()))?;
    }
    for house in ["Americans", "Russians"] {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            house,
            house,
            None,
        )?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.add_record("Structures", [26, 40], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [38, 36], "Russians", "NACNST")?;
    doc.add_record("Structures", [44, 36], "Russians", "NAPOWR")?;
    doc.add_record("Structures", [38, 44], "Russians", "NAHAND")?;
    for at in [[34, 40], [34, 42]] {
        doc.add_record("Infantry", at, "Russians", "E2")?;
    }
    doc.add_record("Units", [31, 50], "Americans", "HTK")?;
    for x in 31..36 {
        doc.add_record("Infantry", [x, 49], "Americans", "E2")?;
    }
    for (kind, positions) in [
        ("ENGINEER", [[40, 52], [41, 52]]),
        ("SENGINEER", [[44, 52], [45, 52]]),
    ] {
        for at in positions {
            doc.add_record("Infantry", at, "Americans", kind)?;
        }
    }
    let bridge_scene =
        ra2_map_io::publication::Publisher::default().scene(doc.snapshot().bytes()?, &directory)?;
    ra2_map_scene::bridge_tools::plan(
        &doc,
        &bridge_scene,
        [20, 34],
        [28, 34],
        ra2_map_scene::bridge_tools::BridgeKind::LowWood,
    )?
    .apply(&mut doc)?;
    drop(bridge_scene);
    doc.commit("fixture");
    for name in ["one.map", "two.map"] {
        std::fs::write(root.join(name), doc.snapshot().bytes()?)?;
    }
    std::fs::write(
        root.join("campaign.ts"),
        r#"import {defineCampaign} from './sdk';
export default defineCampaign({id:'SDK_AUDIT',entry:'one.map',missions:[
 {file:'one.map',source:'one.map',script:'first/mission.ts',title:'第一关：自动组队',briefing:'等待援军完成集结',next:'two.map'},
 {file:'two.map',source:'two.map',script:'second/mission.ts',title:'第二关：共享状态',endOfGame:true}
]});"#,
    )?;
    std::fs::write(
        root.join("shared.ts"),
        "export const ReadyFlag=49;export const FinishedFlag=48;export const CarryFlag=47;",
    )?;
    let first_source = r#"import {defineMission,event,action,order,Crate,AiTriggerCondition,AiTriggerComparison} from './sdk';
import {map} from './map.generated';
import {ReadyFlag,FinishedFlag} from './shared';
export default defineMission(map,{id:'audit_one',player:'Americans'},m=>{
 m.asset('soundmd.ini','soundmd.ini');m.asset('thememd.ini','thememd.ini');m.asset('sdk_sample.wav','sdk_sample.wav');
 m.asset('audio.idx','audio.idx');m.asset('audio.bag','audio.bag');
 m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,credits:100,allies:[m.house('Americans'),m.house('Russians')]});
 m.configureHouse(m.house('Russians'),{country:m.country('Russians'),credits:10000,techLevel:10,iq:5,allies:[m.house('Americans'),m.house('Russians')],basePlan:{percentBuilt:100,nodes:[
  {kind:'building',building:m.building('NACNST'),x:38,y:36},
  {kind:'building',building:m.building('NAPOWR'),x:44,y:36},
  {kind:'building',building:m.building('NAHAND'),x:38,y:44,rebuild:true}]}});
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 const at=m.createWaypoint(30,34,46);
 const shared=m.globalFlag(ReadyFlag), result=m.globalFlag(FinishedFlag);
 const title=m.defineText('wave','援军集结倒计时');
 const team=m.team('ai_team',{house:m.house('Russians'),waypoint:at,units:[{type:m.unit('E2'),count:2}],orders:[order.moveTo(at),order.setGlobal(shared),order.succeed()],isBaseDefense:true,max:1,reinforce:false});
 const reinforcementAt=m.createWaypoint(34,46,50);
 const afterChange=m.script('after_change',[order.setGlobal(m.globalFlag(29)),order.waitForever()]);
 const ordinary=m.team('ordinary',{house:m.house('Americans'),waypoint:reinforcementAt,units:[{type:m.unit('E1'),count:1}],orders:[order.setGlobal(m.globalFlag(34)),order.waitForever()]});
 const chrono=m.team('chrono',{house:m.house('Americans'),waypoint:reinforcementAt,units:[{type:m.unit('MTNK'),count:1}],orders:[order.setGlobal(m.globalFlag(33)),order.changeScript(afterChange)]});
 const killed=m.team('killed',{house:m.house('Americans'),waypoint:reinforcementAt,units:[{type:m.unit('ENGINEER'),count:2}],orders:[order.setGlobal(m.globalFlag(28)),order.jump('finish'),order.setGlobal(m.globalFlag(27)),order.label('finish'),order.next(),order.setGlobal(m.globalFlag(26)),order.guard(2),order.killMembers()],priority:40,max:1,full:true,reinforce:false});
 const removed=m.team('removed',{house:m.house('Americans'),waypoint:reinforcementAt,units:[{type:m.unit('SENGINEER'),count:2}],orders:[order.setGlobal(m.globalFlag(23)),order.guard(2),order.removeMembers()],priority:40,max:1,full:true,reinforce:false});
 m.rule('recruit_killed',{when:event.sinceEnabled(2),once:true,actions:[action.recruit(killed)]});
 m.rule('recruit_removed',{when:event.sinceEnabled(8),once:true,actions:[action.recruit(removed)]});
 // Repeated shared Tags preserve the distinct source-gated and unconditional
 // notifications. Script31 has no attacker; Script37 makes no RecordKill call.
 m.rule('killed_death',{when:event.destroyed(m.object('Infantry:7')),once:false,linked:m.ruleRef('killed_removed'),actions:[action.setGlobal(m.globalFlag(25)),action.disableRule(m.ruleRef('killed_death'))]});
 m.rule('killed_removed',{when:event.destroyedOrRemoved(m.object('Infantry:7')),once:false,linked:m.ruleRef('killed_any'),actions:[action.setGlobal(m.globalFlag(24)),action.disableRule(m.ruleRef('killed_removed'))]});
 m.rule('killed_any',{when:event.destroyedAny(m.object('Infantry:7')),once:false,actions:[action.setGlobal(m.globalFlag(20)),action.disableRule(m.ruleRef('killed_any'))]});
 m.rule('removed_death',{when:event.destroyed(m.object('Infantry:9')),once:false,linked:m.ruleRef('removed_removed'),actions:[action.setGlobal(m.globalFlag(22)),action.disableRule(m.ruleRef('removed_death'))]});
 m.rule('removed_removed',{when:event.destroyedOrRemoved(m.object('Infantry:9')),once:false,linked:m.ruleRef('removed_any'),actions:[action.setGlobal(m.globalFlag(21)),action.disableRule(m.ruleRef('removed_removed'))]});
 m.rule('removed_any',{when:event.destroyedAny(m.object('Infantry:9')),once:false,actions:[action.setGlobal(m.globalFlag(19)),action.disableRule(m.ruleRef('removed_any'))]});
 m.rule('ordinary_entry',{when:event.sinceEnabled(3),once:true,actions:[action.reinforceDefault(ordinary)]});
 m.rule('chrono_entry',{when:event.sinceEnabled(5),once:true,actions:[action.chronoReinforce(chrono,reinforcementAt)]});
 m.rule('flash_entries',{when:event.all(event.globalSet(m.globalFlag(34)),event.globalSet(m.globalFlag(33))),once:true,actions:[action.flashTeam(ordinary,90),action.flashTeam(chrono,90),action.setGlobal(m.globalFlag(32))]});
 m.rule('disband_entries',{when:event.all(event.globalSet(m.globalFlag(32)),event.sinceEnabled(18)),once:true,actions:[action.deleteTeam(ordinary),action.deleteTeam(chrono),action.allToHunt(m.country('Americans')),action.setGlobal(m.globalFlag(31))]});
 const cue=m.createWaypoint(35,49,55);
 m.rule('map_cues',{when:event.sinceEnabled(4),once:true,actions:[action.dropZone(cue),action.createCrate(Crate.Money,cue)]});
 const transfer=m.rule('transfer_power',{when:event.sinceEnabled(10),once:false,linked:m.ruleRef('return_power'),actions:[action.changeAttachedOwner(m.country('Russians')),action.disableRule(m.ruleRef('transfer_power'))]});
 m.rule('return_power',{when:event.sinceEnabled(12),once:false,actions:[action.changeAttachedOwner(m.country('Americans')),action.disableRule(m.ruleRef('return_power'))]});
 m.bindObject(m.object('Structures:0'),m.tag('power_transfer',{rule:transfer}));
 m.aiTrigger('select',{primary:team,owner:m.country('Russians'),condition:AiTriggerCondition.OwnsObjects,objectType:m.techno('E2'),comparison:AiTriggerComparison.GreaterOrEqual,amount:2,weight:5000,minWeight:5000,maxWeight:5000});
 m.rule('activate',{when:event.sinceEnabled(1),once:true,owner:m.country('Russians'),actions:[action.startProduction(m.country('Russians')),action.enableAi(m.country('Russians')),action.setCountdown(30),action.setCountdownTitle(title),action.playSound(m.assetSound('SdkSound')),action.playMusic(m.assetTheme('SdkTheme'))]});
 const target=m.object(map.objects.find(id=>id.startsWith('Structures:') && id==='Structures:3')!);
 m.rule('destroy_barracks',{when:event.sinceEnabled(20),once:true,actions:[action.destroyAttached()]});
 m.bindObject(target,m.tag('barracks',{rule:m.ruleRef('destroy_barracks')}));
 m.rule('finish',{when:event.all(event.globalSet(shared),event.sinceEnabled(40)),once:true,actions:[action.setGlobal(result)]});
});"#;
    std::fs::write(root.join("one.ts"), first_source)?;
    for directory in ["first", "second"] {
        std::fs::create_dir(root.join(directory))?;
    }
    // Keep declarations relative to the nested mission, including its media.
    for name in [
        "soundmd.ini",
        "thememd.ini",
        "sdk_sample.wav",
        "audio.idx",
        "audio.bag",
    ] {
        std::fs::copy(root.join(name), root.join("first").join(name))?;
    }
    std::fs::write(
        root.join("first/mission.ts"),
        first_source.replace("from './", "from '../"),
    )?;
    std::fs::write(
        root.join("second/mission.ts"),
        r#"import {defineMission,event,action,order} from '../sdk';import {map} from '../map.generated';
import {ReadyFlag,CarryFlag} from '../shared';
export default defineMission(map,{id:'audit_two',player:'Americans'},m=>{
 const landing=m.createWaypoint(31,40,48);
 const pickup=m.createWaypoint(32,31,50);
 // Exercise SDK bindings through real movement and passenger egress, without
 // injecting trigger notifications. One shared CellTag watches the landing.
 const arrived=m.rule('entered_landing',{when:event.entered(m.country('Americans')),once:true,actions:[action.setGlobal(m.globalFlag(42))]});
 const landingTag=m.tag('landing',{rule:arrived});
 for(let x=38;x<=42;x++)for(let y=46;y<=50;y++)m.bindCell(x,y,landingTag);
 m.rule('passenger_near',{when:event.nearWaypoint(m.object('Infantry:2'),landing),once:true,actions:[action.setGlobal(m.globalFlag(41))]});
 m.rule('type_present',{when:event.technoExists(m.techno('NACNST')),once:true,actions:[action.setGlobal(m.globalFlag(40))]});
 m.rule('type_absent',{when:event.technoAbsent(m.techno('AMCV')),once:true,actions:[action.setGlobal(m.globalFlag(39))]});
 const zone=m.rule('same_zone',{when:event.enteredZone(m.country('Americans')),once:true,actions:[action.setGlobal(m.globalFlag(38))]});
 m.bindCell(25,44,m.tag('zone',{rule:zone}));
 const otherZone=m.rule('other_country_zone',{when:event.enteredZone(m.country('French')),once:true,actions:[action.setGlobal(m.globalFlag(37))]});
 m.bindCell(24,44,m.tag('other_zone',{rule:otherZone}));
 const bridge=m.createWaypoint(33,24,34);
 const bridgeRule=m.rule('bridge_collapse',{when:event.bridgeDestroyed(),once:true,actions:[action.setGlobal(m.globalFlag(36))]});
 // Native Event8 also registers for ordinary Logic visits, without an attachment.
 m.rule('native_any',{when:event.tagNotification(),once:true,actions:[action.setGlobal(m.globalFlag(35))]});
 m.bindCell(24,34,m.tag('bridge',{rule:bridgeRule}));
 m.rule('damage_bridge',{when:event.sinceEnabled(3),once:true,actions:[action.damageBridgeAt(bridge)]});
 const convoy=m.team('convoy',{house:m.house('Americans'),waypoint:pickup,units:[{type:m.unit('HTK'),count:1},{type:m.unit('E2'),count:5}],orders:[order.load(),order.waitLoaded(),order.moveTo(landing),order.unload(),order.setGlobal(m.globalFlag(46)),order.succeed()],priority:30,max:1,full:true,reinforce:false});
 const curtain=m.superWeapon('IronCurtainSpecial');
 m.rule('super_ready',{when:event.sinceEnabled(1),once:true,actions:[action.grantSuperWeapon(curtain,false),action.setSuperRecharge(curtain,300),action.setSuperCharge(curtain,100),action.setSuperTarget(landing),action.setSuperDefense(landing)]});
 const transport=m.object('Units:0');
 m.rule('selected',{when:event.all(event.selected(transport),event.globalSet(m.globalFlag(46))),once:true,linked:m.ruleRef('selected_follow'),actions:[action.resetSuperRecharge(curtain),action.resetSuperWeapon(curtain),action.clearSuperTarget(),action.clearSuperDefense(),action.setGlobal(m.globalFlag(45))]});
 m.rule('selected_follow',{when:event.selected(transport),once:true,actions:[action.setGlobal(m.globalFlag(44))]});
 m.rule('selected_latch',{when:event.all(event.selected(m.object('Structures:0')),event.globalSet(m.globalFlag(46))),once:false,actions:[action.setGlobal(m.globalFlag(43)),action.disableRule(m.ruleRef('selected_latch'))]});
 m.rule('recruit_convoy',{when:event.sinceEnabled(1),once:true,actions:[action.recruit(convoy)]});
 m.rule('carry',{when:event.globalSet(m.globalFlag(ReadyFlag)),once:true,actions:[action.setGlobal(m.globalFlag(CarryFlag))]});
});"#,
    )?;
    let mut install = Install::open(&directory)?;
    let scene =
        ra2_map_scene::map::load_scene(&mut install, root.join("one.map").to_str().unwrap())?;
    let catalog = mission_script::catalog(&doc, &scene);
    mission_script::refresh_types(&root.join("one.ts"), &catalog)?;
    let worker = Worker::new()?;
    let built = worker
        .build(&doc, catalog.clone(), Sources::read(&root.join("one.ts"))?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    built.build.apply(&mut doc)?;
    let built = worker
        .build(&doc, catalog, Sources::read(&root.join("one.ts"))?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    ensure!(!built.build.apply(&mut doc)?, "unchanged rebuild");
    let prepared = mission_script::prepare_campaign(&root.join("campaign.ts"), &directory)?;
    ensure!(
        prepared.report.passed,
        "dependencies: {}",
        serde_json::to_string(&prepared.report)?
    );
    let first = prepared.payload["one.map"].clone();
    let second = prepared.payload["two.map"].clone();
    let report = package::export(prepared, &root.join("package"))?;
    let published_scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("package/one.map").to_str().unwrap(),
    )?;
    let published_map = map_file::load_from_bytes(first.clone())?;
    let mut reopened = MapDocument::new(Baseline::parse(first.clone().into(), &published_map)?);
    let published_catalog = mission_script::catalog(&reopened, &published_scene);
    let rebuilt = worker
        .build(
            &reopened,
            published_catalog,
            Sources::read_for_document(&reopened, &root.join("first/mission.ts"))?,
        )?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        !rebuilt.build.apply(&mut reopened)?,
        "reopened campaign rebuild"
    );
    ensure!(reopened.ini_text("Basic", "NextScenario") == "two.map");
    mission_script::resource_payload(&first, &published_scene.revision.id())?;
    package::install_clean(
        &root.join("package"),
        &directory,
        &root.join("clean-install"),
    )?;
    let clean = Install::open(&root.join("clean-install"))?;
    let mut runtime = campaign::load(&clean, &first, 1)?;
    let script_identity = |name: &str| -> Result<_> {
        let id = doc
            .ini_entries("ScriptTypes")
            .into_iter()
            .map(|(_, _, value)| String::from_utf8_lossy(&value).into_owned())
            .find(|id| doc.ini_text(id, "Name") == name)
            .with_context(|| format!("script identity for {name}"))?;
        runtime
            .simulation
            .interner()
            .get(&id)
            .context("installed script identity")
    };
    let chrono_original_script = script_identity("chrono")?;
    let chrono_replacement_script = script_identity("after_change")?;
    let mut members = BTreeSet::new();
    let mut rebuilt = false;
    let mut sound = false;
    let mut theme = false;
    let mut ordinary_members = BTreeSet::new();
    let mut chrono_members = BTreeSet::new();
    let mut flashed_members = BTreeSet::new();
    let mut hunted_members = BTreeSet::new();
    let mut chrono_scripts = BTreeSet::new();
    let mut changed_script_stalled = false;
    let mut drop_zone_created = false;
    let mut visible_crate = None;
    let mut saw_power_transfer = false;
    let american = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("lifecycle owner")?;
    let losses_before = runtime.simulation.houses()[&american].stats().units_lost();
    let mut killed_members = BTreeSet::new();
    let mut removed_members = BTreeSet::new();
    let mut losses_after_kill = None;
    let mut losses_after_remove = None;
    let power_id = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| runtime.simulation.interner().resolve(entity.type_ref()) == "GAPOWR")
        .map(|(id, _)| id)
        .context("transfer power plant")?;
    let initial = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "NAHAND")
        .map(|(id, _)| id)
        .context("barracks")?;
    for _ in 0..6000 {
        for team in runtime.simulation.team_script_vm().teams_in_order() {
            for id in team.members() {
                let entity = runtime
                    .simulation
                    .entities()
                    .get(id)
                    .context("team member")?;
                match runtime.simulation.interner().resolve(entity.type_ref()) {
                    "E2" if runtime.simulation.interner().resolve(entity.owner()) == "Russians" => {
                        members.insert(id);
                    }
                    "E1" => {
                        ordinary_members.insert(id);
                    }
                    "MTNK" => {
                        chrono_members.insert(id);
                        chrono_scripts.insert(team.script_id());
                        changed_script_stalled |= chrono_scripts.contains(&chrono_original_script)
                            && team.script_id() == chrono_replacement_script
                            && team.cursor() == -1;
                    }
                    "ENGINEER" => {
                        killed_members.insert(id);
                    }
                    "SENGINEER" => {
                        removed_members.insert(id);
                    }
                    _ => {}
                }
            }
        }
        let losses = runtime.simulation.houses()[&american].stats().units_lost();
        if killed_members.len() == 2
            && killed_members
                .iter()
                .all(|&id| runtime.simulation.entities().get(id).is_none())
        {
            losses_after_kill.get_or_insert(losses);
        }
        if removed_members.len() == 2
            && removed_members
                .iter()
                .all(|&id| runtime.simulation.entities().get(id).is_none())
        {
            losses_after_remove.get_or_insert(losses);
        }
        for &id in ordinary_members.iter().chain(&chrono_members) {
            if let Some(entity) = runtime.simulation.entities().get(id) {
                if entity.flashing.frames() > 0 {
                    flashed_members.insert(id);
                }
                if runtime.simulation.global_variables()[31]
                    && entity.mission.current().known()
                        == Some(vera20k_core::sim::mission::MissionType::Hunt)
                {
                    hunted_members.insert(id);
                }
            }
        }
        saw_power_transfer |= runtime
            .simulation
            .entities()
            .get(power_id)
            .is_some_and(|entity| {
                runtime.simulation.interner().resolve(entity.owner()) == "Russians"
            });
        drop_zone_created |= runtime.simulation.anims().any(|(_, anim)| {
            anim.no_effects()
                && Some(runtime.simulation.interner().resolve(anim.type_id()))
                    == runtime.resources.rules.general.drop_zone_anim.as_deref()
                && anim.world_coord().x / 256 == 49
                && anim.world_coord().y / 256 == 55
        });
        if let Some(grid) = runtime.simulation.overlay_grid() {
            for slot in runtime
                .simulation
                .crate_authority()
                .slots()
                .iter()
                .filter(|slot| !slot.is_empty())
            {
                let cell = grid.cell(slot.cell_x as u16, slot.cell_y as u16);
                if cell.overlay_id.is_some() && cell.overlay_data == 0 {
                    visible_crate = Some((slot.cell_x, slot.cell_y, cell.overlay_id));
                }
            }
        }
        rebuilt |= runtime.simulation.entities().iter_sorted().any(|(id, e)| {
            id != initial
                && runtime.simulation.interner().resolve(e.type_ref()) == "NAHAND"
                && e.health.current() > 0
                && !e.lifecycle.in_limbo()
        });
        if runtime.simulation.global_variables()[48] && rebuilt {
            break;
        }
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        sound|=output.sound_events.iter().any(|e|matches!(e,vera20k_core::sim::world::SimSoundEvent::VocCentered{sound_id} if sound_id=="SdkSound"));
        theme |= output.trigger_effects.iter().any(|e| {
            matches!(
                e,
                vera20k_core::sim::trigger_runtime::TriggerEffect::MusicTheme { .. }
            )
        });
    }
    ensure!(
        runtime.simulation.global_variables()[49]
            && runtime.simulation.global_variables()[48]
            && members.len() >= 2
            && rebuilt,
        "AI selection/script/base reconstruction did not finish"
    );
    ensure!(sound && theme, "published custom sound/music requests");
    let lifecycle_flags = &runtime.simulation.global_variables()[21..29];
    ensure!(
        killed_members.len() == 2
            && removed_members.len() == 2
            && losses_after_kill == Some(losses_before + 2)
            && losses_after_remove == losses_after_kill
            && lifecycle_flags == [false, false, true, true, false, true, false, true]
            && runtime.simulation.global_variables()[20]
            && !runtime.simulation.global_variables()[19],
        "label jump/next and kill/remove lifecycle must differ in losses and notifications (killed={killed_members:?}, removed={removed_members:?}, losses={losses_before}/{losses_after_kill:?}/{losses_after_remove:?}, flags21..28={lifecycle_flags:?})"
    );
    ensure!(
        visible_crate.is_some()
            && saw_power_transfer
            && runtime
                .simulation
                .entities()
                .get(power_id)
                .is_some_and(
                    |entity| runtime.simulation.interner().resolve(entity.owner()) == "Americans"
                ),
        "visible fixed Money crate and attached ownership round trip must execute (crate={visible_crate:?}, transfer={saw_power_transfer})"
    );
    let reinforcement_members: BTreeSet<_> =
        ordinary_members.union(&chrono_members).copied().collect();
    ensure!(
        ordinary_members.len() == 1
            && chrono_members.len() == 1
            && flashed_members == reinforcement_members
            && hunted_members == reinforcement_members
            && runtime.simulation.global_variables()[31..35]
                .iter()
                .all(|set| *set)
            && changed_script_stalled
            && !runtime.simulation.global_variables()[29]
            && reinforcement_members.iter().all(|&id| runtime
                .simulation
                .entities()
                .get(id)
                .is_some_and(|entity| entity.is_active()))
            && runtime
                .simulation
                .team_script_vm()
                .teams_in_order()
                .all(|team| team
                    .members()
                    .all(|id| !reinforcement_members.contains(&id))),
        "ordinary/chrono entry, script replacement, flash, disband preserving members and hunt failed (ordinary={ordinary_members:?}, chrono={chrono_members:?}, flashed={flashed_members:?}, hunted={hunted_members:?}, script_stalled={changed_script_stalled}, flags29..34={:?})",
        &runtime.simulation.global_variables()[29..35]
    );
    let incoming = runtime.simulation.save_campaign_carry_over();
    let serialized = serde_json::to_vec(&incoming)?;
    let incoming = serde_json::from_slice(&serialized)?;
    let mut next = campaign::load_stage(&clean, &second, 1, "two.map", 2, Some(incoming), None)?;
    let bridge_before = next
        .simulation
        .overlay_grid()
        .context("initial bridge overlays")?
        .cell(24, 34)
        .overlay_id;
    let owner = next
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let power = next
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| next.simulation.interner().resolve(entity.type_ref()) == "GAPOWR")
        .map(|(id, _)| id)
        .context("power")?;
    next.advance_frame(
        &[CommandEnvelope::new(
            owner,
            next.simulation.session().tick(),
            Command::Select {
                entity_ids: vec![power],
                additive: false,
            },
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    next.advance_frame(
        &[CommandEnvelope::new(
            owner,
            next.simulation.session().tick(),
            Command::Select {
                entity_ids: vec![],
                additive: false,
            },
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    ensure!(
        !next.simulation.global_variables()[43],
        "notification must wait for the second condition"
    );
    ensure!(
        next.simulation.global_variables()[35] && !next.simulation.global_variables()[36],
        "native Event8 must fire during ordinary Logic visits, before bridge destruction"
    );
    let mut passengers_loaded = 0;
    let mut checkpoint_bytes = None;
    for _ in 0..6000 {
        for (_, entity) in next.simulation.entities().iter_sorted() {
            if next.simulation.interner().resolve(entity.type_ref()) == "HTK" {
                passengers_loaded = passengers_loaded.max(entity.passenger_role.passenger_count());
            }
        }
        if passengers_loaded == 5 && checkpoint_bytes.is_none() {
            checkpoint_bytes = Some(restore_checkpoint(
                &mut next,
                &second,
                &root.join("transport-checkpoint.bin"),
            )?);
        }
        next.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        if next.simulation.global_variables()[46] {
            break;
        }
    }
    ensure!(
        next.simulation.global_variables()[47],
        "cross-stage global state"
    );
    ensure!(
        passengers_loaded == 5
            && checkpoint_bytes.is_some()
            && next.simulation.global_variables()[46],
        "recruited transport must fully load, move, unload and advance its script (loaded {passengers_loaded})"
    );
    let (transport_id, transport) = next
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| next.simulation.interner().resolve(entity.type_ref()) == "HTK")
        .context("transport")?;
    let coord = vera20k_core::sim::movement::ground_pose::position_world_coord(&transport.position);
    let transport_cell = [coord.x / 256, coord.y / 256];
    ensure!(
        transport.passenger_role.passenger_count() == 0
            && (transport_cell[0] - 40).pow(2) + (transport_cell[1] - 48).pow(2) < 25,
        "transport must finish empty near the landing waypoint: {transport_cell:?}"
    );
    let owner = next
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let curtain = next
        .simulation
        .interner()
        .get("IronCurtainSpecial")
        .context("curtain")?;
    let instance = &next.simulation.super_weapons()[&owner][&curtain];
    ensure!(
        instance.is_active() && instance.is_ready(),
        "TS-granted curtain must be ready"
    );
    let launch = next
        .simulation
        .encode_superweapon_record(
            owner,
            &Command::LaunchSuperWeapon {
                sw_type_id: curtain,
                target_rx: transport_cell[0].try_into()?,
                target_ry: transport_cell[1].try_into()?,
            },
            &next.resources.rules,
        )
        .context("launch record")?;
    let output = next.advance_frame(
        &[CommandEnvelope::synchronized(
            launch,
            next.simulation.session().tick(),
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    ensure!(output.sound_events.iter().any(|effect| matches!(effect,
        vera20k_core::sim::world::SimSoundEvent::SuperWeaponLaunched { owner: fired_by, sw_type, .. }
            if *fired_by == owner && *sw_type == curtain
    )), "ordinary player command must launch the granted curtain");
    let transport = next
        .simulation
        .entities()
        .get(transport_id)
        .context("curtained transport")?;
    ensure!(
        transport
            .invulnerability
            .as_ref()
            .is_some_and(|state| state.kind()
                == vera20k_core::sim::superweapon::invulnerability::InvulnKind::IronCurtain),
        "transport must receive Iron Curtain"
    );
    let instance = &next.simulation.super_weapons()[&owner][&curtain];
    ensure!(
        !instance.is_ready() && instance.charge_duration() == 300,
        "TS recharge override must survive a launch"
    );
    next.advance_frame(
        &[CommandEnvelope::new(
            owner,
            next.simulation.session().tick(),
            Command::Select {
                entity_ids: vec![transport_id],
                additive: false,
            },
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    for _ in 0..30 {
        if next.simulation.global_variables()[43]
            && next.simulation.global_variables()[44]
            && next.simulation.global_variables()[45]
        {
            break;
        }
        next.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    ensure!(
        next.simulation.global_variables()[43]
            && next.simulation.global_variables()[44]
            && next.simulation.global_variables()[45],
        "latched selection notification across checkpoint, AND condition and linked rule must execute (flags43/44/45={:?}, tick={})",
        &next.simulation.global_variables()[43..46],
        next.simulation.session().tick()
    );
    ensure!(
        next.simulation.super_weapons()[&owner][&curtain].charge_duration()
            == next
                .resources
                .rules
                .super_weapon("IronCurtainSpecial")
                .context("curtain type")?
                .recharge_time_frames,
        "TS reset must restore the original recharge duration"
    );
    for _ in 0..150 {
        if next.simulation.global_variables()[39..43]
            .iter()
            .all(|set| *set)
        {
            break;
        }
        next.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    ensure!(
        next.simulation.global_variables()[39..43]
            .iter()
            .all(|set| *set),
        "type existence, passenger proximity and shared landing CellTag must trigger through ordinary frames (flags39..42={:?})",
        &next.simulation.global_variables()[39..43]
    );
    let bridge_after = next
        .simulation
        .overlay_grid()
        .context("collapsed bridge overlays")?
        .cell(24, 34)
        .overlay_id;
    ensure!(
        next.simulation.global_variables()[35..37]
            .iter()
            .all(|set| *set)
            && !next.simulation.global_variables()[37]
            && next.simulation.global_variables()[38]
            && bridge_before != bridge_after,
        "zone arrival, country rejection, native Any and real bridge collapse must execute (flags35..38={:?}, bridge={bridge_before:?}->{bridge_after:?})",
        &next.simulation.global_variables()[35..39]
    );
    std::fs::write(root.join("first/mission.ts"), "changed")?;
    ensure!(
        mission_script::prepare_campaign(&root.join("campaign.ts"), &directory).is_err(),
        "broken mission must prevent publication"
    );
    let lifecycle_report = serde_json::json!({
        "label_jump_skips_command_and_next_advances": true,
        "killed_members": killed_members,
        "removed_members": removed_members,
        "losses_before": losses_before,
        "losses_after_kill": losses_after_kill,
        "losses_after_remove": losses_after_remove,
        "null_source_kill_publishes_48_and_29_without_7": true,
        "scripted_remove_publishes_none_of_7_48_29": true,
    });
    let mut audit_report = serde_json::json!({
            "passed": drop_zone_created,
            "implemented_checks_passed": true,
        "ai_members": members,
        "base_rebuilt": rebuilt,
        "global_carry": true,
        "unchanged_rebuild": true,
        "reopened_campaign_rebuild": true,
        "broken_campaign_rejected": true,
        "custom_sound_request": sound,
        "custom_music_request": theme,
            "ordinary_reinforcement_members": ordinary_members,
            "chrono_reinforcement_members": chrono_members,
            "reinforcements_flashed_disbanded_alive_and_hunting": true,
            "change_script_replaces_and_holds_minus_one_cursor": changed_script_stalled,
            "member_lifecycle": lifecycle_report,
            "drop_zone_animation_created": drop_zone_created,
            "engine_limitations": Vec::<&str>::new(),
            "visible_money_crate": visible_crate,
            "attached_ownership_round_trip": saw_power_transfer,
        "clean_install": true,
        "nested_missions_shared_module": true,
        "mission_relative_media": true,
        "transport_recruited_loaded_moved_unloaded": true,
        "passengers_loaded": passengers_loaded,
        "transport_cell": transport_cell,
        "granted_super_player_launch_and_recharge_reset": true,
        "selected_object_and_linked_rule": true,
        "latched_notification_across_checkpoint": true,
        "shared_landing_cell_tag": next.simulation.global_variables()[42],
        "passenger_near_waypoint": next.simulation.global_variables()[41],
        "type_present": next.simulation.global_variables()[40],
        "type_absent": next.simulation.global_variables()[39],
            "zone_entry": next.simulation.global_variables()[38],
            "zone_other_country_rejected": !next.simulation.global_variables()[37],
            "bridge_collapse_event": next.simulation.global_variables()[36],
            "native_any_from_logic": next.simulation.global_variables()[35],
            "bridge_overlay_before": bridge_before,
            "bridge_overlay_after": bridge_after,
        "checkpoint_bytes": checkpoint_bytes,
        "publication": report,
        "scope": "ordinary campaign frames with installed resources and headless checkpoint validators; no GUI save loading, presentation or audible playback claim"
    });
    audit_report["failed_checks"] = serde_json::json!(if drop_zone_created {
        Vec::<&str>::new()
    } else {
        vec![
            "Drop-zone animation was not observed; inspect authored animation bindings, assets and trigger execution.",
        ]
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&audit_report)?,
    )?;
    ensure!(
        drop_zone_created,
        "drop-zone animation must construct from the authored action asset roots"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
