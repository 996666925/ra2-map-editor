use super::*;
use std::{path::PathBuf, time::Instant};
use vera20k_core::{
    headless_scenario::{self, SIM_TICK_MS},
    sim::{
        command::{Command, CommandEnvelope},
        world::{FrameEffects, TickLane},
    },
};

/// Exercises the editor's actual stamp/serialization, retail TMP/rules, load,
/// ordinary Drive into the mouth, TubeMovement, and ordinary exit release.
/// TUNNEL_TEST_MAP can supply an existing map to exercise route redrawing.
#[test]
#[ignore = "requires RA2_DIR with installed retail assets"]
fn retail_tank_enters_and_exits_editor_authored_tunnel() {
    let install_dir = PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    let input = std::env::var_os("TUNNEL_TEST_MAP").map(PathBuf::from);
    let bytes = input.as_ref().map_or_else(
        || {
            crate::new_map::NewMapOptions {
                width: 40,
                height: 40,
                ..Default::default()
            }
            .bytes()
            .unwrap()
        },
        |path| std::fs::read(path).unwrap(),
    );
    let mut install = ra2_map_assets::assets::Install::open(&install_dir).unwrap();
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "tunnel-gameplay.yrm",
        "test".into(),
        bytes,
        Instant::now(),
    )
    .unwrap();
    let (mut editor, _ctx, root) = crate::editing::tests::editor();
    editor.attach(&scene);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let mut state = TunnelEdit::default();
    if input.is_some() {
        let tunnel = rows(editor.doc.as_ref().unwrap()).remove(0);
        state.draft = Some(TunnelDraft {
            replace: Some(tunnel.target.clone()),
            points: route(tunnel.entry, &tunnel.steps),
            direction: exit_facing(&tunnel).unwrap(),
        });
        state.save_draft(&mut editor, &scene).unwrap();
    } else {
        state.draft = Some(TunnelDraft {
            replace: None,
            points: (40..=48).map(|x| [x, 40]).collect(),
            direction: 2,
        });
        state.save_draft(&mut editor, &scene).unwrap();
    }
    let bytes = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        bytes
    );
    // Persist and reload through the engine's production map construction.
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tunnel-gameplay");
    std::fs::create_dir_all(&output_dir).unwrap();
    let output = output_dir.join(if input.is_some() {
        "redrawn.yrm"
    } else {
        "authored.yrm"
    });
    std::fs::write(&output, &bytes).unwrap();
    let mut scenario = headless_scenario::load(&install_dir, output.to_str().unwrap(), 71).unwrap();
    let terrain = scenario.sim().resolved_terrain().unwrap();
    let tube = terrain
        .tube_facts()
        .iter()
        .find(|tube| !tube.path_steps.is_empty())
        .unwrap()
        .clone();
    assert_eq!(tube.direction, tube.path_steps[0] & 7);
    for endpoint in [tube.entry, tube.exit] {
        let cell = terrain.cell(endpoint.0, endpoint.1).unwrap();
        assert!(
            !cell.ground_walk_blocked,
            "endpoint {endpoint:?} is blocked land {}",
            cell.land_type
        );
    }
    let owner = *scenario
        .sim()
        .houses()
        .keys()
        .find(|&&id| scenario.sim().owner_is_human(id))
        .unwrap();
    let owner_name = scenario
        .runtime
        .view()
        .interner()
        .resolve(owner)
        .to_string();
    let runtime = &mut scenario.runtime;
    let start = (tube.entry.0, tube.entry.1 + 3);
    let id = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            start.0,
            start.1,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("tank spawns on approach road");
    let mut issued_exit = false;
    let mut entered = false;
    let mut exited = false;
    for frame in 0..2500 {
        let tank = runtime.simulation.entities().get(id).unwrap();
        let at_entry = (tank.position.rx(), tank.position.ry()) == tube.entry
            && tank
                .navigation
                .path_replay()
                .remaining_directions()
                .is_empty();
        let mut commands = Vec::new();
        if frame == 0 || (!issued_exit && at_entry) {
            let target = if at_entry {
                issued_exit = true;
                tube.exit
            } else {
                tube.entry
            };
            commands.push(CommandEnvelope::new(
                owner,
                runtime.simulation.session().tick() + 1,
                Command::Move {
                    entity_id: id,
                    target_rx: target.0,
                    target_ry: target.1,
                },
            ));
        }
        runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let tank = runtime.simulation.entities().get(id).unwrap();
        if tank.low_bridge_tube_state.is_some() && !entered {
            println!("tank entered {:?} at frame {frame}", tube.entry);
            entered = true;
        }
        if entered && tank.low_bridge_tube_state.is_none() {
            let position = (tank.position.rx(), tank.position.ry());
            assert!(position.0.abs_diff(tube.exit.0) <= 1 && position.1.abs_diff(tube.exit.1) <= 1);
            println!(
                "tank exited {:?} at frame {frame}; map {}",
                position,
                output.display()
            );
            exited = true;
            break;
        }
    }
    assert!(
        entered && exited,
        "tank must drive into the entry, traverse the tube, and release at its exit"
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
