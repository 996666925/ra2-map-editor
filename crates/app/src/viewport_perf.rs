//! Opt-in CPU frame audit against a real map, separate from GPU submission counts.
use crate::{camera::MapCamera, editing::Editor, mission_overlay::MissionOverlay};
use eframe::egui;
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "requires RA2_MAP_UI_PROFILE_MAP and local game assets"]
fn real_map_ui_profile() {
    let map = std::env::var("RA2_MAP_UI_PROFILE_MAP").expect("map path");
    let directory =
        PathBuf::from(std::env::var("RA2_MAP_UI_PROFILE_GAME").expect("game directory"));
    let output = PathBuf::from(std::env::var("RA2_MAP_UI_PROFILE_OUTPUT").expect("report path"));
    let output = if output.is_absolute() {
        output
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(output)
    };
    let mut install = ra2_map_assets::assets::Install::open(&directory).unwrap();
    let scene = ra2_map_scene::map::load_scene(&mut install, &map).unwrap();
    let ctx = egui::Context::default();
    let root = output
        .parent()
        .unwrap()
        .join(format!("ui-profile-{}", std::process::id()));
    let mut editor = Editor::new(ctx.clone(), root).unwrap();
    editor.attach(&scene);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.));
    let mut overlay = MissionOverlay::default();
    let dense = editor
        .marker_cache
        .markers
        .iter()
        .find(|m| m.radar_radius == 1.1 && m.image_rect.is_some())
        .and_then(|m| m.image_rect)
        .map_or(scene.bounds.center(), |r| r.center());
    let mut reports = Vec::new();
    for (name, center, zoom) in [
        (
            "fit",
            scene.bounds.center(),
            (1280. / scene.bounds.width()).min(800. / scene.bounds.height()),
        ),
        ("native", scene.bounds.center(), 1.),
        ("native_units", dense, 1.),
    ] {
        let camera = MapCamera { center, zoom };
        let mut frames = Vec::new();
        for frame in 0..12 {
            let started = Instant::now();
            let mut markers_ms = 0.;
            let mut labels_ms = 0.;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    let start = Instant::now();
                    editor.markers(ui.painter(), &scene, rect, &camera, Some(rect.center()));
                    markers_ms = start.elapsed().as_secs_f64() * 1000.;
                    let start = Instant::now();
                    overlay.draw(
                        ui.painter(),
                        editor.doc.as_ref().unwrap(),
                        rect,
                        &camera,
                        Some(rect.center()),
                        true,
                    );
                    labels_ms = start.elapsed().as_secs_f64() * 1000.;
                },
            );
            let shapes = output.shapes.len();
            let start = Instant::now();
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            let tessellate_ms = start.elapsed().as_secs_f64() * 1000.;
            if frame >= 3 {
                frames.push(serde_json::json!({"markers_ms":markers_ms,"labels_ms":labels_ms,"tessellate_ms":tessellate_ms,"total_ms":started.elapsed().as_secs_f64()*1000.,"shapes":shapes}));
            }
        }
        let mean = |key: &str| {
            frames.iter().map(|f| f[key].as_f64().unwrap()).sum::<f64>() / frames.len() as f64
        };
        reports.push(serde_json::json!({"view":name,"markers_ms":mean("markers_ms"),"labels_ms":mean("labels_ms"),"tessellate_ms":mean("tessellate_ms"),"total_ms":mean("total_ms"),"frames":frames}));
    }
    let mut overview = crate::overview::OverviewOverlay::default();
    let mut navigator_frames = Vec::new();
    for _ in 0..12 {
        let started = Instant::now();
        let result = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                overview.draw(
                    ui.painter(),
                    &scene,
                    &editor,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(256., 128.)),
                    [true; 4],
                );
            },
        );
        navigator_frames.push(serde_json::json!({"ms":started.elapsed().as_secs_f64()*1000.,"shapes":result.shapes.len()}));
    }
    let report = serde_json::json!({"map":map,"records":editor.records.len(),"draws":scene.visual.draws.len(),"views":reports,"navigator":navigator_frames});
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{report}");
}
