use super::*;
use crate::test_support::Fixture;

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
