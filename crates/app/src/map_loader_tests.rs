use super::*;

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
