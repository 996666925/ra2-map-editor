use super::*;
use crate::test_support::Fixture;

#[test]
fn water_vehicle_turret_uses_selected_type_offset_with_aliased_geometry() {
    let fixture = Fixture::with_water_vehicle();
    for name in ["watermodel.vxl", "watermodeltur.vxl"] {
        std::fs::copy(fixture.0.join("water.vxl"), fixture.0.join(name)).unwrap();
    }
    let path = fixture.0.join("rulesmd.ini");
    let rules = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Image=SAM", "Image=SAM\nWaterImage=WATER")
        .replace(
            "[WATER]\nStrength=100",
            "[WATER]\nStrength=100\nImage=WATERMODEL\nTurret=yes",
        );
    std::fs::write(path, rules).unwrap();
    let path = fixture.0.join("artmd.ini");
    let art = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!("{art}\n[WATERMODEL]\nVoxel=yes\nTurretOffset=999\n"),
    )
    .unwrap();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let flat = crate::sprites::SpritePalette::load(&install, &install.catalog);
    std::fs::write(&path, format!("{art}\n[WATER]\nVoxel=yes\nTurretOffset=256\n[WATERMODEL]\nVoxel=yes\nTurretOffset=999\n")).unwrap();
    // Keep the old installation catalog to detect accidental reads from it.
    let refreshed = fixture.install();
    let changed = Catalog::load(&refreshed.manager, &refreshed.theater);
    let shifted = crate::sprites::SpritePalette::load(&install, &changed);
    let old = flat.water_objects["ROBO"].image(0);
    let new = shifted.water_objects["ROBO"].image(0);
    assert_eq!(new.info.name, "WATERMODEL.vxl");
    assert!(
        new.info.width > old.info.width || new.info.height > old.info.height,
        "an offset turret must extend beyond the identical unshifted hull"
    );
}

#[test]
fn startup_archives_and_ini_dialect_follow_the_editor_build() {
    use vera20k_core::util::ares_launch::AresSetting;
    let fixture = Fixture::new();
    // Valid one-entry old-format MIX; these stand in for startup archives.
    let mut mix = Vec::new();
    mix.extend(1u16.to_le_bytes());
    mix.extend(1u32.to_le_bytes());
    mix.extend(0i32.to_le_bytes());
    mix.extend(0u32.to_le_bytes());
    mix.extend(1u32.to_le_bytes());
    mix.push(0);
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
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
        "ares.mix",
    ] {
        std::fs::write(fixture.0.join(name), &mix).unwrap();
    }
    std::fs::write(fixture.0.join("rulesmd.ini"), b"[General]\nBuildSpeed=.7\n").unwrap();
    std::fs::write(fixture.0.join("artmd.ini"), b"[GI]\nFoundation=1x1\n").unwrap();
    std::fs::write(
        fixture.0.join("langrule.ini"),
        b"[General]\nBuildSpeed=.1\n",
    )
    .unwrap();
    std::fs::write(fixture.0.join("custom.ini"), b"[Base]\nX=first\nX=last\n").unwrap();
    let install = crate::assets::Install::open(&fixture.0).unwrap();
    let ares = vera20k_core::rules::ARES_BUILD;
    assert_eq!(
        install.manager.ares_setting(),
        if ares {
            AresSetting::On
        } else {
            AresSetting::Auto
        }
    );
    assert_eq!(install.manager.ares_mix_mounted(), ares);
    let ini = crate::assets::read_ini(&install.manager, "custom.ini").unwrap();
    assert_eq!(
        ini.section_or_empty("Base").read_string("X", "", 32),
        if ares { "last" } else { "first" }
    );
    assert!(
        vera20k_core::rules::retail_sources::RetailRulesSources::select(&install.manager)
            .unwrap()
            .langrule
            .is_none()
    );
}

#[test]
fn shp_vehicle_turret_ring_uses_the_engine_start_and_each_authored_heading() {
    for has_turret in [false, true] {
        let fixture = Fixture::with_shp_turret();
        let rules_path = fixture.0.join("rulesmd.ini");
        let rules = std::fs::read_to_string(&rules_path).unwrap().replace(
            "[TANK]\nStrength=100",
            &format!("[TANK]\nStrength=100\nTurret={has_turret}"),
        );
        std::fs::write(rules_path, rules).unwrap();
        let art_path = fixture.0.join("artmd.ini");
        let art = std::fs::read_to_string(&art_path).unwrap().replace(
            "Facings=1\nWalkFrames=0\nStandingFrames=1",
            "Facings=8\nWalkFrames=1\nStandingFrames=0\nTurretOffset=100",
        );
        std::fs::write(art_path, art).unwrap();
        let path = fixture.0.join("tank.shp");
        crate::test_support::write_shp(&path, 80);
        let mut shp = std::fs::read(&path).unwrap();
        for frame in 8..40 {
            let color = match frame {
                12 => 3,
                13 => 4,
                _ => 2,
            };
            shp[8 + 80 * 24 + frame * 2..8 + 80 * 24 + frame * 2 + 2].copy_from_slice(&[0, color]);
        }
        std::fs::write(path, shp).unwrap();
        let mut install = fixture.install();
        install.catalog = Catalog::load(&install.manager, &install.theater);
        assert_eq!(
            install.catalog.shp_vehicle_turrets.contains_key("TANK"),
            has_turret
        );
        let sprites = install.sprite_palette(&install.catalog);
        let object = &sprites.objects[&("Units".into(), "TANK".into())];
        for (facing, color) in [(0, 3), (8, 4), (248, 2)] {
            let image = object.image(facing);
            assert_eq!(
                image.indices,
                if has_turret {
                    vec![1, color]
                } else {
                    vec![1, 1]
                }
            );
            let expected = install
                .theater
                .unit_palette
                .color(if has_turret { color } else { 1 });
            assert_eq!(
                &image.rgba[4..8],
                &[expected.r, expected.g, expected.b, 255]
            );
            assert_eq!(
                [image.info.width, image.info.height],
                [2, 1],
                "TurretOffset does not move an SHP ring"
            );
        }
    }
}

#[test]
fn mounted_ares_selects_ini_syntax_and_includes_without_the_build_feature() {
    let fixture = Fixture::new();
    let mut mix = Vec::new();
    mix.extend(1u16.to_le_bytes());
    mix.extend(1u32.to_le_bytes());
    mix.extend(0i32.to_le_bytes());
    mix.extend(0u32.to_le_bytes());
    mix.extend(1u32.to_le_bytes());
    mix.push(0);
    std::fs::write(fixture.0.join("ares.mix"), mix).unwrap();
    std::fs::write(
        fixture.0.join("custom.ini"),
        b"[Base]\nX=first\nX=last\n[#include]\n0=child.ini\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("child.ini"),
        b"[Preamble]\nX=1\n[Child]:[Base]\nY=2\n",
    )
    .unwrap();
    let mut install = fixture.install();
    install.manager.load_nested("ares.mix").unwrap();
    assert!(install.manager.ares_form());
    let ini = crate::assets::read_ini(&install.manager, "custom.ini").unwrap();
    assert_eq!(
        ini.section_or_empty("Child").read_string("X", "", 64),
        "last"
    );
    assert_eq!(ini.section_or_empty("Child").read_int("Y", 0), 2);
}

#[test]
fn selected_ini_dialect_expands_ares_includes_and_preserves_input_files() {
    let fixture = Fixture::new();
    let root = b"[Base]\nHealth=1\nHealth=9\nOther=7\n[#include]\n0=child.ini\n";
    // The native include merge arm skips inheritance on its first header.
    let child = b"[Preamble]\nMarker=1\n[Child]:[Base]\nHealth=2\n[Names]\n+=A\n+=B\n";
    std::fs::write(fixture.0.join("custom.ini"), root).unwrap();
    std::fs::write(fixture.0.join("child.ini"), child).unwrap();
    let install = fixture.install();
    let ini = crate::assets::read_ini(&install.manager, "custom.ini").unwrap();
    if vera20k_core::rules::ARES_BUILD {
        let section = ini.section_or_empty("Child");
        assert_eq!(section.read_int("Health", 0), 2);
        assert_eq!(section.read_int("Other", 0), 7);
        let section = ini.section_or_empty("Names");
        assert_eq!(
            section
                .keys()
                .map(|key| section.read_string(key, "", 64))
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(ini.section_or_empty("Base").read_int("Health", 0), 9);
    } else {
        assert!(ini.section("Child").is_none());
        assert_eq!(ini.section_or_empty("Base").read_int("Health", 0), 1);
    }
    assert_eq!(std::fs::read(fixture.0.join("custom.ini")).unwrap(), root);
    assert_eq!(std::fs::read(fixture.0.join("child.ini")).unwrap(), child);
}

#[test]
fn mission_registries_keep_existing_positions_and_detect_registry_only_map_overrides() {
    let mut catalog = Catalog {
        rules: IniFile::from_str(
            "[Countries]\n0=ZCountry\n1=ACountry\n[SuperWeaponTypes]\n0=ZSuper\n[Warheads]\n0=ZWarhead\n",
        ),
        ..Default::default()
    };
    catalog.read_registries();
    let catalog = Arc::new(catalog);
    let local = catalog.with_map(
        b"[Countries]\n0=NewCountry\n[SuperWeaponTypes]\n0=NewSuper\n[Warheads]\n0=NewWarhead\n",
    );
    assert_eq!(
        local.mission_registries["Countries"],
        ["ZCountry", "ACountry", "NewCountry"]
    );
    assert_eq!(
        local.mission_registries["SuperWeaponTypes"],
        ["ZSuper", "NewSuper"]
    );
    assert_eq!(
        local.mission_registries["Warheads"],
        ["ZWarhead", "NewWarhead"]
    );
    assert_eq!(
        catalog.mission_registries["Countries"],
        ["ZCountry", "ACountry"]
    );
    assert!(Arc::ptr_eq(
        &local,
        &local.with_map(b"[Basic]\nName=unrelated\n")
    ));
}

fn csf(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = b" FSC".to_vec();
    for word in [3, entries.len() as u32, entries.len() as u32, 0, 9] {
        bytes.extend(word.to_le_bytes());
    }
    for (label, name) in entries {
        bytes.extend(b" LBL");
        bytes.extend(1u32.to_le_bytes());
        bytes.extend((label.len() as u32).to_le_bytes());
        bytes.extend(label.as_bytes());
        bytes.extend(b" RTS");
        bytes.extend((name.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(name.encode_utf16().flat_map(|unit| (!unit).to_le_bytes()));
    }
    bytes
}

#[test]
fn localized_object_names_search_and_map_overrides_preserve_type_identity() {
    let fixture = Fixture::new();
    let rules = b"[VehicleTypes]\n0=MTNK\n1=MODTANK\n2=UNNAMED\n[BuildingTypes]\n0=GAPOWR\n[MTNK]\nName=Grizzly Battle Tank\nUIName=name:grizzly\n[MODTANK]\nName=Custom Tank\nUIName=NAME:MISSING\n[GAPOWR]\nName=Power Plant\nUIName=NAME:POWER\n";
    std::fs::write(fixture.0.join("rulesmd.ini"), rules).unwrap();
    let strings = csf(&[
        ("NAME:GRIZZLY", "灰熊坦克"),
        ("NAME:POWER", "發電廠"),
        ("MOD:TANK", "自定義戰車"),
    ]);
    std::fs::write(fixture.0.join("ra2md.csf"), &strings).unwrap();
    let install = fixture.install();
    let catalog = Catalog::load(&install.manager, &install.theater);
    assert_eq!(catalog.object_label("MTNK"), "灰熊坦克 · MTNK");
    assert_eq!(catalog.object_name("gapowr"), "发电厂");
    assert!(catalog.object_matches("GAPOWR", "发电厂"));
    assert!(catalog.object_matches("GAPOWR", "發電廠"));
    for query in ["灰熊", "mtnk", "GRIZZLY"] {
        assert!(catalog.object_matches("MTNK", query));
    }
    assert!(!catalog.object_matches("MTNK", "发电厂"));
    assert_eq!(catalog.object_label("MODTANK"), "Custom Tank · MODTANK");
    assert_eq!(catalog.object_label("UNNAMED"), "UNNAMED");
    assert_eq!(catalog.object_label("UNKNOWN"), "UNKNOWN");
    let map = b"[VehicleTypes]\n2=MODNEW\n[MTNK]\nUIName=MOD:TANK\n[MODNEW]\nUIName=NAME:POWER\n";
    let custom = catalog.with_map(map);
    assert_eq!(custom.object_name("MTNK"), "自定义战车");
    assert_eq!(custom.object_name_zh("MTNK"), "自定义战车");
    assert_eq!(custom.object_name("MODNEW"), "发电厂");
    assert_eq!(catalog.object_name("MTNK"), "灰熊坦克");
    assert_eq!(
        custom.object_types["Units"],
        ["MTNK", "MODTANK", "UNNAMED", "MODNEW"]
    );
    assert_eq!(custom.overlays, catalog.overlays);
    assert_eq!(std::fs::read(fixture.0.join("rulesmd.ini")).unwrap(), rules);
    assert_eq!(std::fs::read(fixture.0.join("ra2md.csf")).unwrap(), strings);
    assert_eq!(
        catalog.strings.as_ref().unwrap().get("NAME:POWER"),
        Some("發電廠")
    );
}

#[test]
fn simplified_display_keeps_ids_unicode_and_english_and_converts_rule_name_fallback() {
    // Test display conversion on Unicode text without passing it through the
    // retail INI byte loader, which intentionally preserves single-byte data.
    let mut catalog = Catalog {
        rules: IniFile::from_str("[VehicleTypes]\n0=MOD\n[MOD]\nName=蘇聯採礦車 Mk.2 😀\n"),
        ..Default::default()
    };
    catalog.read_registries();
    assert_eq!(catalog.object_label("MOD"), "苏联采矿车 Mk.2 😀 · MOD");
    assert_eq!(catalog.object_types["Units"], ["MOD"]);
    assert!(catalog.object_matches("MOD", "採礦車"));
    assert!(catalog.object_matches("MOD", "采矿车"));
    assert_eq!(
        crate::chinese::simplified("發電廠 磁暴線圈 灰熊坦克 GI 😀"),
        "发电厂 磁暴线圈 灰熊坦克 GI 😀"
    );
}

#[test]
fn missing_or_invalid_language_tables_keep_object_types_available() {
    for strings in [None, Some(b"broken csf".as_slice())] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.0.join("rulesmd.ini"),
            b"[InfantryTypes]\n0=E1\n[E1]\nName=GI\nUIName=Name:E1\n",
        )
        .unwrap();
        if let Some(bytes) = strings {
            std::fs::write(fixture.0.join("ra2md.csf"), bytes).unwrap();
        }
        let install = fixture.install();
        let catalog = Catalog::load(&install.manager, &install.theater);
        assert_eq!(catalog.object_label("E1"), "GI · E1");
        assert_eq!(catalog.object_types["Infantry"], ["E1"]);
    }
}

#[test]
fn chinese_stock_fallback_translates_english_names_but_preserves_mod_renames() {
    let mut catalog = Catalog {
        rules: IniFile::from_str(
            "[VehicleTypes]\n0=TTNK\n1=HTNK\n2=MOD\n[TTNK]\nName=Tesla Tank\n[HTNK]\nName=Rhino Heavy Tank\n[MOD]\nName=Tesla Tank\n",
        ),
        ..Default::default()
    };
    catalog.read_registries();
    assert_eq!(catalog.object_name("TTNK"), "Tesla Tank");
    assert_eq!(catalog.object_name_zh("TTNK"), "磁能坦克");
    assert_eq!(catalog.object_name_zh("HTNK"), "犀牛坦克");
    assert_eq!(catalog.object_name_zh("MOD"), "Tesla Tank");
    let renamed = std::sync::Arc::new(catalog).with_map(b"[TTNK]\nName=Custom Tesla Tank\n");
    assert_eq!(renamed.object_name_zh("TTNK"), "Custom Tesla Tank");
    assert_eq!(renamed.object_name_zh("UNKNOWN"), "UNKNOWN");
}
