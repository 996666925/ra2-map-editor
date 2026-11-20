use super::*;
use crate::{
    document::{Target, TerrainValue, Value},
    new_map::NewMapOptions,
    source::SourceMap,
};

#[test]
fn resident_layers_preserve_spatial_content_and_use_current_rules_and_lighting() {
    let mut target = document("TEMPERATE");
    target
        .set_ini("Lighting", "Ambient", Some(b"0.7".to_vec()))
        .unwrap();
    target
        .set_ini("HTNK", "Image", Some(b"CURRENT".to_vec()))
        .unwrap();
    target.commit("current settings");
    let mut source = document("SNOW");
    paint(&mut source, [20, 20], 17, 12);
    source
        .set(Target::Overlay([20, 20]), Some(Value::Overlay(24, 11)))
        .unwrap();
    let tank = source
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    let r = source
        .record(tank)
        .unwrap()
        .patched(&[(10, "1".into())])
        .unwrap();
    source
        .set(Target::Record(tank), Some(Value::Record(r)))
        .unwrap();
    source
        .add_record("Terrain", [21, 20], "Neutral", "TREE01")
        .unwrap();
    source
        .set_ini("Lighting", "Ambient", Some(b"0.1".to_vec()))
        .unwrap();
    source
        .set_ini("HTNK", "Image", Some(b"FOREIGN".to_vec()))
        .unwrap();
    source.commit("source");
    let mut workspace = Workspace::new(&target, "base".into()).unwrap();
    workspace.import("imported".into(), &source).unwrap();
    let layer = reopen(workspace.layer_bytes(1).unwrap());
    assert_eq!(layer.base.header.theater, "TEMPERATE");
    assert_eq!(layer.ini_text("Lighting", "Ambient"), "0.7");
    assert_eq!(layer.ini_text("HTNK", "Image"), "CURRENT");
    assert_eq!(layer.terrain([20, 20]).unwrap().tile, 17);
    assert_eq!(layer.terrain([20, 20]).unwrap().level, 12);
    assert_eq!(layer.overlay([20, 20]), (24, 11));
    assert!(
        layer
            .record_iter()
            .any(|(_, r)| r.section == "Terrain" && r.cell() == Some([21, 20]))
    );
    assert!(
        layer
            .record_iter()
            .any(|(_, r)| r.section == "Units" && r.fields()[10] == "1")
    );
    let mut copy = workspace.fork();
    assert!(Arc::ptr_eq(&copy.pieces[1].clip, &workspace.pieces[1].clip));
    copy.pieces[1].offset = [500, -500];
    assert_ne!(copy.pieces[1].offset, workspace.pieces[1].offset);
    assert_eq!(
        copy.layer_bytes(1).unwrap(),
        workspace.layer_bytes(1).unwrap(),
        "moving a layer never changes its decoded source"
    );
}

fn document(theater: &str) -> MapDocument {
    let bytes = NewMapOptions {
        width: 20,
        height: 20,
        theater: theater.into(),
        ..Default::default()
    }
    .bytes()
    .unwrap();
    reopen(bytes)
}

#[test]
fn saved_layout_restores_layers_offsets_underlays_and_survives_preview_publication() {
    let mut base = document("TEMPERATE");
    paint(&mut base, [20, 20], 92, 8);
    base.add_record("Terrain", [20, 20], "Neutral", "TREE01")
        .unwrap();
    base.add_record("Units", [21, 20], "Neutral", "HTNK")
        .unwrap();
    base.set(Target::Overlay([20, 20]), Some(Value::Overlay(25, 7)))
        .unwrap();
    base.commit("original");
    let mut w = Workspace::new(&base, "original".into()).unwrap();
    w.import("snow".into(), &document("SNOW")).unwrap();
    w.pieces[0].offset = [-12, -7];
    w.pieces[1].offset = [-12, -7];
    let composition = w.compose(&Default::default()).unwrap();
    let shift = composition.shift;
    let bytes = crate::preview::embed(&composition.bytes, 1, 1, &[1, 2, 3, 255]).unwrap();
    let saved_len = bytes.len();
    let reopened = reopen(bytes);
    let (mut restored, saved_shift) = Workspace::restore(&reopened).unwrap().unwrap();
    assert!(restored.matches(&reopened));
    assert_eq!(saved_shift, shift);
    assert_eq!(restored.pieces.len(), 2);
    assert_eq!(restored.pieces[1].name, "snow");
    assert_eq!(restored.pieces[1].theater, "SNOW");
    assert_eq!(restored.pieces[1].offset, [-12, -7]);
    for _ in 0..3 {
        let bytes = restored.compose(&Default::default()).unwrap().bytes;
        assert!(
            bytes.len() < saved_len + 100,
            "must not recursively embed earlier projects"
        );
        restored = Workspace::restore(&reopen(bytes)).unwrap().unwrap().0;
    }
    restored.pieces.remove(1);
    let composition = restored.compose(&Default::default()).unwrap();
    let at = [
        (20 - 12 + composition.shift[0]) as u16,
        (20 - 7 + composition.shift[1]) as u16,
    ];
    let result = reopen(composition.bytes);
    assert_eq!(result.terrain(at).unwrap().tile, 92);
    assert_eq!(result.overlay(at), (25, 7));
    assert!(
        result
            .record_iter()
            .any(|(_, r)| r.section == "Terrain" && r.cell() == Some(at))
    );
    assert!(result.record_iter().any(|(_, r)| r.section == "Units"));
}

#[test]
fn saved_layout_never_overwrites_later_normal_edits_and_rejects_corrupt_payloads() {
    let base = document("TEMPERATE");
    let mut w = Workspace::new(&base, "base".into()).unwrap();
    w.import("snow".into(), &document("SNOW")).unwrap();
    let bytes = w.compose(&Default::default()).unwrap().bytes;
    let mut edited = reopen(bytes);
    paint(&mut edited, [20, 20], 77, 3);
    edited.commit("normal edit");
    assert!(Workspace::restore(&edited).unwrap().is_none());
    let fresh = Workspace::new(&edited, "current".into()).unwrap();
    assert_eq!(
        reopen(fresh.compose(&Default::default()).unwrap().bytes)
            .terrain([20, 20])
            .unwrap()
            .tile,
        77
    );
    let mut bytes = base.snapshot().bytes().unwrap();
    bytes.extend_from_slice(b"\r\n[YRMapEditorLayersPack]\r\n1=broken\r\n");
    assert!(Workspace::restore(&reopen(bytes)).is_err());
}

#[test]
#[ignore = "opt-in whole-map finish performance audit"]
fn finish_composition_performance() {
    let options = NewMapOptions {
        width: 160,
        height: 160,
        ..Default::default()
    };
    let mut bytes = options.bytes().unwrap();
    bytes.extend_from_slice(b"\r\n[Terrain]\r\n");
    for x in 120..160u32 {
        for y in 120..160u32 {
            bytes.extend_from_slice(format!("{}=TREE01\r\n", y * 1000 + x).as_bytes());
        }
    }
    let mut target = reopen(bytes);
    let source = reopen(options.bytes().unwrap());
    let mut workspace = Workspace::new(&target, "base".into()).unwrap();
    workspace.import("other".into(), &target).unwrap();
    workspace.pieces[1].offset = [60, 0];
    workspace.import("empty".into(), &source).unwrap();
    workspace.pieces[2].offset = [0, 110];
    let start = std::time::Instant::now();
    let composition = workspace.compose(&Default::default()).unwrap();
    let compose = start.elapsed();
    let prepare_start = std::time::Instant::now();
    let prepared =
        PreparedReplacement::prepare(target.snapshot(), target.edit_revision(), composition.bytes)
            .unwrap();
    let prepare = prepare_start.elapsed();
    let apply_start = std::time::Instant::now();
    workspace.apply_prepared(&mut target, prepared).unwrap();
    eprintln!(
        "FINISH_AUDIT compose_ms={:.3} prepare_worker_ms={:.3} apply_ms={:.3} total_ms={:.3}",
        compose.as_secs_f64() * 1000.,
        prepare.as_secs_f64() * 1000.,
        apply_start.elapsed().as_secs_f64() * 1000.,
        start.elapsed().as_secs_f64() * 1000.
    );
    assert!(target.undo());
}
fn reopen(bytes: Vec<u8>) -> MapDocument {
    let (source, _) = SourceMap::parse("test.yrm".into(), "test".into(), bytes).unwrap();
    MapDocument::new(source.edit_base.unwrap())
}
fn paint(doc: &mut MapDocument, at: Cell, tile: i32, level: u8) {
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            tile,
            sub: 0,
            level,
            ice: 3,
        })),
    )
    .unwrap();
}

#[test]
fn different_theaters_import_all_spatial_layers_and_preserve_bridge_flags_and_followers() {
    let mut target = document("TEMPERATE");
    target.resource_profile = Some("current-mod".into());
    let mut source = document("SNOW");
    paint(&mut source, [20, 20], 42, 17);
    source
        .set(Target::Overlay([20, 20]), Some(Value::Overlay(24, 11)))
        .unwrap();
    let tank = source
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    let follower = source
        .add_record("Units", [21, 20], "Neutral", "HTNK")
        .unwrap();
    let key = source.record(tank).unwrap().key.clone();
    let mut r = source
        .record(follower)
        .unwrap()
        .patched(&[(7, "FOREIGNTAG".into()), (10, "1".into()), (11, key)])
        .unwrap();
    r.value.extend_from_slice(b",opaque-tail");
    source
        .set(Target::Record(follower), Some(Value::Record(r)))
        .unwrap();
    source
        .add_record("Terrain", [20, 21], "Neutral", "TREE01")
        .unwrap();
    source
        .set_ini("Tags", "FOREIGNTAG", Some(b"0,foreign,trigger".to_vec()))
        .unwrap();
    source.commit("source");
    let before = target.snapshot().bytes().unwrap();
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    w.import("snow".into(), &source).unwrap();
    let composition = w.compose(&Footprints::new()).unwrap();
    let after = reopen(composition.bytes.clone());
    assert_eq!(after.base.header.theater, "TEMPERATE");
    let at = after
        .record_iter()
        .find(|(_, r)| {
            r.section == "Units"
                && r.cell()
                    .is_some_and(|at| after.terrain(at).is_some_and(|v| v.tile == 42))
        })
        .unwrap()
        .1
        .cell()
        .unwrap();
    assert_eq!(after.terrain(at).unwrap().level, 17);
    assert_eq!(after.terrain(at).unwrap().ice, 3);
    assert_eq!(after.overlay(at), (24, 11));
    let follower = after
        .record_iter()
        .find(|(_, r)| r.value.ends_with(b"opaque-tail"))
        .unwrap()
        .1;
    assert_eq!(follower.fields()[7], "None");
    assert_eq!(follower.fields()[10], "1");
    let leader = after
        .record_iter()
        .find(|(_, r)| r.section == "Units" && r.key == follower.fields()[11])
        .unwrap()
        .1;
    assert_eq!(leader.cell().unwrap(), at);
    assert!(after.ini_text("Tags", "FOREIGNTAG").is_empty());
    assert!(after.record_iter().any(|(_, r)| r.section == "Terrain"));
    assert!(after.record_iter().any(|(_, r)| r.section == "Waypoints"
        && r.key.parse::<u32>().unwrap() >= u32::from(crate::new_map::MAX_PLAYERS)));
    assert_eq!(
        target.snapshot().bytes().unwrap(),
        before,
        "layout drafts must not edit the live document"
    );
    w.apply(&mut target, composition).unwrap();
    let applied = target.snapshot().bytes().unwrap();
    assert!(target.undo());
    assert_eq!(target.snapshot().bytes().unwrap(), before);
    assert!(target.redo());
    assert_eq!(target.snapshot().bytes().unwrap(), applied);
}

#[test]
fn original_map_and_its_cell_tags_move_with_negative_workspace_coordinates() {
    let mut target = document("TEMPERATE");
    paint(&mut target, [20, 20], 99, 4);
    target
        .set_ini("Tags", "KEEP", Some(b"0,keep,trigger".to_vec()))
        .unwrap();
    crate::mission::bind_cell(&mut target, [20, 20], "KEEP").unwrap();
    target.commit("setup");
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    w.import("other".into(), &document("DESERT")).unwrap();
    w.pieces[0].offset = [-12, -7];
    w.pieces[1].offset = [15, 15];
    let (options, shift) = w.geometry().unwrap();
    let expected = [(20 - 12 + shift[0]) as u16, (20 - 7 + shift[1]) as u16];
    let result = reopen(w.compose(&Footprints::new()).unwrap().bytes);
    assert_eq!(result.terrain(expected).unwrap().tile, 99);
    let key = crate::mission::cell_key(&result, expected).unwrap();
    assert_eq!(result.ini_text("CellTags", &key), "KEEP");
    assert_eq!(result.base.header.width, u32::from(options.width));
    assert_eq!(result.ini_text("Tags", "KEEP"), "0,keep,trigger");
}

#[test]
fn later_maps_replace_terrain_overlays_and_objects_but_keep_current_waypoint_ids() {
    let mut target = document("TEMPERATE");
    paint(&mut target, [20, 20], 8, 3);
    target
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    target
        .set(Target::Overlay([20, 20]), Some(Value::Overlay(25, 1)))
        .unwrap();
    target.commit("setup");
    let source = document("SNOW");
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    w.import("other".into(), &source).unwrap();
    w.pieces[1].offset = [0, 0];
    let result = reopen(w.compose(&Footprints::new()).unwrap().bytes);
    assert_eq!(result.terrain([20, 20]).unwrap().tile, 0);
    assert_eq!(result.overlay([20, 20]), (255, 0));
    assert!(!result.record_iter().any(|(_, r)| r.section == "Units"));
    assert!(
        result
            .record_iter()
            .any(|(_, r)| r.section == "Waypoints" && r.key == "0")
    );
    assert!(
        result
            .record_iter()
            .any(|(_, r)| r.section == "Waypoints"
                && r.key == crate::new_map::MAX_PLAYERS.to_string())
    );
}

#[test]
fn arbitrary_signed_positions_fit_without_clipping_and_layout_can_be_reapplied() {
    let mut target = document("TEMPERATE");
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    w.import("other".into(), &document("SNOW")).unwrap();
    for offset in [[-35, 12], [3, -31], [31, 1], [-2, -2], [0, 0]] {
        w.pieces[1].offset = offset;
        let composition = w.compose(&Footprints::new()).unwrap();
        let doc = reopen(composition.bytes.clone());
        let (_, shift) = w.geometry().unwrap();
        for p in &w.pieces {
            for cell in &p.clip.cells {
                doc.check_cell([
                    (i32::from(cell.at[0]) + p.offset[0] + shift[0]) as u16,
                    (i32::from(cell.at[1]) + p.offset[1] + shift[1]) as u16,
                ])
                .unwrap();
            }
        }
        w.apply(&mut target, composition).unwrap();
        assert!(w.matches(&target));
    }
}

#[test]
fn oversize_and_stale_layouts_fail_without_touching_current_document() {
    let mut target = document("TEMPERATE");
    let before = target.snapshot().bytes().unwrap();
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    w.import("other".into(), &document("SNOW")).unwrap();
    w.pieces[1].offset = [1024, 1024];
    assert!(w.compose(&Footprints::new()).is_err());
    assert_eq!(target.snapshot().bytes().unwrap(), before);
    w.pieces[1].offset = [30, 0];
    let composition = w.compose(&Footprints::new()).unwrap();
    paint(&mut target, [20, 20], 3, 3);
    target.commit("other edit");
    let edited = target.snapshot().bytes().unwrap();
    assert!(w.apply(&mut target, composition).is_err());
    assert_eq!(target.snapshot().bytes().unwrap(), edited);
}

#[test]
fn imported_legacy_trees_use_the_destination_coordinate_radix() {
    let target = document("TEMPERATE");
    let source = document("SNOW");
    let bytes = String::from_utf8(source.snapshot().bytes().unwrap())
        .unwrap()
        .replace("NewINIFormat=4", "NewINIFormat=3")
        + "\r\n[Terrain]\r\n2580=TREE01\r\n";
    let source = reopen(bytes.into_bytes());
    assert!(
        source
            .record_iter()
            .any(|(_, r)| r.section == "Terrain" && r.cell() == Some([20, 20]))
    );
    let clip = Clipboard::import_map(&source, &target).unwrap();
    let tree = clip
        .records
        .iter()
        .find(|(_, r)| r.section == "Terrain")
        .unwrap()
        .1
        .clone();
    assert_eq!(tree.coordinate_radix, 1000);
    assert_eq!(tree.cell(), Some([20, 20]));
    assert_eq!(tree.key, "20020");
}

#[test]
fn imported_layer_covers_original_and_moving_or_removing_it_restores_original_content_after_apply()
{
    let (source, _) = SourceMap::parse(
        "base.yrm".into(),
        "test".into(),
        NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    let mut target = MapDocument::new(source.edit_base.unwrap());
    paint(&mut target, [40, 40], 92, 8);
    target
        .set(Target::Overlay([40, 40]), Some(Value::Overlay(25, 7)))
        .unwrap();
    let unit = target
        .add_record("Units", [40, 40], "Neutral", "HTNK")
        .unwrap();
    target
        .add_record("Terrain", [41, 40], "Neutral", "TREE01")
        .unwrap();
    let original_unit = target.record(unit).unwrap().clone();
    target.commit("base content");
    let mut imported = document("SNOW");
    paint(&mut imported, [20, 20], 43, 4);
    imported
        .set(Target::Overlay([20, 20]), Some(Value::Overlay(24, 11)))
        .unwrap();
    imported
        .add_record("Infantry", [20, 20], "Neutral", "E1")
        .unwrap();
    imported.commit("import content");
    let mut w = Workspace::new(&target, "base".into()).unwrap();
    let original_layer = w.pieces[0]
        .clip
        .cells
        .iter()
        .find(|c| c.at == [40, 40])
        .unwrap()
        .terrain;
    w.import("snow".into(), &imported).unwrap();
    assert_eq!(
        w.pieces[1].offset,
        [20, 20],
        "imports start centered over the original map"
    );
    assert_eq!(
        w.size().unwrap(),
        [40, 40],
        "importing a smaller map must not expand the map automatically"
    );
    let covered = w.compose(&Default::default()).unwrap();
    let doc = reopen(covered.bytes.clone());
    assert_eq!(doc.terrain([40, 40]).unwrap().tile, 43);
    assert_eq!(doc.overlay([40, 40]), (24, 11));
    assert!(
        !doc.record_iter()
            .any(|(_, r)| r.section == "Units" || r.section == "Terrain")
    );
    assert!(
        doc.record_iter()
            .any(|(_, r)| r.section == "Infantry" && r.cell() == Some([40, 40]))
    );
    w.apply(&mut target, covered).unwrap();

    w.pieces[1].offset = [41, 20];
    let (_, shift) = w.geometry().unwrap();
    let original_at = [(40 + shift[0]) as u16, (40 + shift[1]) as u16];
    let imported_at = [(61 + shift[0]) as u16, (40 + shift[1]) as u16];
    let moved = w.compose(&Default::default()).unwrap();
    let doc = reopen(moved.bytes.clone());
    assert_eq!(doc.terrain(original_at).unwrap().tile, 92);
    assert_eq!(doc.terrain(original_at).unwrap().level, 8);
    assert_eq!(doc.overlay(original_at), (25, 7));
    let restored = doc
        .record_iter()
        .find(|(_, r)| r.section == "Units")
        .unwrap()
        .1;
    assert_eq!(restored, &original_unit.moved(original_at).unwrap());
    assert!(doc.record_iter().any(|(_, r)| r.section == "Terrain"));
    assert_eq!(doc.terrain(imported_at).unwrap().tile, 43);
    assert_eq!(
        w.pieces[0]
            .clip
            .cells
            .iter()
            .find(|c| c.at == [40, 40])
            .unwrap()
            .terrain,
        original_layer
    );
    assert!(
        w.pieces[0]
            .clip
            .records
            .iter()
            .any(|(_, r)| r == &original_unit)
    );
    w.apply(&mut target, moved).unwrap();

    w.pieces.remove(1);
    let removed = w.compose(&Default::default()).unwrap();
    let doc = reopen(removed.bytes.clone());
    assert_eq!(doc.terrain([40, 40]).unwrap().tile, 92);
    assert_eq!(doc.overlay([40, 40]), (25, 7));
    assert_eq!(
        doc.record_iter()
            .filter(|(_, r)| r.section == "Units")
            .count(),
        1
    );
    assert!(!doc.record_iter().any(|(_, r)| r.section == "Infantry"));
    w.apply(&mut target, removed).unwrap();
    assert!(target.undo());
    assert_eq!(target.terrain(original_at).unwrap().tile, 92);
    assert!(target.redo());
    assert_eq!(target.terrain([40, 40]).unwrap().tile, 92);
}
