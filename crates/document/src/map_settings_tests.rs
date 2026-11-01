use super::*;
use crate::{
    base_nodes::Node,
    document::{Baseline, Target, TerrainValue, Value},
    new_map::NewMapOptions,
};
fn document() -> MapDocument {
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap())
}

#[test]
fn map_name_edits_preserve_map_data_roundtrip_utf8_and_undo() {
    let mut doc = document();
    let before = doc.snapshot().bytes().unwrap();
    rename(&mut doc, "  中文地图名称  ").unwrap();
    doc.commit("rename");
    let after = doc.snapshot().bytes().unwrap();
    let (source, _) =
        crate::source::SourceMap::parse("map.map".into(), "test".into(), after.clone()).unwrap();
    assert_eq!(source.title, "中文地图名称");
    let expected = String::from_utf8(before.clone())
        .unwrap()
        .replacen("Name=New Map", "Name=中文地图名称", 1)
        .into_bytes();
    assert_eq!(after, expected, "only the map name changes");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    for name in ["", "  ", "map;comment", "map\n[Injected]", &"中".repeat(41)] {
        assert!(rename(&mut doc, name).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), after);
    }
}

#[test]
fn resizing_across_grid_layouts_preserves_high_cells_edits_and_undo() {
    let mut doc = document();
    let terrain = TerrainValue {
        tile: 0,
        sub: 0,
        level: 5,
        ice: 7,
    };
    doc.set(Target::Terrain([24, 40]), Some(Value::Terrain(terrain)))
        .unwrap();
    doc.set(Target::Overlay([24, 40]), Some(Value::Overlay(102, 3)))
        .unwrap();
    doc.add_record("Units", [24, 40], "Americans", "MTNK")
        .unwrap();
    doc.waypoint(29, [24, 40]).unwrap();
    doc.commit("fixture");
    let original = doc.snapshot().bytes().unwrap();

    let mut options = Options::from_document(&doc);
    options.height = 600;
    options.local = [2, 4, 36, 590];
    options.offset = [0, 540];
    plan(&doc, options, &Footprints::new())
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let at = [564, 580];
    assert_eq!(doc.terrain(at), Some(terrain));
    assert_eq!(doc.overlay(at), (102, 3));
    assert_eq!(doc.overlay([52, 581]), (255, 0));
    assert!(
        doc.records()
            .values()
            .any(|r| r.section == "Units" && r.cell() == Some(at))
    );

    // Copy/paste and ordinary saves must use the same extended layout as resizing.
    let clip = crate::region::Clipboard::capture(
        &doc,
        &BTreeSet::from([at]),
        crate::region::RegionLayers::default(),
    )
    .unwrap();
    let paste = clip.plan(&doc, [565, 581]).unwrap();
    paste.apply(&mut doc, true).unwrap();
    doc.commit("large-map paste");
    let bytes = doc.snapshot().bytes().unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    assert!(
        map.cells
            .iter()
            .any(|c| [c.rx, c.ry] == [565, 581] && c.z == 5)
    );
    assert!(
        map.overlays
            .iter()
            .any(|c| [c.rx, c.ry] == [565, 581] && c.overlay_id == 102 && c.frame == 3)
    );
    let reopened = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    assert_eq!(reopened.terrain([565, 581]), Some(terrain));
    assert_eq!(reopened.overlay([565, 581]), (102, 3));
    assert_eq!(map.waypoints[&29].rx, at[0]);
    assert_eq!(map.waypoints[&29].ry, at[1]);
    assert!(doc.undo());
    let expanded = doc.snapshot().bytes().unwrap();

    let mut options = Options::from_document(&doc);
    options.height = 40;
    options.local = [2, 4, 36, 30];
    options.offset = [0, -540];
    plan(&doc, options, &Footprints::new())
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.terrain([24, 40]), Some(terrain));
    assert_eq!(doc.overlay([24, 40]), (102, 3));
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), expanded);
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), expanded);
}

#[test]
fn expansion_migrates_objects_packs_tags_nodes_tubes_and_script_cells_as_one_history_step() {
    let mut doc = document();
    mission::create(&mut doc, Kind::House, "Americans", "Americans", None).unwrap();
    mission::create(&mut doc, Kind::Script, "MOVE_SCRIPT", "Americans", None).unwrap();
    doc.set_ini(
        "MOVE_SCRIPT",
        "0",
        Some(format!("4,{}", 40 * 128 + 24).into_bytes()),
    )
    .unwrap();
    doc.set_ini("CellTags", "40024", Some(b"TAG".to_vec()))
        .unwrap();
    doc.set_ini("Tubes", "0", Some(b"24,40,2,26,40,2,2,-1".to_vec()))
        .unwrap();
    crate::base_nodes::write(
        &mut doc,
        "Americans",
        &[Node::new("GAPOWR", [24, 40]).unwrap()],
    )
    .unwrap();
    let object = doc
        .add_record("Structures", [24, 40], "Americans", "GAPOWR")
        .unwrap();
    doc.set(
        Target::Terrain([24, 40]),
        Some(Value::Terrain(TerrainValue {
            tile: 0,
            sub: 0,
            level: 5,
            ice: 7,
        })),
    )
    .unwrap();
    doc.set(Target::Overlay([25, 40]), Some(Value::Overlay(102, 3)))
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    let mut opts = Options::from_document(&doc);
    opts.width = 44;
    opts.height = 44;
    opts.offset = [2, 1];
    opts.local = [2, 4, 40, 34];
    let plan = plan(&doc, opts, &Footprints::new()).unwrap();
    assert!(plan.can_apply(), "{:?}", plan.summary);
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(plan.apply(&mut doc).unwrap());
    assert_eq!(doc.base.header.width, 44);
    assert_eq!(doc.terrain([27, 43]).unwrap().ice, 7);
    assert_eq!(doc.overlay([28, 43]), (102, 3));
    assert_eq!(doc.ini_text("CellTags", "43027"), "TAG");
    assert_eq!(
        doc.ini_text("MOVE_SCRIPT", "0"),
        format!("4,{}", 43 * 128 + 27)
    );
    assert_eq!(doc.ini_text("Tubes", "0"), "27,43,2,29,43,2,2,-1");
    assert_eq!(
        crate::base_nodes::read(&doc, "Americans").unwrap()[0].cell,
        [27, 43]
    );
    assert!(
        doc.records()
            .values()
            .any(|r| r.section == "Structures" && r.cell() == Some([27, 43]))
    );
    let after = doc.snapshot().bytes().unwrap();
    // Ordinary edits and structural replacements share the same history stack.
    doc.waypoint(15, [30, 45]).unwrap();
    doc.commit("after resize");
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.records().contains_key(&object));
    assert!(doc.undo());
    assert_eq!(doc.base.header.width, 40);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    assert!(doc.redo());
    assert_eq!(
        doc.records()
            .values()
            .filter(|r| r.section == "Waypoints")
            .count(),
        3
    );
}
#[test]
fn crop_preview_refuses_loss_and_stale_plans_and_clears_removed_follow_target() {
    let mut doc = document();
    let edge = doc
        .add_record("Units", [40, 1], "Americans", "MTNK")
        .unwrap();
    let follower = doc
        .add_record("Units", [24, 40], "Americans", "MTNK")
        .unwrap();
    let r = doc.records()[&follower]
        .patched(&[(11, doc.records()[&edge].key.clone())])
        .unwrap();
    doc.set(Target::Record(follower), Some(Value::Record(r)))
        .unwrap();
    doc.commit("units");
    let mut opts = Options::from_document(&doc);
    opts.width = 36;
    opts.local = [2, 4, 32, 30];
    let p = plan(&doc, opts.clone(), &Footprints::new()).unwrap();
    assert!(!p.can_apply());
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.source.starts_with("Units"))
    );
    opts.delete_outside = true;
    let p = plan(&doc, opts.clone(), &Footprints::new()).unwrap();
    assert!(p.can_apply());
    p.apply(&mut doc).unwrap();
    assert_eq!(
        doc.records()
            .values()
            .find(|r| r.section == "Units")
            .unwrap()
            .fields()[11],
        "-1"
    );
    assert!(p.apply(&mut doc).is_err());
}
#[test]
fn local_size_uses_native_clipping_and_only_changes_header_and_invalid_preview() {
    let mut doc = document();
    let before = doc.snapshot().bytes().unwrap();
    let mut opts = Options::from_document(&doc);
    opts.local = [3, 5, 30, 25];
    let p = plan(&doc, opts, &Footprints::new()).unwrap();
    assert!(p.can_apply());
    let raw = RawIni::parse(before.clone().into()).unwrap();
    let after = RawIni::parse(p.bytes.clone().into()).unwrap();
    assert_eq!(
        raw.packed("IsoMapPack5").unwrap(),
        after.packed("IsoMapPack5").unwrap()
    );
    p.apply(&mut doc).unwrap();
    let b = bounds(&doc);
    assert_eq!((b.off_fc, b.off_100, b.off_104, b.off_108), (3, 5, 30, 25));
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
#[test]
fn crop_cannot_erase_waypoints_bridges_or_unrepresentable_script_destinations() {
    let mut doc = document();
    doc.waypoint(20, [40, 1]).unwrap();
    doc.commit("edge waypoint");
    let mut opts = Options::from_document(&doc);
    opts.width = 36;
    opts.local = [2, 4, 32, 30];
    opts.delete_outside = true;
    let p = plan(&doc, opts.clone(), &Footprints::new()).unwrap();
    assert!(!p.can_apply());
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.blocking && a.source.starts_with("Waypoints"))
    );
    let id = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Waypoints" && r.key == "20")
        .unwrap()
        .0;
    doc.set(Target::Record(id), None).unwrap();
    doc.set(Target::Overlay([40, 1]), Some(Value::Overlay(24, 0)))
        .unwrap();
    doc.commit("edge bridge");
    let p = plan(&doc, opts.clone(), &Footprints::new()).unwrap();
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.blocking && a.source == "Overlay 24")
    );
    assert!(!p.can_apply());
    doc.set(Target::Overlay([40, 1]), Some(Value::Overlay(255, 0)))
        .unwrap();
    let mut action = mission::action(40, "0");
    for (i, value) in [34, 5, 4, 20].iter().enumerate() {
        action[i + 3] = value.to_string();
    }
    doc.set_ini(
        "Actions",
        "RECT",
        Some(mission::encode_list(&[action]).into_bytes()),
    )
    .unwrap();
    doc.commit("edge rectangle");
    let p = plan(&doc, opts, &Footprints::new()).unwrap();
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.blocking && a.source.starts_with("Actions/RECT"))
    );
    assert!(!p.can_apply());
}

#[test]
fn unchanged_settings_preserve_all_bytes_and_history() {
    let mut doc = document();
    let before = doc.snapshot().bytes().unwrap();
    let p = plan(&doc, Options::from_document(&doc), &Footprints::new()).unwrap();
    assert_eq!(p.bytes, before);
    assert!(!p.apply(&mut doc).unwrap());
    assert!(!doc.undo());
}

#[test]
fn odd_width_roundtrip_preserves_opaque_bytes_controls_and_visible_rectangle() {
    let mut doc = document();
    mission::create(&mut doc, Kind::House, "Americans", "Americans", None).unwrap();
    crate::base_nodes::write(
        &mut doc,
        "Americans",
        &[
            Node::parse(b"GAPOWR,0,0,opaque".to_vec()).unwrap(),
            Node::parse(b"-2,40,1".to_vec()).unwrap(),
            Node::parse(b"-3,40,1".to_vec()).unwrap(),
        ],
    )
    .unwrap();
    let id = doc
        .add_record("Units", [24, 40], "Americans", "MTNK")
        .unwrap();
    let mut r = doc.records()[&id].clone();
    r.value.extend_from_slice(b",\x81\xfe,private");
    doc.set(Target::Record(id), Some(Value::Record(r))).unwrap();
    doc.set_ini("Private", "Raw", Some(b"\x81\xfe".to_vec()))
        .unwrap();
    let mut action = mission::action(40, "0");
    for (i, v) in [3, 5, 28, 24].iter().enumerate() {
        action[i + 3] = v.to_string();
    }
    doc.set_ini(
        "Actions",
        "RECT",
        Some(mission::encode_list(&[action]).into_bytes()),
    )
    .unwrap();
    doc.set_ini(
        "Actions",
        "LEGACY",
        Some(b"1,11,0,0,0,0,0,0,\x81\xfe".to_vec()),
    )
    .unwrap();
    doc.commit("fixture");
    let original = doc.snapshot().bytes().unwrap();
    let nodes = crate::base_nodes::read(&doc, "Americans").unwrap();
    let mut o = Options::from_document(&doc);
    o.width = 43;
    o.height = 42;
    o.offset = [1, 1];
    plan(&doc, o, &Footprints::new())
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(crate::base_nodes::read(&doc, "Americans").unwrap(), nodes);
    assert_eq!(doc.ini_value("Private", "Raw").unwrap(), b"\x81\xfe");
    assert_eq!(
        doc.ini_value("Actions", "LEGACY").unwrap(),
        b"1,11,0,0,0,0,0,0,\x81\xfe"
    );
    let r = doc
        .records()
        .into_values()
        .find(|r| r.section == "Units")
        .unwrap();
    assert_eq!(r.cell(), Some([26, 43]));
    assert!(r.value.ends_with(b",\x81\xfe,private"));
    assert_eq!(
        &mission::decode_list(&doc.ini_text("Actions", "RECT"), false).unwrap()[0][3..7],
        &["4", "6", "28", "24"]
    );
    let mut o = Options::from_document(&doc);
    o.width = 40;
    o.height = 40;
    o.local = [2, 4, 36, 30];
    o.offset = [-1, -1];
    plan(&doc, o, &Footprints::new())
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(
        doc.records()
            .into_values()
            .find(|r| r.section == "Units")
            .unwrap()
            .cell(),
        Some([24, 40])
    );
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
}

#[test]
fn crop_detects_entire_foundation_and_rejects_ambiguous_spatial_sources_atomically() {
    let mut doc = document();
    doc.add_record("Structures", [36, 5], "Americans", "GAPOWR")
        .unwrap();
    doc.commit("edge foundation");
    let mut o = Options::from_document(&doc);
    o.width = 36;
    o.local = [2, 4, 32, 30];
    assert!(
        plan(&doc, o.clone(), &Footprints::new())
            .unwrap()
            .can_apply()
    );
    let footprints = BTreeMap::from([("GAPOWR".into(), vec![(0, 0), (4, 0)])]);
    let p = plan(&doc, o.clone(), &footprints).unwrap();
    assert!(!p.can_apply());
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.source.starts_with("Structures"))
    );
    let mut bytes = doc.snapshot().bytes().unwrap();
    bytes.extend_from_slice(b"\n[CellTags]\n40024=TAG\n40024=OTHER\n");
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let doc = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
    assert!(plan(&doc, o, &footprints).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
}

#[test]
fn legacy_radix_and_script_destination_overflow_are_not_silently_reinterpreted() {
    let mut doc = document();
    let mut bytes = String::from_utf8(doc.snapshot().bytes().unwrap())
        .unwrap()
        .replace("NewINIFormat=4", "NewINIFormat=0")
        .into_bytes();
    bytes.extend_from_slice(
        format!(
            "\n[CellTags]\n{}=TAG\n[Terrain]\n{}=TREE01,opaque\n",
            40 * 128 + 24,
            40 * 128 + 24
        )
        .as_bytes(),
    );
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    mission::create(&mut doc, Kind::Script, "MOVE", "Americans", None).unwrap();
    doc.set_ini(
        "MOVE",
        "0",
        Some(format!("4,{}", 40 * 128 + 24).into_bytes()),
    )
    .unwrap();
    doc.commit("legacy");
    let mut o = Options::from_document(&doc);
    o.width = 44;
    o.height = 44;
    o.offset = [2, 1];
    plan(&doc, o, &Footprints::new())
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(
        doc.ini_text("CellTags", &(43 * 128 + 27).to_string()),
        "TAG"
    );
    assert_eq!(
        doc.ini_text("Terrain", &(43 * 128 + 27).to_string()),
        "TREE01,opaque"
    );
    let mut o = Options::from_document(&doc);
    o.width = 180;
    o.height = 180;
    o.offset = [105, 0];
    // The tree and tag also use 128 encoding; remove them to isolate script safety.
    doc.set_ini("CellTags", &(43 * 128 + 27).to_string(), None)
        .unwrap();
    let id = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Terrain")
        .unwrap()
        .0;
    doc.set(Target::Record(id), None).unwrap();
    doc.commit("remove legacy records");
    let p = plan(&doc, o, &Footprints::new()).unwrap();
    assert!(!p.can_apply());
    assert!(
        p.summary
            .affected
            .iter()
            .any(|a| a.blocking && a.source.starts_with("MOVE/"))
    );
}
