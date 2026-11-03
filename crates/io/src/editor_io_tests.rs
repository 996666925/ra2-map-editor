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
    let (loaded, _) = CsfFile::load_active(|name| manager.get_ref(name)).unwrap();
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
    };
    write_recovery(&path, &meta, &bytes).unwrap();
    let (restored, again) = read_recovery(&path).unwrap();
    assert_eq!(restored.target, meta.target);
    assert_eq!(restored.resource_profile, meta.resource_profile);
    assert_eq!(again, bytes);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let mut bad = fs::read(&path).unwrap();
    bad.truncate(14);
    fs::write(&path, bad).unwrap();
    assert!(read_recovery(&path).is_err());
    fs::remove_dir_all(root).unwrap();
}
