//! TS result actions and team orders through the production campaign frame loop.
//! No GUI result screen, EVA playback or campaign auto-transition claim.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use std::path::PathBuf;
use vera20k_core::{
    map::map_file,
    sim::{
        house_state::HouseOutcomeKind,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, SimSoundEvent, TickLane},
    },
};

#[path = "support/campaign.rs"]
mod campaign;

struct Case {
    id: &'static str,
    command: &'static str,
    team: bool,
    expected: HouseOutcomeKind,
    skips_savour_delay: bool,
}

fn main() -> Result<()> {
    use HouseOutcomeKind::{Defeat, Victory};
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for name in ["Americans", "Russians"] {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            name,
            name,
            None,
        )?;
        doc.set_ini(name, "Country", Some(name.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [26, 40], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [44, 40], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [40, 42], "Russians", "E2")?;
    doc.commit("outcome fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    mission_script::refresh_types(&root.join("mission.ts"), &catalog)?;
    let worker = Worker::new()?;
    let cases = [
        Case {
            id: "country_win_match",
            command: "action.winCountry(m.country('Americans'))",
            team: false,
            expected: Victory,
            skips_savour_delay: false,
        },
        Case {
            id: "country_win_other",
            command: "action.winCountry(m.country('Russians'))",
            team: false,
            expected: Defeat,
            skips_savour_delay: false,
        },
        Case {
            id: "country_lose_match",
            command: "action.loseCountry(m.country('Americans'))",
            team: false,
            expected: Defeat,
            skips_savour_delay: false,
        },
        Case {
            id: "country_lose_other",
            command: "action.loseCountry(m.country('Russians'))",
            team: false,
            expected: Victory,
            skips_savour_delay: false,
        },
        Case {
            id: "announce_win",
            command: "action.winPlayer()",
            team: false,
            expected: Victory,
            skips_savour_delay: true,
        },
        Case {
            id: "announce_lose",
            command: "action.losePlayer()",
            team: false,
            expected: Defeat,
            skips_savour_delay: true,
        },
        Case {
            id: "end_without_result",
            command: "action.endScenario()",
            team: false,
            expected: Victory,
            skips_savour_delay: false,
        },
        Case {
            id: "team_win",
            command: "order.winPlayer()",
            team: true,
            expected: Victory,
            skips_savour_delay: false,
        },
        Case {
            id: "team_lose",
            command: "order.losePlayer()",
            team: true,
            expected: Defeat,
            skips_savour_delay: false,
        },
    ];
    let mut reports = Vec::new();
    for case in cases {
        let declarations = if case.team {
            format!(
                "const at=m.createWaypoint(30,40,42);\n\
                 const t=m.team('caller',{{house:m.house('Russians'),waypoint:at,units:[{{type:m.unit('E2'),count:1}}],orders:[order.guard(1),{}],full:true,reinforce:false}});\n\
                 m.rule('finish',{{owner:m.country('Russians'),when:event.sinceEnabled(1),once:true,actions:[action.recruit(t)]}});",
                case.command
            )
        } else {
            format!(
                "m.rule('finish',{{owner:m.country('Russians'),when:event.sinceEnabled(1),once:true,actions:[{}]}});",
                case.command
            )
        };
        let source = format!(
            "import {{defineMission,event,action,order}} from './sdk';import {{map}} from './map.generated';\n\
             export default defineMission(map,{{id:'{}',player:'Americans'}},m=>{{\n\
             m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});\n\
             m.configureHouse(m.house('Americans'),{{country:m.country('Americans'),playerControl:true,allies:[m.house('Russians')]}});\n\
             m.configureHouse(m.house('Russians'),{{country:m.country('Russians'),allies:[m.house('Americans')]}});\n\
             {declarations}\n}});",
            case.id
        );
        let path = root.join(format!("{}.ts", case.id));
        std::fs::write(&path, source)?;
        let map = map_file::load_from_bytes(base.clone())?;
        let mut doc = MapDocument::new(Baseline::parse(base.clone().into(), &map)?);
        let build = worker
            .build(&doc, catalog.clone(), Sources::read(&path)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?;
        build.build.apply(&mut doc)?;
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(root.join(format!("{}.map", case.id)), &bytes)?;
        for difficulty in 0..=2 {
            let mut runtime = campaign::load(&install, &bytes, difficulty)?;
            let player = runtime
                .simulation
                .interner()
                .get("Americans")
                .context("player")?;
            let caller = runtime
                .simulation
                .interner()
                .get("Russians")
                .context("caller")?;
            ensure!(runtime.simulation.session().current_house() == Some(player));
            let mut announcement_tick = None;
            let mut announcement_count = 0;
            let mut locked = false;
            let mut admitted_tick = None;
            let mut team_observed = !case.team;
            // This retail audit deliberately checks the stock 0.1 minute /
            // 90 logic frame delay; do not expose or duplicate engine internals.
            ensure!(
                (runtime.resources.rules.general.savour_delay_minutes - 0.1).abs() < 0.000001,
                "outcome audit requires stock SavourDelay=0.1; actual={}",
                runtime.resources.rules.general.savour_delay_minutes
            );
            let savour = 90u64;
            for _ in 0..600 {
                team_observed |= runtime
                    .simulation
                    .team_script_vm()
                    .teams_in_order()
                    .any(|team| team.owner() == caller && team.members().next().is_some());
                let tick = runtime.simulation.session().tick();
                let output =
                    runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
                for event in &output.sound_events {
                    if let SimSoundEvent::MatchOutcome { owner, kind } = event {
                        ensure!(
                            *owner == player && *kind == case.expected,
                            "{}: announcement target/result",
                            case.id
                        );
                        announcement_count += 1;
                        announcement_tick.get_or_insert(tick);
                    }
                }
                locked |= output.trigger_effects.iter().any(|effect| {
                    matches!(effect, TriggerEffect::InputLockChanged { locked: true, .. })
                });
                ensure!(
                    runtime.simulation.houses()[&caller].outcome().is_none(),
                    "{}: caller House must not receive player outcome",
                    case.id
                );
                ensure!(runtime.simulation.ready_outcome_for_owner(caller).is_none());
                if let Some(result) = runtime.simulation.ready_outcome_for_owner(player) {
                    ensure!(result.kind == case.expected, "{}: admitted result", case.id);
                    admitted_tick = Some(runtime.simulation.session().tick());
                    break;
                }
            }
            let announcement_tick = announcement_tick.context("missing outcome announcement")?;
            let admitted_tick = admitted_tick.with_context(|| {
                format!("{} difficulty {difficulty}: no admitted outcome", case.id)
            })?;
            ensure!(
                announcement_count == 1 && locked && team_observed,
                "{}: real caller/announcement/input lock",
                case.id
            );
            let elapsed = admitted_tick.saturating_sub(announcement_tick);
            ensure!(
                case.skips_savour_delay || elapsed >= savour,
                "{}: SavourDelay must elapse ({elapsed}/{savour})",
                case.id
            );
            reports.push(serde_json::json!({
                "case": case.id,
                "difficulty": difficulty,
                "expected_and_actual": format!("{:?}", case.expected),
                "player": "Americans", "caller": "Russians",
                "caller_house_has_no_result": true,
                "announcement_tick": announcement_tick,
                "admitted_tick": admitted_tick,
                "savour_frames": savour,
                "skips_savour_delay": case.skips_savour_delay,
                "input_locked": locked,
                "team_observed": team_observed,
            }));
            println!(
                "{} difficulty {difficulty}: {:?} ({announcement_tick}->{admitted_tick})",
                case.id, case.expected
            );
        }
    }
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": true, "cases": reports,
            "scope": "TS build, native campaign bootstrap, real team recruitment and ordinary production frames at all three campaign difficulties; result admission and emitted announcement/input lock, no GUI result screen, audio playback or automatic campaign transition claim"
        }))?,
    )?;
    Ok(())
}
