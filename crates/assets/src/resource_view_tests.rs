use super::*;
use crate::map_resources::{PathBase, ResourcePath};

fn mix(path: &Path, items: &[(&str, &[u8])]) {
    let mut data = Vec::new();
    if items.is_empty() {
        data.extend([0; 4]);
    }
    data.extend((items.len() as u16).to_le_bytes());
    data.extend((items.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() as u32).to_le_bytes());
    let mut offset = 0u32;
    for (name, bytes) in items {
        data.extend(vera20k_core::assets::mix_hash::mix_hash(name).to_le_bytes());
        data.extend(offset.to_le_bytes());
        data.extend((bytes.len() as u32).to_le_bytes());
        offset += bytes.len() as u32;
    }
    for (_, bytes) in items {
        data.extend_from_slice(bytes);
    }
    fs::write(path, data).unwrap();
}
fn game() -> crate::test_support::Fixture {
    let f = crate::test_support::Fixture::new();
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
        mix(&f.0.join(name), &[]);
    }
    f
}
#[test]
fn stock_city_buildings_share_corrected_palette_in_preview_and_map() {
    let fixture = game();
    let shp = fs::read(fixture.0.join("gi.shp")).unwrap();
    let theater_mix = fixture.0.join("isotemp.mix");
    mix(
        &theater_mix,
        &[
            ("CITY01.SHP", &shp),
            ("CITY06.SHP", &shp),
            ("CITY05.SHP", &shp),
        ],
    );
    let theater_bytes = fs::read(&theater_mix).unwrap();
    fs::remove_file(theater_mix).unwrap();
    mix(
        &fixture.0.join("ra2.mix"),
        &[("isotemp.mix", &theater_bytes)],
    );
    fs::write(fixture.0.join("CITY02.SHP"), &shp).unwrap(); // Mod replacement.
    let mut city = [0; 768];
    city[3..6].copy_from_slice(&[25, 15, 5]);
    fs::write(fixture.0.join("citytem.pal"), city).unwrap();
    let mut custom = [0; 768];
    custom[3..6].copy_from_slice(&[12, 17, 21]);
    fs::write(fixture.0.join("customtem.pal"), custom).unwrap();
    fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[BuildingTypes]\n0=CITY01\n1=CITY02\n2=CITY05\n3=CITY06\n",
    )
    .unwrap();
    fs::write(
        fixture.0.join("artmd.ini"),
        b"[CITY01]\nFoundation=3x3\n[CITY02]\nFoundation=3x3\n\
          [CITY05]\nPalette=custom\n[CITY06]\nTerrainPalette=yes\n",
    )
    .unwrap();
    let install = crate::assets::Install::open(&fixture.0).unwrap();
    let sprites = install.sprite_palette(&install.catalog);
    for (id, color) in [
        ("CITY01", [100, 60, 20, 255]),
        ("CITY02", [60, 80, 100, 255]),
        ("CITY05", [48, 68, 84, 255]),
        ("CITY06", [60, 80, 100, 255]),
    ] {
        let preview = sprites
            .get("Structures", id)
            .unwrap()
            .image
            .as_ref()
            .unwrap();
        let sprite = sprites.objects[&("Structures".into(), id.into())].image(0);
        assert_eq!(&preview.rgba[..4], &color, "preview of {id}");
        assert_eq!(&sprite.rgba[..4], &color, "map sprite of {id}");
    }
}

fn dir(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    p
}
#[test]
fn resource_directory_order_high_priority_base_archives_and_low_fallback() {
    let f = game();
    let high = dir(&f.0, "high");
    let first = dir(&f.0, "first");
    let second = dir(&f.0, "second");
    let low = dir(&f.0, "low");
    fs::write(first.join("GI.SHP"), b"first").unwrap();
    fs::write(second.join("gi.shp"), b"second").unwrap();
    fs::write(high.join("gi.shp"), b"high").unwrap();
    fs::write(low.join("gi.shp"), b"low").unwrap();
    fs::write(low.join("only-low.shp"), b"fallback").unwrap();
    fs::write(low.join("archived.shp"), b"wrong").unwrap();
    mix(
        &f.0.join("conquer.mix"),
        &[("archived.shp", b"base archive")],
    );
    let mut config = MapResources {
        directories: vec![ResourcePath::map(&first), ResourcePath::map(&second)],
        low_priority: vec![ResourcePath::map(&low)],
        ..Default::default()
    };
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    let manager = view.manager().unwrap();
    assert_eq!(manager.get_ref("gi.shp"), Some(b"first".as_slice()));
    assert_eq!(
        manager.get_ref("archived.shp"),
        Some(b"base archive".as_slice())
    );
    assert_eq!(
        manager.get_ref("only-low.shp"),
        Some(b"fallback".as_slice())
    );
    assert_eq!(
        view.materialize("TEMPERATE", &|| false).unwrap(),
        ResourceView::open(&f.0, Some(&config), &|| false)
            .unwrap()
            .materialize("TEMPERATE", &|| false)
            .unwrap()
    );
    config.high_priority.push(ResourcePath::map(&high));
    assert_eq!(
        ResourceView::open(&f.0, Some(&config), &|| false)
            .unwrap()
            .manager()
            .unwrap()
            .get_ref("gi.shp"),
        Some(b"high".as_slice())
    );
    config.high_priority.clear();
    config.directories.swap(0, 1);
    assert_ne!(
        Revision::scan_with_resources(&f.0, Some(&config), &|| false).unwrap(),
        view.revision
    );
    assert_eq!(
        ResourceView::open(&f.0, Some(&config), &|| false)
            .unwrap()
            .manager()
            .unwrap()
            .get_ref("gi.shp"),
        Some(b"second".as_slice())
    );
}
#[test]
fn arbitrary_archives_keep_first_winners_without_replacing_stock_archives() {
    let f = game();
    let a = f.0.join("custom-one.dat");
    let b = f.0.join("custom-two.dat");
    mix(
        &a,
        &[
            ("shared.shp", b"first"),
            ("gi.shp", b"archive must lose to loose"),
        ],
    );
    mix(
        &b,
        &[("shared.shp", b"second"), ("only-extra.shp", b"extra")],
    );
    let a_mix = f.0.join("custom-one.mix");
    let b_mix = f.0.join("custom-two.mix");
    fs::rename(&a, &a_mix).unwrap();
    fs::rename(&b, &b_mix).unwrap();
    // Keep these archives out of the base startup wildcard families.
    mix(
        &f.0.join("langmd.mix"),
        &[
            ("shared.shp", b"language"),
            ("lang-only.shp", b"language retained"),
        ],
    );
    mix(
        &f.0.join("expandmd99.mix"),
        &[
            ("shared.shp", b"expansion"),
            ("expand-only.shp", b"expansion retained"),
        ],
    );
    let original = fs::read(f.0.join("langmd.mix")).unwrap();
    for mode in [AresMode::Off, AresMode::On] {
        let config = MapResources {
            archives: vec![ResourcePath::map(&a_mix), ResourcePath::map(&b_mix)],
            ares: mode,
            ..Default::default()
        };
        let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
        let manager = view.manager().unwrap();
        assert_eq!(manager.get_ref("shared.shp"), Some(b"first".as_slice()));
        assert_eq!(manager.get_ref("only-extra.shp"), Some(b"extra".as_slice()));
        assert_eq!(
            manager.get_ref("lang-only.shp"),
            Some(b"language retained".as_slice())
        );
        assert_eq!(
            manager.get_ref("expand-only.shp"),
            Some(b"expansion retained".as_slice())
        );
        assert_ne!(
            manager.get_ref("gi.shp"),
            Some(b"archive must lose to loose".as_slice())
        );
    }
    assert_eq!(fs::read(f.0.join("langmd.mix")).unwrap(), original);
}

#[test]
fn map_resource_refresh_reuses_verified_game_revision_and_reloads_new_sources() {
    let f = game();
    let first = dir(&f.0, "first-sources");
    let second = dir(&f.0, "second-sources");
    fs::write(first.join("first.shp"), b"first").unwrap();
    fs::write(second.join("second.shp"), b"second").unwrap();
    let initial = crate::assets::Install::open_with_resources(
        &f.0,
        Some(MapResources {
            directories: vec![ResourcePath::map(&first)],
            ..Default::default()
        }),
        &|| false,
    )
    .unwrap();
    let base = initial.base_revision().clone();
    assert_eq!(
        initial.manager.get_ref("first.shp"),
        Some(b"first".as_slice())
    );
    let refreshed = initial
        .reconfigure_map_resources(
            Some(MapResources {
                directories: vec![ResourcePath::map(&second)],
                ..Default::default()
            }),
            &|| false,
        )
        .unwrap();
    assert_eq!(*refreshed.base_revision(), base);
    assert_ne!(*refreshed.revision, *initial.revision);
    assert!(refreshed.manager.get_ref("first.shp").is_none());
    assert_eq!(
        refreshed.manager.get_ref("second.shp"),
        Some(b"second".as_slice())
    );
}

#[test]
fn a_single_flat_resource_directory_is_mounted_directly_but_unknown_files_fall_back_to_overlay() {
    let f = game();
    let extra = dir(&f.0, "single-source");
    fs::write(extra.join("custom.shp"), b"custom asset").unwrap();
    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let direct = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_eq!(
        direct.direct_mod_directory().unwrap(),
        Some(extra.canonicalize().unwrap())
    );
    let manager = direct.manager().unwrap();
    assert_eq!(
        manager.get_ref("custom.shp"),
        Some(b"custom asset".as_slice())
    );
    fs::write(extra.join("notes.md"), b"not a game resource").unwrap();
    let filtered = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert!(filtered.direct_mod_directory().unwrap().is_none());
    let manager = filtered.manager().unwrap();
    assert_eq!(
        manager.get_ref("custom.shp"),
        Some(b"custom asset".as_slice())
    );
    assert!(manager.get_ref("notes.md").is_none());
}

#[test]
fn map_resource_refresh_detects_base_game_changes_without_forgetting_the_cache() {
    let game = game();
    let initial = crate::assets::Install::open_with_resources(&game.0, None, &|| false).unwrap();
    let before = initial.base_revision().clone();
    fs::write(game.0.join("new-loose-resource.shp"), b"new game asset").unwrap();
    let refreshed = initial.reconfigure_map_resources(None, &|| false).unwrap();
    assert_ne!(*refreshed.base_revision(), before);
    assert_eq!(
        refreshed.manager.get_ref("new-loose-resource.shp"),
        Some(b"new game asset".as_slice())
    );
}

#[test]
fn ini_replace_roots_and_auto_ares_survives_staging() {
    let f = game();
    let extra = dir(&f.0, "extra");
    let lines = |items: &[&str]| {
        format!(
            "{}{}",
            items.join(&char::from(10).to_string()),
            char::from(10)
        )
    };
    fs::write(
        f.0.join("artmd.ini"),
        lines(&["[BASE]", "Height=3"]).as_bytes(),
    )
    .unwrap();
    fs::write(
        extra.join("custom.ini"),
        lines(&["[CUSTOM]", "Height=7"]).as_bytes(),
    )
    .unwrap();
    mix(&extra.join("ares.mix"), &[("extension.shp", b"ares")]);
    let mut config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    config.ini.insert("art".into(), "custom.ini".into());
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_eq!(view.ares, AresSetting::On);
    let manager = view.manager().unwrap();
    assert!(manager.ships_ares());
    let art = crate::assets::read_ini(&manager, manager.ini_roots().art()).unwrap();
    assert_eq!(art.section_or_empty("CUSTOM").read_int("Height", 0), 7);
    assert_eq!(art.section_or_empty("BASE").read_int("Height", 0), 0);
    config.ares = AresMode::Off;
    assert!(
        !ResourceView::open(&f.0, Some(&config), &|| false)
            .unwrap()
            .manager()
            .unwrap()
            .ships_ares()
    );
}
#[test]
fn imported_config_preserves_order_and_anchors_relative_paths_to_its_own_file() {
    let f = game();
    fs::create_dir(f.0.join("local")).unwrap();
    let path = f.0.join("resources.json");
    fs::write(
        &path,
        br#"{
        "directories": [
            {"path":"z", "base":"game"},
            {"path":"a", "base":"game"},
            {"path":"local", "base":"map"}
        ],
        "archives": [{"path":"custom.mix", "base":"game"}],
        "ini": {"art":"custom.ini"}
    }"#,
    )
    .unwrap();
    let config = MapResources::import(&path).unwrap();
    assert_eq!(config.directories[0].path, Path::new("z"));
    assert_eq!(config.directories[1].path, Path::new("a"));
    assert_eq!(config.directories[0].base, PathBase::Game);
    assert_eq!(
        config.directories[2].path,
        f.0.join("local").canonicalize().unwrap()
    );
    assert_eq!(config.ini["art"], "custom.ini");
    // Saving for another map keeps the imported resources attached to their origin.
    let moved = dir(&f.0, "maps").join("mission.map");
    config.write(&moved).unwrap();
    assert_eq!(MapResources::read(&moved).unwrap().unwrap(), config);
}

#[test]
fn real_art_entry_keeps_include_of_original_root_and_survives_runtime_staging() {
    let f = game();
    let extra = dir(&f.0, "includes");
    fs::write(f.0.join("artmd.ini"), b"[BASE]\nHeight=3\n").unwrap();
    fs::write(
        extra.join("custom.ini"),
        b"[CUSTOM]\nHeight=7\n[#include]\n0=artmd.ini\n",
    )
    .unwrap();
    let mut config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ares: AresMode::On,
        ..Default::default()
    };
    config.ini.insert("art".into(), "custom.ini".into());
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    let manager = view.manager().unwrap();
    assert_eq!(manager.ini_roots().art(), "custom.ini");
    let art = crate::assets::read_ini(&manager, manager.ini_roots().art()).unwrap();
    assert_eq!(art.section_or_empty("BASE").read_int("Height", 0), 3);
    assert_eq!(art.section_or_empty("CUSTOM").read_int("Height", 0), 7);
    assert_eq!(
        crate::assets::read_ini(&manager, "artmd.ini")
            .unwrap()
            .section_or_empty("BASE")
            .read_int("Height", 0),
        3
    );
    let staged = view.materialize("TEMPERATE", &|| false).unwrap();
    assert_eq!(
        fs::read(staged.join("custom.ini")).unwrap(),
        fs::read(extra.join("custom.ini")).unwrap()
    );
    assert!(
        !staged.join("artmd.ini").exists(),
        "the game installation stays outside the overlay"
    );
    assert_eq!(view.ini_config().art.as_deref(), Some("custom.ini"));
}

#[test]
fn missing_or_same_size_corrupt_cache_payload_is_rebuilt_from_source() {
    let f = game();
    let extra = dir(&f.0, "extra");
    fs::write(extra.join("unique.shp"), b"source").unwrap();
    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    let root = view.materialize("TEMPERATE", &|| false).unwrap();
    fs::remove_file(root.join("unique.shp")).unwrap();
    let restored = view.materialize("TEMPERATE", &|| false).unwrap();
    assert_eq!(fs::read(restored.join("unique.shp")).unwrap(), b"source");
    fs::write(restored.join("unique.shp"), b"broken").unwrap();
    let restored = view.materialize("TEMPERATE", &|| false).unwrap();
    assert_eq!(fs::read(restored.join("unique.shp")).unwrap(), b"source");
    assert_eq!(fs::read(extra.join("unique.shp")).unwrap(), b"source");
}

#[test]
fn terrain_tiles_are_linked_into_a_cache_sibling_of_the_resource_directory() {
    let f = game();
    let extra = dir(&f.0, "terrain-source");
    let source = extra.join("large-set.tem");
    fs::write(&source, b"terrain data").unwrap();
    let probe = extra.join("hard-link-probe.tem");
    let hard_links_supported = fs::hard_link(&source, &probe).is_ok();
    let _ = fs::remove_file(probe);
    let staged_probe = extra.join("staged-probe.tem");
    let method = stage_file_with_method(&source, &staged_probe).unwrap();
    if hard_links_supported {
        assert_eq!(method, StageMethod::HardLink);
    }
    fs::remove_file(staged_probe).unwrap();

    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    let root = view.materialize("TEMPERATE", &|| false).unwrap();
    let cache = root.parent().unwrap().to_owned();
    assert!(!root.starts_with(&extra));
    assert_eq!(
        fs::read(root.join("large-set.tem")).unwrap(),
        b"terrain data"
    );
    fs::remove_dir_all(&root).unwrap();
    let _ = fs::remove_dir(cache);
    assert_eq!(fs::read(&source).unwrap(), b"terrain data");
}

#[test]
fn large_terrain_sets_are_loaded_from_the_generated_expansion_mix() {
    let f = game();
    mix(
        &f.0.join("expandmd99.mix"),
        &[
            ("stock-expansion.shp", b"stock content"),
            ("tile001.tem", b"stock terrain"),
        ],
    );
    fs::write(f.0.join("tile000.tem"), b"installed loose terrain").unwrap();
    let extra = dir(&f.0, "large-terrain");
    for index in 0..258 {
        let bytes = match index {
            0 => b"map loose terrain".to_vec(),
            1 => b"map archive terrain".to_vec(),
            _ => vec![index as u8],
        };
        fs::write(extra.join(format!("tile{index:03}.tem")), bytes).unwrap();
    }
    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert!(view.direct_mod_directory().unwrap().is_none());
    let manager = view.manager().unwrap();
    assert_eq!(
        manager.get_ref("tile000.tem"),
        Some(b"map loose terrain".as_slice())
    );
    assert_eq!(
        manager.get_ref("tile001.tem"),
        Some(b"map archive terrain".as_slice())
    );
    for index in [128, 256] {
        let name = format!("tile{index:03}.tem");
        assert_eq!(manager.get_ref(&name), Some([index as u8].as_slice()));
    }
    assert_eq!(
        manager.get_ref("stock-expansion.shp"),
        Some(b"stock content".as_slice())
    );
    let root = view.materialize("TEMPERATE", &|| false).unwrap();
    assert!(
        root.join("expandmd99.mix").is_file(),
        "overlay root: {root:?}"
    );
}

#[test]
fn install_relative_subdirectories_survive_and_participate_in_resource_identity() {
    let f = game();
    let taunts = dir(&f.0, "Taunts");
    fs::write(taunts.join("tauam01.wav"), b"base taunt").unwrap();
    let extra = dir(&f.0, "extra");
    let custom = dir(&extra, "Taunts");
    fs::write(custom.join("tauam01.wav"), b"map taunt").unwrap();
    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let before = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_eq!(
        before
            .manager()
            .unwrap()
            .read_install_file("TAUNTS/TAUAM01.WAV")
            .unwrap(),
        b"map taunt"
    );
    fs::write(custom.join("tauam01.wav"), b"changed taunt").unwrap();
    let after = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_ne!(before.revision, after.revision);
    assert_eq!(
        after
            .manager()
            .unwrap()
            .read_install_file("taunts/tauam01.wav")
            .unwrap(),
        b"changed taunt"
    );
    assert_eq!(fs::read(taunts.join("tauam01.wav")).unwrap(), b"base taunt");
}

#[test]
fn low_priority_tracks_live_theater_mounts_instead_of_unmounted_archive_catalogue() {
    let f = game();
    let low = dir(&f.0, "low");
    let snow = f.0.join("temporary.mix");
    mix(&snow, &[("fallback.shp", b"snow")]);
    let bytes = fs::read(&snow).unwrap();
    fs::remove_file(snow).unwrap();
    mix(&f.0.join("ra2md.mix"), &[("isosnow.mix", &bytes)]);
    fs::write(low.join("fallback.shp"), b"fallback").unwrap();
    fs::copy(f.0.join("temperatmd.ini"), f.0.join("snowmd.ini")).unwrap();
    let config = MapResources {
        low_priority: vec![ResourcePath::map(&low)],
        ..Default::default()
    };
    let view = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_eq!(
        view.manager_for_theater("TEMPERATE")
            .unwrap()
            .get_ref("fallback.shp"),
        Some(b"fallback".as_slice())
    );
    assert_eq!(
        view.manager_for_theater("SNOW")
            .unwrap()
            .get_ref("fallback.shp"),
        Some(b"snow".as_slice())
    );
    assert_eq!(
        view.manager_for_theater("TEMPERATE")
            .unwrap()
            .get_ref("fallback.shp"),
        Some(b"fallback".as_slice())
    );
}
#[cfg(windows)]
#[test]
fn symlink_payload_changes_invalidate_resource_profiles_and_views() {
    let f = game();
    let extra = dir(&f.0, "extra");
    let source = f.0.join("payload.bin");
    fs::write(&source, b"before").unwrap();
    if std::os::windows::fs::symlink_file(&source, extra.join("linked.shp")).is_err() {
        return;
    }
    let config = MapResources {
        directories: vec![ResourcePath::map(&extra)],
        ..Default::default()
    };
    let before = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    let before_root = before.materialize("TEMPERATE", &|| false).unwrap();
    let before_manager = before.manager().unwrap();
    assert_eq!(
        before_manager.get_ref("linked.shp"),
        Some(b"before".as_slice())
    );
    fs::write(&source, b"after!").unwrap();
    let after = ResourceView::open(&f.0, Some(&config), &|| false).unwrap();
    assert_ne!(before.revision, after.revision);
    assert_ne!(
        before_root,
        after.materialize("TEMPERATE", &|| false).unwrap()
    );
    assert_eq!(
        before_manager.get_ref("linked.shp"),
        Some(b"before".as_slice())
    );
    assert_eq!(
        after.manager().unwrap().get_ref("linked.shp"),
        Some(b"after!".as_slice())
    );
}

#[test]
fn resource_refresh_detects_same_size_loose_ini_edits_with_same_stamp() {
    let game = game();
    let path = game.0.join("rulesmd.ini");
    fs::write(&path, b"[General]\nBuildSpeed=.7\n").unwrap();
    let stamp = crate::profile::FileStamp::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let initial = crate::assets::Install::open_with_resources(&game.0, None, &|| false).unwrap();
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    fs::write(&path, b"[General]\nBuildSpeed=.8\n").unwrap();
    file.set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(stamp, crate::profile::FileStamp::read(&path).unwrap());
    let refreshed = initial.reconfigure_map_resources(None, &|| false).unwrap();
    assert_ne!(initial.base_revision(), refreshed.base_revision());
    assert_eq!(
        refreshed.manager.get_ref("rulesmd.ini"),
        Some(b"[General]\nBuildSpeed=.8\n".as_slice())
    );
}
