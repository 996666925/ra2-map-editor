use crate::editor_io::*;
use ra2_map_document::test_support::{document, fixture};
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "yr-p2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn recovery_worker_retains_the_document_ini_dialect() {
    let root = root();
    let mut bytes = ra2_map_document::new_map::NewMapOptions::default()
        .bytes()
        .unwrap();
    bytes.extend_from_slice(b"\n[Custom]\nX=first\nX=last\n");
    let (source, _) = ra2_map_document::source::SourceMap::parse_with_ares(
        "ares.map".into(),
        "test".into(),
        bytes,
        true,
    )
    .unwrap();
    let doc = ra2_map_document::document::MapDocument::new(source.edit_base.unwrap());
    let io = EditorIo::new(egui::Context::default(), root.clone()).unwrap();
    let receive = || {
        io.results
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    };
    assert!(matches!(receive(), Done::Scan(Ok(_))));
    let path = root.join("ares.yrrec");
    io.send(Job::Recover {
        snapshot: doc.snapshot(),
        path: path.clone(),
        meta: RecoveryMeta {
            version: 1,
            name: "ares.map".into(),
            directory: root.clone(),
            target: None,
            expected: None,
            resource_profile: None,
            reads_ares: None,
        },
    })
    .unwrap();
    assert!(matches!(receive(), Done::Recover { result: Ok(()), .. }));
    io.send(Job::Restore(path)).unwrap();
    let Done::Restore { result, .. } = receive() else {
        panic!("restore response")
    };
    let (base, meta) = result.unwrap();
    assert_eq!(meta.reads_ares, Some(true));
    assert!(base.reads_ares);
    assert_eq!(
        ra2_map_document::document::MapDocument::new(base).ini_text("Custom", "X"),
        "last"
    );
    drop(io);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_config_loads_mission_overrides_and_base_assets_without_changing_install() {
    use std::collections::BTreeMap;
    use vera20k_core::{
        assets::{asset_manager::AssetManager, csf_file::CsfFile},
        util::config::GameConfig,
    };

    let fixture = ra2_map_assets::test_support::Fixture::new();
    let empty: Vec<u8> = [0x43534620u32, 3, 0, 0, 0, 9]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let base = crate::campaign::merge_csf(
        &empty,
        &BTreeMap::from([("BASE:TEXT".into(), "原版文本".into())]),
    )
    .unwrap();
    fs::write(fixture.0.join("RA2MD.CSF"), &base).unwrap();
    fs::write(fixture.0.join("ARTMD.INI"), b"[Movies]\nBASE=base\n").unwrap();
    fs::write(fixture.0.join("expandmd01.mix"), b"archive bytes").unwrap();
    fs::write(fixture.0.join("VERA.INI"), b"[VERA]\nFullScreen=Yes\n").unwrap();
    let csf = crate::campaign::merge_csf(
        &base,
        &BTreeMap::from([("TSTX12345678".into(), "苏军正在逼近！".into())]),
    )
    .unwrap();
    let root = root();
    let game_dir = prepare_playtest_files(
        &root,
        &fixture.0,
        None,
        BTreeMap::from([
            ("ra2md.csf".into(), csf),
            ("artmd.ini".into(), b"[Movies]\nINTRO=intro\n".to_vec()),
            ("intro.bik".into(), b"mission movie".to_vec()),
            ("subtitlemd.txt".into(), b"mission subtitles".to_vec()),
        ]),
        &crate::playtest::PlayOptions::default(),
    )
    .unwrap();
    let config = GameConfig::load_from(&root, &root.join("vera20k.exe")).unwrap();
    assert_eq!(config.paths.ra2_dir, game_dir);
    let manager = AssetManager::from_ini_probe_root(&config.paths.ra2_dir).unwrap();
    let (loaded, _) =
        CsfFile::load_active(manager.ships_ares(), |name| manager.get_ref(name)).unwrap();
    assert_eq!(loaded.get("TSTX12345678"), Some("苏军正在逼近！"));
    assert_eq!(loaded.get("BASE:TEXT"), Some("原版文本"));
    assert_eq!(
        manager.get_ref("artmd.ini"),
        Some(b"[Movies]\nINTRO=intro\n".as_slice())
    );
    assert_eq!(
        manager.get_ref("intro.bik"),
        Some(b"mission movie".as_slice())
    );
    assert_eq!(
        manager.get_ref("subtitlemd.txt"),
        Some(b"mission subtitles".as_slice())
    );
    assert_eq!(
        manager.get_ref("gi.shp"),
        Some(fs::read(fixture.0.join("gi.shp")).unwrap().as_slice())
    );
    assert_eq!(
        fs::read(game_dir.join("expandmd01.mix")).unwrap(),
        b"archive bytes"
    );
    assert!(!game_dir.join("vera.ini").exists());
    // Writing a session's loose resource must never mutate the source install.
    fs::write(game_dir.join("temperatmd.ini"), b"session change").unwrap();
    assert_ne!(
        fs::read(fixture.0.join("temperatmd.ini")).unwrap(),
        b"session change"
    );
    assert_eq!(fs::read(fixture.0.join("RA2MD.CSF")).unwrap(), base);
    assert_eq!(
        fs::read(fixture.0.join("ARTMD.INI")).unwrap(),
        b"[Movies]\nBASE=base\n"
    );
    assert!(!fixture.0.join("intro.bik").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_without_mission_resources_keeps_the_original_asset_directory() {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    let root = root();
    let game_dir = prepare_playtest_files(
        &root,
        &fixture.0,
        None,
        Default::default(),
        &crate::playtest::PlayOptions::default(),
    )
    .unwrap();
    let config =
        vera20k_core::util::config::GameConfig::load_from(&root, &root.join("vera20k.exe"))
            .unwrap();
    assert_eq!(game_dir, fixture.0);
    assert_eq!(config.paths.ra2_dir, fixture.0);
    assert!(!root.join("assets").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_window_options_are_written_to_the_isolated_vera_profile() {
    let root = root();
    let settings = root.join("settings");
    let mut options = crate::playtest::PlayOptions::default();
    for display in [None, Some(r"\\.\DISPLAY2".to_owned())] {
        for full_screen in [true, false] {
            options.display = display.clone();
            options.full_screen = full_screen;
            options.zoom = if full_screen { 150 } else { 100 };
            options.write_settings(&settings).unwrap();
            let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(
                &fs::read(settings.join("VERA.INI")).unwrap(),
            )
            .unwrap();
            let vera = ini.section_or_empty("VERA");
            assert_eq!(vera.read_bool("FullScreen", !full_screen), full_screen);
            let video = ini.section_or_empty("Video");
            assert!(!video.is_present("ScreenWidth"));
            assert!(!video.is_present("ScreenHeight"));
            assert_eq!(vera.read_int("Zoom", -1), options.zoom as i32);
            assert!(!vera.is_present("WindowWidth"));
            assert!(!vera.is_present("WindowHeight"));
            assert_eq!(
                vera.read_name("Display", 128).unwrap_or_default(),
                display.as_deref().unwrap_or_default(),
            );
        }
    }
    options.display = Some("DISPLAY2\r\n[Audio]".into());
    assert!(options.write_settings(&root.join("invalid")).is_err());
    assert!(!root.join("invalid").exists());
    options.display = None;
    for zoom in [0, 100, 200] {
        options.zoom = zoom;
        options.write_settings(&settings).unwrap();
        let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(
            &fs::read(settings.join("VERA.INI")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ini.section_or_empty("VERA").read_int("Zoom", -1),
            zoom as i32
        );
    }
    for percent in [25, 50, 75, 99, 125, 10050] {
        options.zoom = percent;
        assert!(options.write_settings(&root.join("invalid")).is_err());
    }
    assert!(!root.join("invalid").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_checks_map_assets_and_detects_changed_resources() {
    use ra2_map_assets::{map_resources::MapResources, profile::Revision};

    let fixture = ra2_map_assets::test_support::Fixture::new();
    let root = root();
    let assets = root.join("任意素材名");
    fs::create_dir(&assets).unwrap();
    fs::write(assets.join("custom.shp"), b"custom sprite").unwrap();
    let source = root.join("campus.yrm");
    MapResources {
        directories: vec![ra2_map_assets::map_resources::ResourcePath::map(&assets)],
        ..Default::default()
    }
    .write(&source)
    .unwrap();
    let resources = MapResources::read(&source).unwrap().unwrap();
    let revision = Revision::scan_with_resources(&fixture.0, Some(&resources), &|| false).unwrap();
    assert_ne!(revision, Revision::scan(&fixture.0).unwrap());
    SessionResources::read(&fixture.0, Some(&source), Some(&revision.id())).unwrap();
    assert!(SessionResources::read(&fixture.0, None, Some(&revision.id())).is_err());
    fs::write(assets.join("custom.shp"), b"changed sprite").unwrap();
    assert!(SessionResources::read(&fixture.0, Some(&source), Some(&revision.id())).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn saved_and_published_maps_carry_ordered_sources_and_failed_saves_restore_sidecars() {
    use ra2_map_assets::map_resources::{AresMode, MapResources, PathBase, ResourcePath};
    let root = root();
    let assets = root.join("custom");
    fs::create_dir(&assets).unwrap();
    let source = root.join("source.yrm");
    let output = root.join("out");
    fs::create_dir(&output).unwrap();
    let resources = MapResources {
        directories: vec![
            ResourcePath::map(&assets),
            ResourcePath {
                path: "shared".into(),
                base: PathBase::Game,
            },
        ],
        ares: AresMode::Off,
        ..Default::default()
    };
    resources.write(&source).unwrap();
    let resolved = MapResources::read(&source).unwrap().unwrap();
    let target = output.join("published.map");
    replace_map_with_resources(&target, &fixture(), None, Some(&resolved), true).unwrap();
    assert_eq!(MapResources::read(&target).unwrap(), Some(resolved.clone()));
    let old_sidecar = fs::read(MapResources::sidecar(&target)).unwrap();
    let old_map = fs::read(&target).unwrap();
    assert!(
        replace_map_with_resources(&target, b"broken", Some(Stamp::bytes(&old_map)), None, true)
            .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), old_map);
    assert_eq!(
        fs::read(MapResources::sidecar(&target)).unwrap(),
        old_sidecar
    );
    let blocked = output.join("blocked.map");
    fs::create_dir(MapResources::sidecar(&blocked)).unwrap();
    assert!(replace_map_with_resources(&blocked, &fixture(), None, Some(&resolved), true).is_err());
    assert!(!blocked.exists());
    let valid = MapResources::default();
    replace_map_with_resources(
        &target,
        &fixture(),
        Some(Stamp::bytes(&old_map)),
        Some(&valid),
        true,
    )
    .unwrap();
    assert!(MapResources::read(&target).unwrap().unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_stages_map_assets_with_case_insensitive_precedence_and_write_isolation() {
    use std::collections::BTreeMap;
    use vera20k_core::assets::asset_manager::AssetManager;

    let fixture = ra2_map_assets::test_support::Fixture::new();
    let root = root();
    let assets = root.join("custom-resources");
    fs::create_dir(&assets).unwrap();
    fs::write(fixture.0.join("expandmd01.mix"), b"base archive").unwrap();
    fs::write(assets.join("GI.SHP"), b"map sprite").unwrap();
    fs::write(assets.join("custom.pal"), b"map palette").unwrap();
    fs::write(assets.join("isotem.pal"), b"map terrain palette").unwrap();
    fs::write(assets.join("OVERRIDE.SHP"), b"map resource").unwrap();
    fs::write(assets.join("RA2MD.INI"), b"do not import settings").unwrap();
    let session = root.join("session");
    fs::create_dir(&session).unwrap();
    let game = prepare_playtest_files(
        &session,
        &fixture.0,
        Some(&assets),
        BTreeMap::from([("override.shp".into(), b"mission override".to_vec())]),
        &Default::default(),
    )
    .unwrap();
    let manager = AssetManager::from_ini_probe_root(&game).unwrap();
    assert_eq!(manager.get_ref("gi.shp"), Some(b"map sprite".as_slice()));
    assert_eq!(
        manager.get_ref("custom.pal"),
        Some(b"map palette".as_slice())
    );
    assert_eq!(
        manager.get_ref("isotem.pal"),
        Some(b"map terrain palette".as_slice())
    );
    assert_eq!(
        manager.get_ref("override.shp"),
        Some(b"mission override".as_slice())
    );
    assert!(manager.get_ref("clear01.tem").is_some());
    assert_eq!(
        fs::read(game.join("expandmd01.mix")).unwrap(),
        b"base archive"
    );
    assert!(!game.join("ra2md.ini").exists());
    fs::write(game.join("gi.shp"), b"session edit").unwrap();
    assert_eq!(fs::read(assets.join("GI.SHP")).unwrap(), b"map sprite");
    assert_ne!(fs::read(fixture.0.join("gi.shp")).unwrap(), b"session edit");
    let second = root.join("no-payload");
    fs::create_dir(&second).unwrap();
    let game = prepare_playtest_files(
        &second,
        &fixture.0,
        Some(&assets),
        BTreeMap::new(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(fs::read(game.join("gi.shp")).unwrap(), b"map sprite");
    drop(manager);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn staged_art_retains_base_definitions_and_merged_properties() {
    use vera20k_core::rules::ini_parser::IniFile;

    let _dialect = vera20k_core::rules::ares_switch::read_ares_way(false);
    let mut art =
        IniFile::from_bytes(b"[BASE]\nHeight=3\nShadow=yes\n[Shared]\nHeight=1\nShadow=yes\n")
            .unwrap();
    art.merge(&IniFile::from_bytes(b"[Shared]\nHeight=7\n[Custom]\nHeight=11\n").unwrap());
    let decoded = IniFile::from_bytes(&encode_art(&art)).unwrap();
    assert_eq!(decoded.section_or_empty("BASE").read_int("Height", 0), 3);
    assert_eq!(decoded.section_or_empty("Shared").read_int("Height", 0), 7);
    assert!(
        decoded
            .section_or_empty("Shared")
            .read_bool("Shadow", false)
    );
    assert_eq!(decoded.section_or_empty("Custom").read_int("Height", 0), 11);
}

#[test]
fn embedded_preview_export_preserves_custom_pixels_and_rejects_missing_or_broken_pack() {
    let root = root();
    let path = root.join("自定义预览.png");
    let rgba = [1, 2, 3, 255, 120, 80, 40, 255, 250, 240, 230, 255];
    // The thumbnail is custom artwork, independent of terrain and game assets.
    let bytes = ra2_map_document::preview::embed(b"[Basic]\nName=Custom\n", 3, 1, &rgba).unwrap();
    let size = crate::publication::preview_png(&bytes, &path).unwrap();
    assert_eq!(size, [3, 1]);
    let image = image::open(&path).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (3, 1));
    assert_eq!(image.as_raw(), &rgba);
    // Replacing an existing output also works on Windows.
    crate::publication::preview_png(&bytes, &path).unwrap();
    let exported = fs::read(&path).unwrap();
    let error = crate::publication::preview_png(b"[Basic]\nName=NoPreview\n", &path).unwrap_err();
    assert!(format!("{error:#}").contains("PreviewPack"));
    assert!(
        crate::publication::preview_png(
            b"[Preview]\nSize=0,0,3,1\n[PreviewPack]\n1=invalid!\n",
            &path,
        )
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), exported);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hd_preview_failure_preserves_existing_png_and_snapshot() {
    let root = root();
    let path = root.join("高清预览.png");
    fs::write(&path, b"keep existing output").unwrap();
    let bytes = fixture();
    let original = bytes.clone();
    let error = crate::map_picture::export(
        &bytes,
        &root.join("missing-engine.exe"),
        &root,
        &root,
        &path,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("VERA20k"));
    assert_eq!(fs::read(&path).unwrap(), b"keep existing output");
    assert_eq!(bytes, original);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires retail RA2_DIR, VERA20K_EXE and a GPU"]
fn retail_engine_hd_preview_exports_the_current_snapshot() {
    let directory = PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR required"));
    let exe = PathBuf::from(std::env::var_os("VERA20K_EXE").expect("VERA20K_EXE required"));
    let root = root();
    let bytes = ra2_map_document::new_map::NewMapOptions {
        width: 40,
        height: 25,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let path = root.join("高清预览.png");
    fs::write(&path, b"previous output").unwrap();
    let size = crate::map_picture::export(&bytes, &exe, &directory, &root, &path).unwrap();
    let image = image::open(&path).unwrap().to_rgb8();
    assert_eq!([image.width(), image.height()], size);
    assert!(size.iter().all(|side| *side > 0 && *side <= 4096));
    assert!(image.pixels().any(|p| p.0 != [0; 3]));
    assert_eq!(fs::read_dir(root.join("map-pictures")).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires retail RA2_DIR, VERA20K_EXE, RA2_MAP, PLAYTEST_OUTPUT and a desktop"]
fn retail_map_assets_playtest_uses_the_loaded_profile() {
    use ra2_map_assets::{assets::Install, map_resources::MapResources};
    use ra2_map_document::{document::MapDocument, source::SourceMap};
    use std::time::{Duration, Instant};

    let directory = PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR required"));
    let source = PathBuf::from(std::env::var_os("RA2_MAP").expect("RA2_MAP required"));
    let exe = PathBuf::from(std::env::var_os("VERA20K_EXE").expect("VERA20K_EXE required"));
    let output =
        PathBuf::from(std::env::var_os("PLAYTEST_OUTPUT").expect("PLAYTEST_OUTPUT required"));
    fs::create_dir_all(&output).unwrap();
    let resources = MapResources::read(&source).unwrap();
    let install = Install::open_with_resources(&directory, resources, &|| false).unwrap();
    let bytes = fs::read(&source).unwrap();
    let (map, _) = SourceMap::parse(source.display().to_string(), "test".into(), bytes).unwrap();
    let mut doc = MapDocument::new(map.edit_base.unwrap());
    doc.resource_profile = Some(install.revision.id());
    let io = EditorIo::new(egui::Context::default(), output.clone()).unwrap();
    io.send(Job::Play {
        options: Default::default(),
        snapshot: doc.snapshot(),
        exe,
        directory,
        source: Some(source),
        root: output.clone(),
    })
    .unwrap();
    let mut process = loop {
        let result = io.results.recv_timeout(Duration::from_secs(120)).unwrap();
        if let Done::Play(result) = result {
            break result.unwrap();
        }
    };
    let started = Instant::now();
    let mut status = None;
    while started.elapsed() < Duration::from_secs(10) {
        status = process.child.try_wait().unwrap();
        if status.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let running = status.is_none();
    if running {
        process.child.kill().unwrap();
    }
    process.child.wait().unwrap();
    let provenance: serde_json::Value =
        serde_json::from_slice(&fs::read(process.folder.join("editor-session.json")).unwrap())
            .unwrap();
    assert_eq!(
        provenance["resource_profile"],
        serde_json::to_value(&*install.revision).unwrap()
    );
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": running,
            "running_seconds": started.elapsed().as_secs_f64(),
            "session": process.folder,
            "resource_profile": *install.revision,
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        running,
        "playtest exited early: {status:?}, logs: {}",
        process.folder.display()
    );
}
#[test]
fn atomic_save_replaces_on_windows_backs_up_original_and_rejects_external_changes() {
    let root = root();
    let path = root.join("中文地图.map");
    let original = fixture();
    let stamp = replace_map(&path, &original, None).unwrap();
    let mut doc = document();
    doc.waypoint(1, [3, 2]).unwrap();
    doc.commit("start");
    let edited = doc.snapshot().bytes().unwrap();
    let new = replace_map(&path, &edited, Some(stamp)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), edited);
    assert_ne!(new, stamp);
    let backups: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "bak"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path()).unwrap(), original);
    fs::write(&path, b"external edit").unwrap();
    assert!(replace_map(&path, &edited, Some(new)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"external edit");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn failed_validation_preserves_original_and_complete_temp() {
    let root = root();
    let path = root.join("original.map");
    let original = fixture();
    let stamp = replace_map(&path, &original, None).unwrap();
    assert!(replace_map(&path, b"[Map]\nSize=0,0,512,512\n", Some(stamp)).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(
        fs::read_dir(&root)
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|e| e == "tmp"))
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn recovery_roundtrip_is_independent_and_truncation_is_rejected() {
    let root = root();
    let path = root.join("recovery.yrrec");
    let original = root.join("original.map");
    let bytes = fixture();
    fs::write(&original, &bytes).unwrap();
    let meta = RecoveryMeta {
        version: 1,
        name: "中文地图.map".into(),
        directory: root.clone(),
        target: Some(original.clone()),
        expected: Some(Stamp::bytes(&bytes)),
        resource_profile: Some("schema:resources".into()),
        reads_ares: Some(true),
    };
    write_recovery(&path, &meta, &bytes).unwrap();
    let (restored, again) = read_recovery(&path).unwrap();
    assert_eq!(restored.target, meta.target);
    assert_eq!(restored.resource_profile, meta.resource_profile);
    assert_eq!(restored.reads_ares, Some(true));
    let mut legacy = serde_json::to_value(&meta).unwrap();
    legacy.as_object_mut().unwrap().remove("reads_ares");
    assert!(
        serde_json::from_value::<RecoveryMeta>(legacy)
            .unwrap()
            .reads_ares
            .is_none()
    );
    assert_eq!(again, bytes);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let mut bad = fs::read(&path).unwrap();
    bad.truncate(14);
    fs::write(&path, bad).unwrap();
    assert!(read_recovery(&path).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn playtest_spawn_and_preview_use_the_map_resources_dialect() {
    use ra2_map_assets::map_resources::{AresMode, MapResources};
    use vera20k_core::rules::{ares_switch::read_ares_way, ini_parser::IniFile};

    let game = ra2_map_assets::test_support::Fixture::new();
    let root = root();
    let source = root.join("mission.map");
    let mut bytes = fixture();
    bytes.extend_from_slice(b"\n[Basic]\nPlayer=Missing\nPlayer=Americans\n[Houses]\n0=Americans\n[Americans]\nCountry=Americans\n");
    let options = crate::playtest::PlayOptions {
        mission: true,
        ..Default::default()
    };
    for (mode, outer_ares) in [(AresMode::On, false), (AresMode::Off, true)] {
        MapResources {
            ares: mode,
            ..Default::default()
        }
        .write(&source)
        .unwrap();
        let session = root.join(mode.setting().ini_value());
        fs::create_dir(&session).unwrap();
        let _other_map = read_ares_way(outer_ares);
        let prepared = SessionResources::read(&game.0, Some(&source), None)
            .unwrap()
            .prepare(&bytes, &session, &options)
            .unwrap();
        let ini = prepared.map.ini.section_or_empty("Basic");
        assert_eq!(
            ini.read_name("Player", 64),
            Some(if outer_ares { "Missing" } else { "Americans" })
        );
        let spawn = options.spawn_ini_with_ares(&prepared.map, prepared.ares);
        if mode == AresMode::On {
            let spawn = IniFile::from_str(&spawn.unwrap());
            assert_eq!(
                spawn.section_or_empty("Settings").read_name("Ares", 16),
                Some("On")
            );
        } else {
            assert!(spawn.unwrap_err().to_string().contains("Missing"));
        }
        assert!(
            fs::read_to_string(session.join("settings/VERA.INI"))
                .unwrap()
                .contains(&format!("Ares={}", mode.setting().ini_value()))
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_config_uses_selected_ini_roots_and_keeps_original_art_available() {
    use ra2_map_assets::map_resources::{AresMode, MapResources, ResourcePath};
    let game = ra2_map_assets::test_support::Fixture::new();
    // Empty native archives keep the fixture independent of a retail installation.
    for name in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
    ] {
        fs::write(game.0.join(name), [0u8; 10]).unwrap();
    }
    fs::write(game.0.join("artmd.ini"), b"[BASE]\nHeight=3\n").unwrap();
    let root = root();
    let extra = root.join("extra");
    fs::create_dir(&extra).unwrap();
    fs::write(extra.join("custom.ini"), b"[CUSTOM]\nHeight=7\n").unwrap();
    let source = root.join("mission.map");
    MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ini: [
            ("art".into(), "custom.ini".into()),
            ("temperate".into(), "temperatmd.ini".into()),
        ]
        .into(),
        ares: AresMode::Off,
        ..Default::default()
    }
    .write(&source)
    .unwrap();
    let session = root.join("session");
    fs::create_dir(&session).unwrap();
    let prepared = SessionResources::read(&game.0, Some(&source), None)
        .unwrap()
        .prepare(&fixture(), &session, &Default::default())
        .unwrap();
    let text = fs::read_to_string(session.join("config.toml")).unwrap();
    assert_eq!(text.lines().filter(|s| *s == "[ini]").count(), 1);
    let loaded =
        vera20k_core::util::config::GameConfig::load_from(&session, &session.join("vera20k.exe"))
            .unwrap();
    assert_eq!(loaded.ini.art.as_deref(), Some("custom.ini"));
    assert_eq!(loaded.ini.theater_suffix.as_deref(), Some("editorres"));
    let roots = loaded.ini.resolve(|_| true).unwrap();
    let manager = vera20k_core::assets::asset_manager::AssetManager::with_mod_and_ares(
        &prepared.directory,
        None,
        vera20k_core::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        prepared.ares,
        Default::default(),
    )
    .unwrap()
    .with_selected_ini_roots(roots);
    let art = ra2_map_assets::assets::read_ini(&manager, manager.ini_roots().art()).unwrap();
    assert_eq!(
        ra2_map_assets::assets::read_ini(&manager, "artmd.ini")
            .unwrap()
            .section_or_empty("BASE")
            .read_int("Height", 0),
        3
    );
    assert_eq!(art.section_or_empty("CUSTOM").read_int("Height", 0), 7);
    assert_eq!(
        fs::read(prepared.directory.join("artmd.ini")).unwrap(),
        b"[BASE]\nHeight=3\n"
    );
    assert_eq!(
        fs::read(prepared.directory.join("custom.ini")).unwrap(),
        b"[CUSTOM]\nHeight=7\n"
    );
    drop(manager);
    fs::remove_dir_all(root).unwrap();
}
