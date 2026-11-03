//! Build TS-authored inputs for the fixed client's tactical/campaign capture routes.
//! This prepares fixtures only; a successful GPU capture is separate evidence.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
use ra2_map_io::{mission_script, publication::Publisher};
use serde_json::json;
use std::path::PathBuf;
use vera20k_core::skirmish_launch::{
    LaunchCountry, LaunchHumanRole, LaunchStartPosition, LaunchTeam, PreFillHouseRoster,
    SkirmishHumanSlot, SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession,
    SkirmishLocalSlot,
};
#[path = "support/campaign.rs"]
mod campaign;

fn audit_intro_runtime(install: &Install, bytes: &[u8], root: &std::path::Path) -> Result<()> {
    use vera20k_core::sim::world::{FrameEffects, TickLane};
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = runtime
        .simulation
        .entities()
        .iter_sorted()
        .map(|(id, e)| {
            (
                id,
                e.type_ref(),
                e.owner(),
                e.health.current(),
                runtime.simulation.object_coords_cell(id),
            )
        })
        .collect::<Vec<_>>();
    let mut actors_preserved = true;
    let mut observations = Vec::new();
    let mut messages = Vec::new();
    let texts = mission_script::declared_texts(bytes)?;
    let mut timer_preserved = true;
    for _ in 0..150 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        actors_preserved &= initial.iter().all(|(id, ty, owner, health, cell)| {
            sim.entities().get(*id).is_some_and(|e| {
                e.type_ref() == *ty
                    && e.owner() == *owner
                    && e.health.current() == *health
                    && e.lifecycle.object_alive()
                    && sim.object_coords_cell(*id) == *cell
            })
        });
        let effects = output
            .trigger_effects
            .iter()
            .map(|e| format!("{e:?}"))
            .collect::<Vec<_>>();
        for effect in output.trigger_effects {
            if let vera20k_core::sim::trigger_runtime::TriggerEffect::LocalizedMessage {
                csf_label,
            } = effect
            {
                messages.push(json!({"tick":sim.session().tick(),"text":texts.get(&csf_label).context("declared intro message")?}));
            }
        }
        let timer = sim.session().scenario_timer_view();
        timer_preserved &= timer.as_ref().is_some_and(|(frames, label)| {
            i64::from(*frames) == 450 - sim.session().tick() as i64
                && texts
                    .get(*label)
                    .is_some_and(|s| s == "SDK intro countdown")
        });
        observations.push(json!({"tick":sim.session().tick(),"effects":effects,"timer":timer}));
    }
    let checks = json!({"authored_landmark_survives":initial.len()==1 && actors_preserved,
        "two_task_messages":messages.iter().map(|m|m["text"].as_str()).collect::<Vec<_>>()==[Some("SDK intro finished"),Some("SDK intro running")],
        "first_task_message_at_first_logic_frame":messages.first().is_some_and(|m|m["tick"]==1),
        "second_task_message_at_31":messages.get(1).is_some_and(|m|m["tick"]==31),
        "caption_and_timer_run_for_all_150_frames":timer_preserved});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("intro-runtime.json"),
        serde_json::to_vec_pretty(&json!({
        "passed":passed,"checks":checks,"messages":messages,"observations":observations,
        "scope":"Independent headless ordinary-frame task rules and original landmark only; headless loading does not present Intro and does not prove movie playback or pre-playback logic gating."}))?,
    )?;
    ensure!(passed, "intro task runtime audit failed");
    Ok(())
}

fn write_talk_shape(install: &Install, root: &std::path::Path) -> Result<()> {
    let palette = vera20k_core::assets::pal_file::Palette::from_bytes(
        install
            .manager
            .get_ref("palette.pal")
            .context("bubble palette")?,
    )?;
    let colors = (1u8..=255)
        .map(|index| {
            let color = palette.colors()[usize::from(index)];
            (
                index,
                u32::from(color.r) + u32::from(color.g) + u32::from(color.b),
            )
        })
        .collect::<Vec<_>>();
    let white = colors.iter().max_by_key(|(_, light)| light).unwrap().0;
    let black = colors.iter().min_by_key(|(_, light)| light).unwrap().0;
    let mut bytes = Vec::new();
    for value in [0u16, 24, 64, 3] {
        bytes.extend(value.to_le_bytes());
    }
    for frame in 0..3u32 {
        for value in [0u16, 0, 24, 16] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0u8; 12]);
        bytes.extend((8 + 3 * 24 + frame * 24 * 16).to_le_bytes());
    }
    for frame in 0..3 {
        let mut pixels = vec![0u8; 24 * 16];
        for y in 1..13 {
            for x in 1..23 {
                pixels[y * 24 + x] = white;
            }
        }
        for y in 13..16 {
            for x in 10..13 {
                pixels[y * 24 + x] = white;
            }
        }
        for dot in 0..=frame {
            for y in 5..9 {
                for x in 5 + dot * 5..8 + dot * 5 {
                    pixels[y * 24 + x] = black;
                }
            }
        }
        bytes.extend(pixels);
    }
    // Parse the actual file before publishing it through m.asset().
    let shape = vera20k_core::assets::shp_file::ShpFile::from_bytes(&bytes)?;
    for frame in 0..3 {
        let rgba = shape.frame_to_rgba(frame, &palette)?;
        image::RgbaImage::from_raw(24, 16, rgba)
            .context("bubble image size")?
            .save(root.join(format!("talk-shape-{frame}.png")))?;
    }
    std::fs::write(root.join("talkbubl.shp"), bytes)?;
    Ok(())
}

fn audit_talk_bubbles(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    control: bool,
    expiry: bool,
) -> Result<()> {
    use vera20k_core::sim::{
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    };
    let mut runtime = campaign::load(install, bytes, 1)?;
    let actor = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (runtime.simulation.interner().resolve(entity.type_ref()) == "E1").then_some(id)
        })
        .context("authored bubble actor")?;
    let mut cues = Vec::new();
    let mut actor_preserved = true;
    let mut team_members = Vec::new();
    for _ in 0..230 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        if sim.session().tick() == 30 {
            team_members = sim
                .team_script_vm()
                .teams_in_order()
                .flat_map(|team| team.members())
                .collect();
        }
        actor_preserved &= sim
            .entities()
            .get(actor)
            .is_some_and(|e| e.is_active() && e.health.current() > 0);
        for effect in output.trigger_effects {
            if let TriggerEffect::TalkBubble {
                actor,
                value,
                duration,
            } = effect
            {
                cues.push(json!({"tick":sim.session().tick(),"actor":actor,"value":value,"duration":duration}));
            }
        }
    }
    let expected = if control {
        vec![]
    } else if expiry {
        vec![(46, Some(actor), 1)]
    } else {
        vec![
            (46, Some(actor), 1),
            (76, Some(actor), 2),
            (136, None, 0),
            (166, Some(actor), 3),
            (196, None, 0),
        ]
    };
    let contract = cues.len() == expected.len()
        && cues
            .iter()
            .zip(expected)
            .all(|(cue, (tick, actor, value))| {
                cue["tick"] == tick
                    && cue["actor"] == json!(actor)
                    && cue["value"] == value
                    && cue["duration"]
                        == if actor.is_some() {
                            runtime.resources.rules.general.talk_bubble_time
                        } else {
                            0
                        }
            });
    let checks = json!({"ordered_cues_and_missing_team_noop":contract,
        "original_actor_recruited":team_members == vec![actor],"actor_remains_alive":actor_preserved});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("talk-runtime-report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,
        "checks":checks,"actor":actor,"cues":cues,"talk_bubble_time":runtime.resources.rules.general.talk_bubble_time,
        "scope":"TS-built ordinary campaign frames and ordered presentation cues; GPU and wall-clock expiry require separate observation."}))?,
    )?;
    ensure!(
        passed,
        "talk bubble runtime audit failed; inspect talk-runtime-report.json"
    );
    Ok(())
}

fn audit_scene_pause(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    control: bool,
) -> Result<()> {
    use vera20k_core::sim::{
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    };
    let mut runtime = campaign::load(install, bytes, 1)?;
    let mut cues = Vec::new();
    for _ in 0..46 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        for effect in output.trigger_effects {
            if let TriggerEffect::ScenarioPause {
                duration,
                outermost,
            } = effect
            {
                cues.push(json!({"tick":runtime.simulation.session().tick(),"duration":duration,"outermost":outermost}));
            }
        }
    }
    let session = runtime.simulation.session();
    let expected = if control {
        json!([])
    } else {
        json!([{"tick":46,"duration":2,"outermost":true}])
    };
    let checks = json!({"ordinary_frame_pause_cue":json!(cues) == expected,
        "timed_hold_matches_requested_action":session.scenario_pause_timed() != control,
        "pause_depth_matches_requested_action":session.scenario_paused() != control,
        "prior_input_lock_retained":session.input_locked()});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("scene-pause-runtime-report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,
        "checks":checks,"cues":cues,"scope":"TS action and pause state after ordinary frame46. Automatic expiry is verified separately through the real host clock."}))?,
    )?;
    ensure!(passed, "pause runtime audit failed");
    Ok(())
}

fn audit_radar_movie(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    paused: bool,
    movie_index: Option<usize>,
) -> Result<()> {
    use vera20k_core::sim::{
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    };
    let mut runtime = campaign::load(install, bytes, 1)?;
    let mut cues = Vec::new();
    for _ in 0..46 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        for effect in output.trigger_effects {
            if let TriggerEffect::RadarMovie {
                index,
                pause_simulation,
            } = effect
            {
                cues.push(json!({"tick":runtime.simulation.session().tick(),"index":index,"pause_simulation":pause_simulation}));
            }
        }
    }
    let checks = json!({
        "unique_movie_request":cues == movie_index.map(|index|json!({"tick":46,"index":index,"pause_simulation":paused})).into_iter().collect::<Vec<_>>(),
        "prior_input_lock_retained":runtime.simulation.session().input_locked(),
        "pause_admission_belongs_to_host":!runtime.simulation.session().scenario_paused()
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("radar-runtime-report.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":passed,"checks":checks,"cues":cues,
            "scope":"TS-generated Action100/117 and ordinary-frame request only. Media playback and pause lifecycle require the real host."
        }))?,
    )?;
    ensure!(passed, "radar movie request audit failed");
    Ok(())
}

fn audit_team_media(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    control: bool,
    movie: usize,
) -> Result<()> {
    use vera20k_core::sim::{
        trigger_runtime::{ThemeRequest, TriggerEffect},
        world::{FrameEffects, SimSoundEvent, TickLane},
    };
    let audio = vera20k_core::rules::audio_sources::AudioDefinitions::select(&install.manager);
    let owner = vera20k_core::rules::retail_sources::RetailRulesSources::select(&install.manager)
        .map_err(anyhow::Error::msg)?
        .into_owner(
            std::sync::Arc::clone(audio.sounds()),
            std::sync::Arc::clone(audio.eva()),
        )
        .map_err(anyhow::Error::msg)?;
    let installed_movie = owner.movies().find_index("A01_P01e");
    let sound = audio
        .sounds()
        .index_of("SdkTone")
        .context("installed SdkTone ordinal")?;
    let music = vera20k_core::rules::theme_ini::ThemeCatalog::select(&install.manager)
        .from_name("SdkMusic");
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = runtime
        .simulation
        .entities()
        .iter_sorted()
        .map(|(id, e)| {
            (
                id,
                e.type_ref(),
                e.owner(),
                e.health.current(),
                runtime.simulation.object_coords_cell(id),
            )
        })
        .collect::<Vec<_>>();
    let mut cues = Vec::new();
    let mut stages = Vec::new();
    let mut markers = Vec::new();
    let mut previous = None;
    let mut original_intact = true;
    for _ in 0..350 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        for e in output.sound_events {
            if let SimSoundEvent::VocCenteredIndex { index } = e {
                cues.push(json!({"tick":tick,"kind":"sound","index":index}));
            }
        }
        for e in output.trigger_effects {
            match e {
                TriggerEffect::MusicTheme {
                    request: ThemeRequest::Index(index),
                } => cues.push(json!({"tick":tick,"kind":"music","index":index})),
                TriggerEffect::FullscreenMovie { index } => {
                    cues.push(json!({"tick":tick,"kind":"movie","index":index}))
                }
                _ => {}
            }
        }
        if let Some(t) = sim.team_script_vm().teams_in_order().next() {
            let cursor = t.cursor();
            if previous != Some(cursor) {
                stages.push(json!({"tick":tick,"cursor":cursor,"team":t}));
                previous = Some(cursor);
            }
        }
        let flags = sim.global_variables()[..3].to_vec();
        if markers
            .last()
            .is_none_or(|m: &serde_json::Value| m["flags"] != json!(flags))
        {
            markers.push(json!({"tick":tick,"flags":flags}));
        }
        original_intact &= initial.iter().all(|(id, ty, owner, health, cell)| {
            sim.entities().get(*id).is_some_and(|e| {
                e.type_ref() == *ty
                    && e.owner() == *owner
                    && e.health.current() == *health
                    && sim.object_coords_cell(*id) == *cell
                    && e.lifecycle.object_alive()
            })
        });
    }
    let count = |kind| cues.iter().filter(|c| c["kind"] == kind).count();
    let checks = json!({"movie_process_catalog_matches_sdk":usize::try_from(installed_movie).ok()==Some(movie),
        "single_centered_sound_request":count("sound")==1&&cues.iter().any(|c|c["kind"]=="sound"&&c["index"]==sound),
        "single_music_request":count("music")==1&&cues.iter().any(|c|c["kind"]=="music"&&c["index"]==music),
        "movie_request_only_in_authored_variant":count("movie")==usize::from(!control)&&cues.iter().filter(|c|c["kind"]=="movie").all(|c|c["index"]==movie),
        "requests_follow_script_order":cues.iter().map(|c|c["kind"].as_str().unwrap()).collect::<Vec<_>>()==if control{vec!["sound","music"]}else{vec!["sound","music","movie"]},
        "all_successor_markers_run":runtime.simulation.global_variables()[..3]==[true;3],
        "original_objects_unchanged":original_intact,
        "media_pause_belongs_to_host":!runtime.simulation.session().scenario_paused()});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("team-media-runtime-report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"cues":cues,"stages":stages,"markers":markers,"scope":"TS recruitment and ordinary frame media requests, exact installed catalog ordinals and successor lines. No media, team, timers or state injection. Not actual audio/video playback."}),
        )?,
    )?;
    ensure!(passed, "team media runtime audit failed");
    Ok(())
}

fn audit_chrono_screen(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    control: bool,
) -> Result<()> {
    use vera20k_core::sim::{
        replay::ScenarioServiceInput,
        superweapon::chrono_screen::ChronoScreenCommand,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    };
    let mut runtime = campaign::load(install, bytes, 1)?;
    let mut commands = Vec::new();
    let mut pauses = 0;
    let mut resumes = 0;
    let mut services = 0;
    let mut service_ticks_frozen = true;
    let mut draws = Vec::new();
    let mut whites = 0;
    while runtime.simulation.session().tick() < 190 {
        let paused = runtime.simulation.session().scenario_paused();
        let effects = if paused {
            let tick = runtime.simulation.session().tick();
            services += 1;
            ensure!(services < 100, "chrono services did not finish");
            let effects = ScenarioServiceInput::StepChronoScreen.apply(&mut runtime.simulation);
            service_ticks_frozen &= runtime.simulation.session().tick() == tick;
            effects
        } else {
            runtime
                .advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?
                .trigger_effects
        };
        for effect in effects {
            match effect {
                TriggerEffect::ChronoScreen { command } => {
                    match command {
                        ChronoScreenCommand::Draw { step, duration } => {
                            ensure!(duration == 15, "unexpected chrono duration");
                            draws.push(step);
                        }
                        ChronoScreenCommand::White => whites += 1,
                        _ => {}
                    }
                    commands.push(json!({"tick":runtime.simulation.session().tick(),"service":paused,"command":command}));
                }
                TriggerEffect::ScenarioPause {
                    duration,
                    outermost,
                } => {
                    ensure!(duration == 0 && outermost, "chrono owns one untimed pause");
                    pauses += 1;
                }
                TriggerEffect::ScenarioResume => resumes += 1,
                _ => {}
            }
        }
    }
    let expected_draws = if control {
        vec![]
    } else {
        (0..15).chain((1..=15).rev()).collect::<Vec<_>>()
    };
    let count = |name: &str| commands.iter().filter(|row| row["command"] == name).count();
    let checks = json!({"single_start_capture_and_finish": if control {commands.is_empty()} else {count("Start")==1 && count("CaptureDestination")==1 && count("Finish")==1},
        "draw_steps_match_both_phases":draws==expected_draws,"white_phase_is_45_frames":whites==if control {0} else {45},
        "one_pause_and_resume":pauses==if control {0} else {1} && resumes==pauses,
        "service_steps_preserve_world_tick":service_ticks_frozen && services==if control {0} else {16},
        "ordinary_world_continues_and_unlocks":!runtime.simulation.session().scenario_paused() && !runtime.simulation.session().input_locked()});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("chrono-runtime-report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"checks":checks,
        "commands":commands,"services":services,"scope":"TS-built ordinary frames and public replay service inputs while paused. GPU transitions and actual host service are verified separately."}))?,
    )?;
    ensure!(passed, "chrono runtime audit failed");
    Ok(())
}

fn write_tone(root: &std::path::Path, name: &str, hz: f32, count: usize) -> Result<Vec<i16>> {
    let samples: Vec<i16> = (0..count)
        .map(|index| ((index as f32 * hz * std::f32::consts::TAU / 8000.0).sin() * 6000.0) as i16)
        .collect();
    let mut wave = b"RIFF".to_vec();
    wave.extend((36 + samples.len() as u32 * 2).to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(8000u32.to_le_bytes());
    wave.extend(16000u32.to_le_bytes());
    wave.extend(2u16.to_le_bytes());
    wave.extend(16u16.to_le_bytes());
    wave.extend(b"data");
    wave.extend((samples.len() as u32 * 2).to_le_bytes());
    wave.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
    std::fs::write(root.join(name), wave)?;
    Ok(samples)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    let variant = args.next();
    ensure!(
        variant
            .as_ref()
            .is_none_or(|value| value == "--campaign-media"
                || value == "--audio-streams"
                || value == "--team-media"
                || value == "--team-media-control"
                || value == "--intro-media"
                || value == "--intro-media-control"
                || value == "--visibility"
                || value == "--camera-pan"
                || value == "--camera-restore"
                || value == "--camera-restore-menu"
                || value == "--talk-bubbles"
                || value == "--talk-bubbles-control"
                || value == "--talk-bubbles-fixture"
                || value == "--talk-bubbles-fixture-control"
                || value == "--talk-bubbles-fixture-expiry"
                || value == "--scene-pause"
                || value == "--scene-pause-control"
                || value == "--chrono-screen"
                || value == "--chrono-screen-control"
                || value == "--radar-paused"
                || value == "--radar-unpaused"
                || value == "--radar-control")
            && args.next().is_none(),
        "optional variant: --campaign-media, --audio-streams, --team-media[-control], --intro-media[-control], --visibility, --camera-pan, --camera-restore[-menu], --talk-bubbles[-control], --talk-bubbles-fixture[-control|-expiry], --scene-pause[-control], --chrono-screen[-control] or --radar-paused/--radar-unpaused/--radar-control"
    );
    let audio_streams = variant
        .as_ref()
        .is_some_and(|value| value == "--audio-streams");
    let team_media = variant
        .as_ref()
        .is_some_and(|v| v == "--team-media" || v == "--team-media-control");
    let team_media_control = variant
        .as_ref()
        .is_some_and(|v| v == "--team-media-control");
    let visibility = variant
        .as_ref()
        .is_some_and(|value| value == "--visibility");
    let camera_restore_menu = variant
        .as_ref()
        .is_some_and(|value| value == "--camera-restore-menu");
    let camera_restore = camera_restore_menu
        || variant
            .as_ref()
            .is_some_and(|value| value == "--camera-restore");
    let camera_pan = camera_restore
        || variant
            .as_ref()
            .is_some_and(|value| value == "--camera-pan");
    let talk_variant = variant
        .as_ref()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let talk_bubbles = talk_variant.starts_with("--talk-bubbles");
    let talk_control = talk_bubbles && talk_variant.ends_with("-control");
    let talk_fixture = talk_variant.starts_with("--talk-bubbles-fixture");
    let talk_expiry = talk_variant.ends_with("-expiry");
    let scene_pause = talk_variant.starts_with("--scene-pause");
    let pause_control = talk_variant == "--scene-pause-control";
    let chrono_screen = talk_variant.starts_with("--chrono-screen");
    let chrono_control = talk_variant == "--chrono-screen-control";
    let radar_movie = talk_variant.starts_with("--radar-");
    let radar_paused = talk_variant == "--radar-paused";
    let radar_control = talk_variant == "--radar-control";
    let intro_media = talk_variant.starts_with("--intro-media");
    let intro_control = talk_variant == "--intro-media-control";
    let campaign_media = variant.is_some() && !visibility;
    let player = if campaign_media {
        "Americans"
    } else {
        "Neutral"
    };
    ensure!(!root.exists(), "output must be new");
    std::fs::create_dir_all(&root)?;
    if radar_movie || team_media || intro_media {
        let subtitles = "BeginMovie A01_P01e\n\
<Time=00:00:00:20, Duration=00:00:10:00, Text=\"SDK movie cue A\">\n\
<Time=00:00:00:90, Duration=00:00:10:00, Text=\"SDK movie cue B\">\n\
<Time=00:00:01:60, Duration=00:00:10:00, Text=\"SDK movie cue C\">\n\
<Time=00:00:02:30, Duration=00:00:10:00, Text=\"SDK movie cue D\">\nEndMovie\n";
        let bytes: Vec<_> = std::iter::once(0xfeffu16)
            .chain(subtitles.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        std::fs::write(root.join("subtitlemd.txt"), bytes)?;
    }
    // A short, quiet nonzero PCM source lets the production output observer
    // distinguish played media from a submitted request or silent fixture.
    let samples = write_tone(&root, "sdk_tone.wav", 440.0, 2000)?;
    if audio_streams || team_media || radar_movie || intro_media {
        write_tone(&root, "sdkmusic.wav", 660.0, 16000)?;
        write_tone(&root, "sdkeva.wav", 880.0, 8000)?;
        write_tone(&root, "sdkquiet.wav", 0.0, 80000)?;
        std::fs::write(
            root.join("thememd.ini"),
            "[Themes]\n0=SdkMusic\n1=SdkQuiet\n[SdkMusic]\nSound=sdkmusic\nNormal=no\nRepeat=no\n[SdkQuiet]\nSound=sdkquiet\nNormal=yes\nRepeat=yes\n",
        )?;
        std::fs::write(
            root.join("evamd.ini"),
            "[DialogList]\n0=SdkEva\n[SdkEva]\nAllied=sdkeva\nRussian=sdkeva\nYuri=sdkeva\n",
        )?;
    }
    // Registered SFX resolve sample ordinals through the selected BAG index.
    let pcm: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let mut index = Vec::new();
    for value in [0x41424147u32, 2, 1] {
        index.extend(value.to_le_bytes());
    }
    let mut name = [0u8; 16];
    name[..8].copy_from_slice(b"sdk_tone");
    index.extend(name);
    for value in [0u32, pcm.len() as u32, 8000, 4, 0] {
        index.extend(value.to_le_bytes());
    }
    std::fs::write(root.join("audio.idx"), index)?;
    std::fs::write(root.join("audio.bag"), pcm)?;
    std::fs::write(
        root.join("soundmd.ini"),
        "[SoundList]\n0=SdkTone\n[SdkTone]\nSounds=sdk_tone\nVolume=35\n",
    )?;
    let initial = NewMapOptions::default().bytes()?;
    let mut publisher = Publisher::default();
    let scene = publisher.scene(initial, &directory)?;
    let mut doc = MapDocument::new(scene.source.edit_base.map_err(anyhow::Error::msg)?);
    if campaign_media {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            player,
            player,
            None,
        )?;
        doc.set_ini(player, "Country", Some(b"Americans".to_vec()))?;
    }
    doc.add_record("Structures", [40, 100], player, "GAPOWR")?;
    if visibility {
        for (at, kind) in [
            ([46, 100], "GACNST"),
            ([50, 100], "GAPILE"),
            ([54, 100], "GAAIRC"),
        ] {
            doc.add_record("Structures", at, player, kind)?;
        }
        doc.add_record("Structures", [80, 100], "Special", "NAPOWR")?;
    }
    if camera_pan {
        doc.add_record("Structures", [46, 100], player, "GACNST")?;
        doc.add_record("Structures", [80, 100], "Special", "NAPOWR")?;
    }
    if talk_bubbles || team_media || camera_restore {
        doc.add_record("Infantry", [44, 100], player, "E1")?;
    }
    if talk_bubbles {
        // Keep a selected bubble alive throughout exact-step GPU captures.
        // The ordinary wall clock still owns expiry; this is no timer override.
        doc.set_ini(
            "General",
            "TalkBubbleTime",
            Some(if talk_expiry {
                b"0.5".to_vec()
            } else {
                b"60".to_vec()
            }),
        )?;
    }
    doc.commit("presentation landmark");
    let filename = if scene_pause
        || chrono_screen
        || radar_movie
        || team_media
        || intro_media
        || (camera_restore && !camera_restore_menu)
    {
        "SOV01UMD.MAP"
    } else if campaign_media {
        "ALL01UMD.MAP"
    } else {
        "presentation.map"
    };
    let map_path = root.join(filename);
    std::fs::write(&map_path, doc.snapshot().bytes()?)?;
    let source = root.join("mission.ts");
    std::fs::write(
        &source,
        r#"import {defineMission,event,action,order,RadarEvent,SidebarTab,MissionMode} from './sdk';
import {map,House,MapObject,Techno,Country,Movie,Theme,Unit} from './map.generated';
export default defineMission(map,__OPTIONS__,m=>{
 m.asset('sdk_tone.wav','sdk_tone.wav');m.asset('soundmd.ini','soundmd.ini');
 m.asset('audio.idx','audio.idx');m.asset('audio.bag','audio.bag');
 __TALK_ASSET__
 const target=m.createWaypoint(30,40,100);
 __CAMPAIGN_MEDIA__
 const title=m.defineText('timer','TS SDK 倒计时');
 const text=m.defineText('message','TS SDK：任务已开始');
 __SCENARIO_RULES__
});"#
        .replace("__OPTIONS__", &if visibility { "{id:'visibility',mode:MissionMode.Multiplayer}".to_string() } else { format!("{{id:'presentation',player:House.{player}}}") })
        .replace("__SCENARIO_RULES__", if camera_restore { r#"
 m.configureBasic({homeCell:target,ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),playerControl:true,iq:0});
 const view=m.createWaypoint(31,80,100);
 const actor=m.team('saved_guard',{house:m.house(House.Americans),waypoint:target,
   units:[{type:m.unit(Unit.E1),count:1}],full:true,max:1,reinforce:false,
   orders:[order.guard(4),order.setGlobal(m.globalFlag(0)),order.guard(4),order.waitForever()]});
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,
   actions:[action.lockInput(),action.jumpCamera(target),action.revealMap(),action.setCountdown(30),action.setCountdownTitle(title),action.recruit(actor)]});
 m.rule('pan',{when:event.sinceScenarioStart(3),once:true,actions:[action.centerCamera(view,4)]});
 m.rule('repeat',{when:event.sinceEnabled(2),once:false,actions:[action.message(m.defineText('repeat','SDK repeated rule'))]});
 m.rule('guard_done',{when:event.globalSet(m.globalFlag(0)),once:true,actions:[action.message(m.defineText('guard_done','SDK saved guard finished'))]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(8),once:true,actions:[action.unlockInput()]});
"# } else if intro_media { r#"
 m.asset('subtitlemd.txt','subtitlemd.txt');
 m.asset('thememd.ini','thememd.ini');m.asset('sdkquiet.wav','sdkquiet.wav');
 m.configureBasic({homeCell:target,__INTRO_FIELD__theme:m.assetTheme('SdkQuiet'),ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),playerControl:true,iq:0});
 m.rule('begin',{when:event.sinceScenarioStart(0),once:true,actions:[action.revealMap(),
   action.setCountdown(30),action.setCountdownTitle(m.defineText('intro_timer','SDK intro countdown')),
   action.message(m.defineText('intro_finished','SDK intro finished'))]});
 m.rule('running',{when:event.sinceScenarioStart(2),once:true,
   actions:[action.message(m.defineText('intro_running','SDK intro running'))]});
"# } else if team_media { r#"
 m.asset('subtitlemd.txt','subtitlemd.txt');
 m.asset('thememd.ini','thememd.ini');m.asset('sdkmusic.wav','sdkmusic.wav');m.asset('sdkquiet.wav','sdkquiet.wav');
 m.configureBasic({homeCell:target,theme:m.assetTheme('SdkQuiet'),ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),playerControl:true,iq:0});
 const media=m.team('media',{house:m.house(House.Americans),waypoint:target,units:[{type:m.unit(Unit.E1),count:1}],full:true,max:1,reinforce:false,
   orders:[order.guard(2),order.playSound(m.assetSound('SdkTone')),order.setGlobal(m.globalFlag(0)),order.guard(4),
    order.playMusic(m.assetTheme('SdkMusic')),order.setGlobal(m.globalFlag(1)),order.guard(4),__TEAM_MOVIE__,order.setGlobal(m.globalFlag(2)),order.waitForever()]});
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.setCountdown(30),action.recruit(media)]});
 for(const [n,label] of [[0,'SDK team sound'],[1,'SDK team music'],[2,'SDK team movie returned']] as const)
   m.rule('marker_'+n,{when:event.globalSet(m.globalFlag(n)),once:true,actions:[action.message(m.defineText('stage_'+n,label))]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(20),once:true,actions:[action.unlockInput()]});
"# } else if visibility { r#"
 for(const id of [MapObject.Structures_0,MapObject.Structures_1,MapObject.Structures_2,MapObject.Structures_3]) m.assignObjectToPlayer(m.object(id),0);
 const view=m.createWaypoint(31,80,100), alert=m.createWaypoint(32,100,85);
 m.rule('reveal',{when:event.sinceScenarioStart(1),once:true,actions:[action.revealAt(view),action.jumpCamera(view),action.sidebarTab(SidebarTab.Infantry),action.flashCameo(m.techno(Techno.E1),90)]});
 m.rule('conceal',{when:event.sinceScenarioStart(3),once:true,actions:[action.concealAt(view)]});
 m.rule('zone',{when:event.sinceScenarioStart(5),once:true,actions:[action.revealZone(view)]});
 m.rule('reshroud',{owner:m.country(Country.Americans),when:event.sinceScenarioStart(7),once:true,actions:[action.reshroudMap()]});
 m.rule('whole_map',{when:event.sinceScenarioStart(9),once:true,actions:[action.revealMap()]});
 m.rule('grow',{when:event.sinceScenarioStart(11),once:true,actions:[action.growShroud(),action.radarEvent(RadarEvent.BeaconPlaced,alert)]});
 m.rule('home',{when:event.sinceScenarioStart(13),once:true,actions:[action.homeCamera(),action.sidebarTab(SidebarTab.Buildings)]});
 m.rule('area',{when:event.sinceScenarioStart(17),once:true,actions:[action.visibleMapArea(8,8,64,64)]});
 m.rule('outage',{owner:m.country(Country.Americans),when:event.sinceScenarioStart(19),once:true,actions:[action.radarOutage(90)]});
"# } else if radar_movie { r#"
 m.asset('subtitlemd.txt','subtitlemd.txt');
 m.asset('thememd.ini','thememd.ini');m.asset('sdkquiet.wav','sdkquiet.wav');
 m.configureBasic({homeCell:target,theme:m.assetTheme('SdkQuiet'),ignoreGlobalAiTriggers:true,skipScore:true});
 const before=m.defineText('before','SDK radar start'),after=m.defineText('after','SDK radar finished');
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.setCountdown(30)]});
 m.rule('radar',{when:event.sinceScenarioStart(3),once:true,actions:[action.message(before)__RADAR_ACTION__]});
 m.rule('after',{when:event.sinceScenarioStart(16),once:true,actions:[action.message(after)]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(20),once:true,actions:[action.unlockInput()]});
"# } else if chrono_screen { r#"
 m.configureBasic({homeCell:target,ignoreGlobalAiTriggers:true,skipScore:true});
 const before=m.defineText('before','SDK chrono start'),after=m.defineText('after','SDK chrono finished');
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.setCountdown(30)]});
 m.rule('chrono',{when:event.sinceScenarioStart(3),once:true,actions:[action.message(before),action.chronoScreen(__CHRONO_DURATION__)]});
 m.rule('after',{when:event.sinceScenarioStart(10),once:true,actions:[action.message(after)]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(12),once:true,actions:[action.unlockInput()]});
"# } else if scene_pause { r#"
 m.configureBasic({homeCell:target,ignoreGlobalAiTriggers:true,skipScore:true});
 const before=m.defineText('before','SDK pause start'),after=m.defineText('after','SDK pause resumed');
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.setCountdown(30)]});
 m.rule('pause',{when:event.sinceScenarioStart(3),once:true,actions:[action.message(before),__PAUSE_ACTION__]});
 m.rule('after',{when:event.sinceScenarioStart(4),once:true,actions:[action.message(after)]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(6),once:true,actions:[action.unlockInput()]});
"# } else if talk_bubbles { r#"
 m.configureBasic({homeCell:target,ignoreGlobalAiTriggers:true,skipScore:true});
 const actor=m.team('speaker',{house:m.house(House.Americans),waypoint:target,
   units:[{type:m.unit(Unit.E1),count:1}],full:true,max:1,reinforce:false,orders:[order.waitForever()]});
 const absent=m.team('absent',{house:m.house(House.Americans),waypoint:target,
   units:[{type:m.unit(Unit.ENGINEER),count:1}],full:true,max:1,reinforce:false,orders:[order.waitForever()]});
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.recruit(actor)]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(2),once:true,actions:[action.unlockInput()]});
 __TALK_RULES__
"# } else if camera_pan { r#"
 m.configureBasic({homeCell:target,ignoreGlobalAiTriggers:true,skipScore:true});
 const view=m.createWaypoint(31,80,100);
 // The existing campaign opening capture waits for an authored input lock.
 m.rule('input_lock',{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput()]});
 m.rule('input_unlock',{when:event.sinceScenarioStart(2),once:true,actions:[action.unlockInput()]});
 m.rule('jump',{when:event.sinceScenarioStart(1),once:true,actions:[action.jumpCamera(target),action.revealMap()]});
 m.rule('pan',{when:event.sinceScenarioStart(3),once:true,actions:[action.centerCamera(view,4)]});
 m.rule('home',{when:event.sinceScenarioStart(8),once:true,actions:[action.homeCamera()]});
"# } else { r#"
 m.rule('hud',{when:event.sinceEnabled(1),once:true,actions:[action.revealMap(),action.message(text),action.setCountdown(30),action.setCountdownTitle(title),action.playSound(m.assetSound('SdkTone'))]});
 m.rule('positioned_sound',{when:event.sinceEnabled(2),once:true,actions:[action.playSoundAt(m.assetSound('SdkTone'),m.waypoint(0))]});
 m.rule('dark',{when:event.sinceEnabled(3),once:true,actions:[action.setAmbientStep(1),action.setAmbientRate(1),action.setAmbient(35)]});
 m.rule('camera',{when:event.sinceEnabled(5),once:true,actions:[action.jumpCamera(target),action.radarEvent(RadarEvent.BeaconPlaced,target)]});
"# })
        .replace("__CAMPAIGN_MEDIA__", if audio_streams { r#"
 m.asset('thememd.ini','thememd.ini');
 m.asset('evamd.ini','evamd.ini');m.asset('sdkmusic.wav','sdkmusic.wav');m.asset('sdkeva.wav','sdkeva.wav');
 m.asset('sdkquiet.wav','sdkquiet.wav');
 m.configureBasic({homeCell:target,theme:m.assetTheme('SdkQuiet'),ignoreGlobalAiTriggers:true,skipScore:true});
 m.rule('input_lock',{when:event.sinceEnabled(1),once:true,actions:[action.lockInput()]});
 m.rule('input_unlock',{when:event.sinceEnabled(2),once:true,actions:[action.unlockInput()]});
 m.rule('music_stream',{when:event.sinceEnabled(3),once:true,actions:[action.playMusic(m.assetTheme('SdkMusic'))]});
 m.rule('eva_stream',{when:event.sinceEnabled(30),once:true,actions:[action.playEva(m.assetEva('SdkEva'))]});
"# } else if campaign_media && !camera_pan && !talk_bubbles && !scene_pause && !chrono_screen && !radar_movie && !team_media && !intro_media { r#"
 m.configureBasic({homeCell:target,intro:m.movie(Movie.A01_P01e),theme:m.theme(Theme.Drok)});
 m.rule('input_lock',{when:event.sinceEnabled(1),once:true,actions:[action.lockInput()]});
 m.rule('input_unlock',{when:event.sinceEnabled(2),once:true,actions:[action.unlockInput()]});
 m.rule('transmission',{when:event.sinceEnabled(3),once:true,actions:[action.playRadarMovie(m.movie(Movie.A01_P01e))]});
 m.rule('music',{when:event.sinceEnabled(6),once:true,actions:[action.playMusic(m.theme(Theme.INTRO))]});
"# } else { "" })
        .replace("__TALK_ASSET__", if talk_fixture { "m.asset('talkbubl.shp','talkbubl.shp');" } else { "" })
        .replace("__PAUSE_ACTION__", if pause_control { "" } else { "action.pauseScene(2)" })
        .replace("__CHRONO_DURATION__", if chrono_control { "0" } else { "15" })
        .replace("__RADAR_ACTION__", if radar_control { "" } else if radar_paused { ",action.playRadarMovie(m.movie(Movie.A01_P01e),true)" } else { ",action.playRadarMovie(m.movie(Movie.A01_P01e),false)" })
        .replace("__TEAM_MOVIE__",if team_media_control {"order.next()"}else{"order.playMovie(m.movie(Movie.A01_P01e))"})
        .replace("__INTRO_FIELD__",if intro_control {""}else{"intro:m.movie(Movie.A01_P01e),"})
        .replace("__TALK_RULES__", if talk_control { "" } else if talk_expiry { r#"
 m.rule('one',{when:event.sinceScenarioStart(3),once:true,actions:[action.message(text),action.talkBubble(actor,1)]});
"# } else { r#"
 m.rule('one',{when:event.sinceScenarioStart(3),once:true,actions:[action.talkBubble(actor,1)]});
 m.rule('two',{when:event.sinceScenarioStart(5),once:true,actions:[action.talkBubble(actor,2)]});
 m.rule('no_live_team',{when:event.sinceScenarioStart(7),once:true,actions:[action.talkBubble(absent,3)]});
 m.rule('clear_team',{when:event.sinceScenarioStart(9),once:true,actions:[action.talkBubble(actor,0)]});
 m.rule('three',{when:event.sinceScenarioStart(11),once:true,actions:[action.talkBubble(actor,3)]});
 m.rule('clear_global',{when:event.sinceScenarioStart(13),once:true,actions:[action.clearTalkBubble()]});
"# }),
    )?;
    let mut install = Install::open(&directory)?;
    if talk_fixture {
        write_talk_shape(&install, &root)?;
    }
    if talk_bubbles {
        let sprite = install.manager.get_ref("talkbubl.shp");
        let shape = sprite
            .map(vera20k_core::assets::shp_file::ShpFile::from_bytes)
            .transpose()?;
        std::fs::write(
            root.join("talk-resources.json"),
            serde_json::to_vec_pretty(&json!({
            "original_sprite_found":sprite.is_some(),"original_sprite_bytes":sprite.map(<[u8]>::len),
            "controlled_shape_declared":talk_fixture,
                "frames":shape.as_ref().map(|shp| shp.frames.iter().map(|frame|json!({
                    "width":frame.frame_width,"height":frame.frame_height,
                    "opaque_pixels":frame.pixels.iter().filter(|pixel|**pixel != 0).count()
                })).collect::<Vec<_>>()),"palette_found":install.manager.get_ref("palette.pal").is_some()
            }))?,
        )?;
    }
    let scene = ra2_map_scene::map::load_scene(&mut install, map_path.to_str().unwrap())?;
    let catalog = mission_script::catalog(&doc, &scene);
    let radar_index = if radar_movie || team_media {
        Some(
            catalog.registry_order["movies"]
                .iter()
                .position(|id| id == "A01_P01e")
                .context("movie ordinal")?,
        )
    } else {
        None
    };
    let movies: Vec<_> = catalog
        .movies
        .iter()
        .map(|id| {
            let filename = format!("{id}.bik");
            let installed = install.manager.get_with_source_ref(&filename);
            let metadata = installed.map(|(bytes, source)| {
                match vera20k_core::assets::bink_file::BinkFile::parse(std::sync::Arc::from(bytes))
                {
                    Ok(movie) => json!({"source":source,"bytes":bytes.len(),
                        "width":movie.header.width,"height":movie.header.height,
                        "frames":movie.header.num_frames,"fps":movie.header.fps(),
                        "audio_tracks":movie.header.num_audio_tracks}),
                    Err(error) => json!({"source":source,"error":error.to_string()}),
                }
            });
            json!({"id":id,"installed":installed.is_some(),"metadata":metadata})
        })
        .collect();
    std::fs::write(
        root.join("movie-inventory.json"),
        serde_json::to_vec_pretty(&json!({"movies":movies,
            "limits":"Container parsing only; not video decode, GPU presentation or audio playback."}))?,
    )?;
    if (team_media || radar_movie || intro_media)
        && let Some(movie) = install.manager.get_ref("A01_P01e.bik")
    {
        // Keep the production-resolved bytes for an independent soundtrack
        // decode and comparison with the normal client's OS audio output.
        std::fs::write(root.join("A01_P01e.bik"), movie)?;
    }
    mission_script::refresh_types(&source, &catalog)?;
    let worker = mission_script::Worker::new()?;
    let built = worker
        .build(
            &doc,
            catalog.clone(),
            mission_script::Sources::read(&source)?,
        )?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    built.build.apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    if intro_media {
        worker
            .build(&doc, catalog, mission_script::Sources::read(&source)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut doc)?;
        ensure!(bytes == doc.snapshot().bytes()?, "intro unchanged rebuild");
        let parsed = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
        ensure!(
            parsed.basic.intro.as_deref() == (!intro_control).then_some("A01_P01e"),
            "parsed Intro"
        );
        std::fs::write(
            root.join("intro-build.json"),
            serde_json::to_vec_pretty(&json!({
            "passed":true,"unchanged_rebuild":true,"control":intro_control,"intro":parsed.basic.intro,
            "scope":"TS Worker build, unchanged rebuild and parsed Basic/Intro only; ordinary visible playback checked separately."}))?,
        )?;
    }
    if talk_bubbles {
        audit_talk_bubbles(&install, &bytes, &root, talk_control, talk_expiry)?;
    }
    if scene_pause {
        audit_scene_pause(&install, &bytes, &root, pause_control)?;
    }
    if chrono_screen {
        audit_chrono_screen(&install, &bytes, &root, chrono_control)?;
    }
    if let Some(index) = radar_index.filter(|_| radar_movie) {
        audit_radar_movie(
            &install,
            &bytes,
            &root,
            radar_paused,
            (!radar_control).then_some(index),
        )?;
    }
    let payload = mission_script::playtest_payload(&bytes, &directory, &scene.revision.id())?;
    let font_bytes = install.manager.get_ref("game.fnt");
    if radar_movie || intro_media {
        std::fs::write(
            root.join("game.fnt"),
            font_bytes.context("radar fixture font")?,
        )?;
    }
    let font = font_bytes
        .map(vera20k_core::assets::fnt_file::FntFile::from_bytes)
        .transpose()?;
    let coverage: Vec<_> = mission_script::declared_texts(&bytes)?
        .into_iter()
        .map(|(label, text)| {
            let missing: std::collections::BTreeSet<_> = text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .filter(|ch| {
                    u16::try_from(u32::from(*ch))
                        .ok()
                        .and_then(|cp| font.as_ref()?.glyph(cp))
                        .is_none()
                })
                .collect();
            json!({"label":label,"text":text,"missing":missing})
        })
        .collect();
    let sample_chars: std::collections::BTreeSet<_> = mission_script::declared_texts(&bytes)?
        .into_values()
        .flat_map(|text| text.chars().filter(|ch| !ch.is_ascii()).collect::<Vec<_>>())
        .collect();
    let mut samples = image::RgbaImage::new(32 * sample_chars.len().max(1) as u32, 32);
    for (index, ch) in sample_chars.iter().enumerate() {
        if let Some(glyph) = u16::try_from(u32::from(*ch))
            .ok()
            .and_then(|cp| font.as_ref()?.glyph(cp))
        {
            let bitmap = image::RgbaImage::from_raw(
                glyph.width,
                font.as_ref().unwrap().bitmap_rows,
                glyph.rgba.clone(),
            )
            .context("font glyph dimensions")?;
            image::imageops::overlay(&mut samples, &bitmap, index as i64 * 32, 0);
        }
    }
    samples.save(root.join("font-glyphs.png"))?;
    std::fs::write(
        root.join("font-coverage.json"),
        serde_json::to_vec_pretty(
            &json!({"font_found":font.is_some(),"texts":coverage,"sample_chars":sample_chars,"note":"Lookup presence does not guarantee that a font's stored glyph is not a placeholder; inspect font-glyphs.png and the GPU capture separately."}),
        )?,
    )?;
    let assets = root.join("install");
    std::fs::create_dir(&assets)?;
    // Retain the selected base roots and MIX archives, without linking mutable
    // user settings, saves or log directories from the original installation.
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if entry.file_type()?.is_file()
            && (name.ends_with(".mix")
                || matches!(
                    name.as_str(),
                    "rulesmd.ini"
                        | "artmd.ini"
                        | "aimd.ini"
                        | "soundmd.ini"
                        | "evamd.ini"
                        | "thememd.ini"
                        | "theme.ini"
                        | "temperatmd.ini"
                        | "snowmd.ini"
                        | "urbanmd.ini"
                        | "urbannmd.ini"
                        | "desertmd.ini"
                        | "lunarmd.ini"
                        | "ra2md.csf"
                        | "game.fnt"
                        | "audio.idx"
                        | "audio.bag"
                ))
            && !payload.contains_key(&name)
        {
            if name.ends_with(".mix") {
                std::fs::hard_link(entry.path(), assets.join(&name))
                    .or_else(|_| std::fs::copy(entry.path(), assets.join(&name)).map(|_| ()))?;
            } else {
                std::fs::copy(entry.path(), assets.join(&name))?;
            }
        }
    }
    for (name, bytes) in payload {
        std::fs::write(assets.join(name), bytes)?;
    }
    let installed_map = assets.join(filename);
    std::fs::write(&installed_map, &bytes)?;
    std::fs::write(&map_path, &bytes)?;
    if intro_media {
        audit_intro_runtime(&Install::open(&assets)?, &bytes, &root)?;
    }
    let config =
        vera20k_core::util::config::set_paths_key("", "ra2_dir", &assets.to_string_lossy());
    let config = vera20k_core::util::config::set_paths_key(
        &config,
        "settings_dir",
        &root.join("settings").to_string_lossy(),
    );
    std::fs::write(root.join("config.toml"), config)?;
    if team_media {
        audit_team_media(
            &Install::open(&assets)?,
            &bytes,
            &root,
            team_media_control,
            radar_index.context("movie ordinal")?,
        )?;
    }
    if visibility {
        // The existing capture input accepts ASCII keys only. Exercise the
        // ordinary configurable radar-review command through an isolated J
        // binding, without changing client diagnostics or the user's profile.
        std::fs::create_dir_all(root.join("settings"))?;
        std::fs::write(
            root.join("settings/VERA.INI"),
            "[Hotkey]\nCenterOnRadarEvent=74\n",
        )?;
    }
    let launch = SkirmishLaunchSession {
        mode: SkirmishLaunchMode::stock(1).context("Battle mode")?,
        selected_map_file: Some(installed_map.to_string_lossy().into_owned()),
        humans: vec![SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: "SDK audit".into(),
            slot: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Position(0),
                team: LaunchTeam::None,
            },
            role: LaunchHumanRole::default(),
        }],
        local_player_index: 0,
        ai_player_count: 0,
        opponents: vec![],
        pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
        options: SkirmishLaunchOptions {
            unit_count: 0,
            short_game: false,
            crates: false,
            shroud: visibility,
            fog_of_war: false,
            ..Default::default()
        },
    };
    if visibility {
        // The GPU receipt intentionally has no LocalSize projection. Read
        // existing simulation authority through the production headless loader
        // instead of adding an engine diagnostic just for this audit.
        let mut scenario =
            vera20k_core::headless_scenario::load_session(&assets, filename, 12345, launch.clone())
                .map_err(anyhow::Error::msg)?;
        let initial = scenario.sim().playfield_bounds();
        let initial_revision = scenario.sim().playfield_revision();
        let mut observations = Vec::new();
        let mut unchanged_before_deadline = true;
        let mut settled_after_deadline = true;
        let mut first_change = None;
        let expected = vera20k_core::map::playfield::PlayfieldBounds {
            base: 80,
            off_fc: 8,
            off_100: 8,
            off_104: 64,
            off_108: 64,
        };
        for tick in 1..=420 {
            scenario.tick();
            let bounds = scenario.sim().playfield_bounds();
            let revision = scenario.sim().playfield_revision();
            if tick <= 250 {
                unchanged_before_deadline &= bounds == initial && revision == initial_revision;
            }
            if tick >= 270 {
                settled_after_deadline &=
                    bounds == Some(expected) && revision == initial_revision + 1;
            }
            if bounds != initial && first_change.is_none() {
                first_change = Some(tick);
            }
            observations.push(json!({"tick":tick,"bounds":bounds,"revision":revision}));
        }
        let checks = json!({
            "authored_initial_bounds":initial == Some(vera20k_core::map::playfield::PlayfieldBounds {
                base:80,off_fc:2,off_100:4,off_104:76,off_108:70,
            }),
            "unchanged_before_deadline":unchanged_before_deadline,
            "changes_at_authored_deadline":first_change.is_some_and(|tick| (255..=257).contains(&tick)),
            "single_revision_and_stable_authored_bounds":settled_after_deadline,
        });
        let passed = checks
            .as_object()
            .unwrap()
            .values()
            .all(|value| value == true);
        std::fs::write(
            root.join("local-size-report.json"),
            serde_json::to_vec_pretty(&json!({"passed":passed,"checks":checks,
                "initial":initial,"initial_revision":initial_revision,"first_change_tick":first_change,
                "observations":observations,
                "scope":"TS build and ordinary production headless frames; existing readonly LocalSize authority and revision. Not GPU clipping, all zone geometry or save/load acceptance."}))?,
        )?;
        ensure!(
            passed,
            "LocalSize runtime audit failed; inspect local-size-report.json"
        );
    }
    let capture_ticks: &[u32] = if visibility {
        &[
            0, 30, 32, 60, 90, 120, 122, 150, 180, 210, 250, 270, 300, 360, 420, 600,
        ]
    } else {
        &[0, 30, 60, 90]
    };
    for &ticks in capture_ticks {
        let mut profile = json!({
            "schema_version":"vera20k.map-observation-profile.v2",
            "launch":launch,"seed":12345,"input_delay_ticks":vera20k_core::util::config::default_input_delay_ticks(),"ticks":ticks,
            "width":1024,"height":768,"timeout_seconds":120,
            "observe_owners":[player],"observe_types":["GAPOWR"],
            "observe_tooltips":true,
            "cursor_position":[1000,700],
        });
        if visibility {
            profile["observe_owners"] = json!(["SDK audit"]);
            profile["observe_types"] = json!(["GAPOWR", "GACNST", "GAPILE", "GAAIRC", "NAPOWR"]);
            profile["terrain_cells"] =
                json!([[40, 100], [80, 100], [85, 100], [115, 100], [100, 85]]);
            profile["observe_sidebar_steps"] = json!([ticks]);
        }
        std::fs::write(
            root.join(format!("profile-{ticks}.json")),
            serde_json::to_vec_pretty(&profile)?,
        )?;
        if visibility && ticks == 210 {
            profile["gestures"] =
                json!([{"issue_after_step":205,"gesture":{"kind":"key","key":"J"}}]);
            std::fs::write(
                root.join("profile-event-review.json"),
                serde_json::to_vec_pretty(&profile)?,
            )?;
        }
    }
    let audio_profile = json!({
        "schema_version":"vera20k.map-observation-profile.v2", "launch":launch,
        "seed":12345, "input_delay_ticks":vera20k_core::util::config::default_input_delay_ticks(),
        "ticks":60, "width":1024, "height":768, "timeout_seconds":120,
        "observe_owners":[player], "observe_types":["GAPOWR"], "cursor_position":[1000,700],
        "observe_audio":{"sound_ids":["SdkTone"],"max_events":8,"max_samples_per_event":262144,"completion_tail_ms":4000}
    });
    std::fs::write(
        root.join("profile-audio.json"),
        serde_json::to_vec_pretty(&audio_profile)?,
    )?;
    let mut saved_profile = audio_profile;
    saved_profile
        .as_object_mut()
        .unwrap()
        .remove("observe_audio");
    saved_profile["ticks"] = json!(120);
    saved_profile["allow_load_segments"] = json!(true);
    saved_profile["gestures"] = json!([
        {"issue_after_step":30,"gesture":{"kind":"key","key":"M","modifiers":["Ctrl","Shift"]}},
        {"issue_after_step":60,"gesture":{"kind":"key","key":"N","modifiers":["Ctrl","Shift"]}}
    ]);
    std::fs::write(
        root.join("profile-save-load.json"),
        serde_json::to_vec_pretty(&saved_profile)?,
    )?;
    println!("Prepared {} (GPU capture pending)", root.display());
    if intro_media {
        println!(
            "Use tools/verify_mission_intro_media.py with the intro and no-intro fixtures for ordinary visible campaign playback."
        );
    } else if camera_restore_menu {
        println!(
            "Use shell capture campaign-all1-opening-load-unlocked and campaign-all1-opening-tick-167 to compare loading an unlocked save during an authored lock with continuous execution."
        );
    } else if camera_restore {
        println!(
            "Use shell capture campaign-sov1-opening-restore-unlocked and campaign-sov1-opening-unlocked to compare ordinary client save/load with continuous execution."
        );
    } else if scene_pause || chrono_screen || radar_movie || team_media {
        println!(
            "Use shell capture campaign-sov1-opening-realtime-unlocked for ordinary host-clock pause service."
        );
    } else if campaign_media {
        let tick = if audio_streams {
            720
        } else if talk_bubbles {
            210
        } else if camera_pan {
            150
        } else {
            420
        };
        println!(
            "Use shell capture campaign-all1-opening-tick-{tick} for campaign-only media; tactical profiles run in Skirmish mode."
        );
    }
    Ok(())
}
