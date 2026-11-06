use super::*;

#[test]
fn mission_refreshes_reuse_the_worker_visual_across_history_and_then_reload_lighting() {
    use ra2_map_document::{document::MapDocument, lighting::Profile, new_map::NewMapOptions};
    let fixture = crate::test_support::map_loader_fixture();
    let path = fixture.0.join("worker.map");
    std::fs::write(
        &path,
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    let name = path.to_string_lossy().into_owned();
    let loader = MapLoader::new(egui::Context::default()).unwrap();
    loader.request(1, fixture.0.clone(), name.clone()).unwrap();
    let initial = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    let initial = initial.result.unwrap();
    let mut doc = MapDocument::new(initial.source.edit_base.as_ref().unwrap().clone());
    doc.set_ini("Events", "TRIGGER", Some(b"1,13,0,2".to_vec()))
        .unwrap();
    doc.commit("event");
    for generation in 2..=4 {
        if generation == 3 {
            assert!(doc.undo());
        }
        if generation == 4 {
            assert!(doc.redo());
        }
        loader
            .preview_profile(
                generation,
                fixture.0.clone(),
                name.clone(),
                doc.snapshot(),
                Profile::Normal,
            )
            .unwrap();
        let result = loader
            .results
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert_eq!(result.preview, Some((doc.id, doc.state_id)));
        assert!(result.timings.incremental_scene);
        assert!(result.timings.visual_reused);
        assert!(Arc::ptr_eq(
            &result.result.as_ref().unwrap().visual,
            &initial.visual
        ));
        assert_eq!(
            result.result.unwrap().source.bytes(),
            doc.snapshot().bytes().unwrap()
        );
    }
    doc.set_ini("Lighting", "Ambient", Some(b"0.5".to_vec()))
        .unwrap();
    doc.commit("lighting");
    loader
        .preview_profile(5, fixture.0.clone(), name, doc.snapshot(), Profile::Normal)
        .unwrap();
    let result = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    assert!(!result.timings.incremental_scene);
    assert!(!result.timings.visual_reused);
    assert!(!Arc::ptr_eq(
        &result.result.unwrap().visual,
        &initial.visual
    ));
    loader.invalidate_resources();
    loader
        .preview_profile(
            6,
            fixture.0.clone(),
            path.to_string_lossy().into_owned(),
            doc.snapshot(),
            Profile::Normal,
        )
        .unwrap();
    let refreshed = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    assert!(!refreshed.timings.incremental_scene);
    assert!(!refreshed.timings.visual_reused);
    assert!(refreshed.result.is_ok());
}

#[test]
fn worker_error_keeps_latest_generation() {
    let loader = MapLoader::new(egui::Context::default()).unwrap();
    loader
        .request(
            92,
            PathBuf::from("__missing_yr_p1_directory__"),
            "bad.map".into(),
        )
        .unwrap();
    let result = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(result.generation, 92);
    assert!(result.result.is_err());
}

#[test]
fn superseded_worker_does_not_publish_a_stale_load() {
    use std::sync::{Mutex, atomic::AtomicBool};
    let ctx = egui::Context::default();
    let (entered_tx, entered) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let release = Mutex::new(release);
    let first = AtomicBool::new(true);
    // Gate the first worker at a real progress notification, before the load.
    // Requesting a second generation must not wait on resource/GPU preparation.
    ctx.set_request_repaint_callback(move |_| {
        if first.swap(false, Ordering::AcqRel) {
            entered_tx.send(()).unwrap();
            release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
    });
    let loader = MapLoader::new(ctx).unwrap();
    loader
        .request(
            1,
            PathBuf::from("__p1_missing_install_first__"),
            "old.map".into(),
        )
        .unwrap();
    entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    loader
        .request(
            2,
            PathBuf::from("__p1_missing_install_second__"),
            "new.map".into(),
        )
        .unwrap();
    release_tx.send(()).unwrap();
    let result = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(result.generation, 2);
    assert!(result.result.is_err());
    assert!(
        loader.results.try_recv().is_err(),
        "old generation must not be published"
    );
}

#[test]
fn terrain_refreshes_reuse_worker_resources_and_rebuild_geometry_across_history() {
    use ra2_map_document::{
        document::{MapDocument, Target, TerrainValue, Value},
        lighting::Profile,
        new_map::NewMapOptions,
    };
    let fixture = crate::test_support::map_loader_fixture();
    let path = fixture.0.join("terrain-worker.map");
    std::fs::write(
        &path,
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    let name = path.to_string_lossy().into_owned();
    let loader = MapLoader::new(egui::Context::default()).unwrap();
    loader.request(1, fixture.0.clone(), name.clone()).unwrap();
    let initial = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap()
        .result
        .unwrap();
    let mut doc = MapDocument::new(initial.source.edit_base.as_ref().unwrap().clone());
    let value = doc.terrain([20, 20]).unwrap();
    doc.set(
        Target::Terrain([20, 20]),
        Some(Value::Terrain(TerrainValue { level: 4, ..value })),
    )
    .unwrap();
    doc.commit("height");
    for generation in 2..=4 {
        if generation == 3 {
            assert!(doc.undo());
        }
        if generation == 4 {
            assert!(doc.redo());
        }
        loader
            .preview_profile(
                generation,
                fixture.0.clone(),
                name.clone(),
                doc.snapshot(),
                Profile::Normal,
            )
            .unwrap();
        let result = loader
            .results
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert_eq!(result.preview, Some((doc.id, doc.state_id)));
        assert!(result.timings.incremental_scene);
        assert!(!result.timings.visual_reused);
        let scene = result.result.unwrap();
        assert!(Arc::ptr_eq(&scene.catalog, &initial.catalog));
        assert!(Arc::ptr_eq(&scene.sprites, &initial.sprites));
        assert!(!scene.resolved_terrain_is_shared(&initial));
        assert_eq!(
            scene
                .cells
                .iter()
                .find(|c| [c.rx, c.ry] == [20, 20])
                .unwrap()
                .z,
            if generation == 3 { value.level } else { 4 }
        );
    }
}
