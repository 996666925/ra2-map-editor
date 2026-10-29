use super::*;
use crate::{document::Baseline, new_map::NewMapOptions, source::SourceMap};

fn setup() -> MapDocument {
    let (source, _) = SourceMap::parse(
        "region.map".into(),
        "fixture".into(),
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    MapDocument::new(source.edit_base.unwrap())
}
fn rect() -> BTreeSet<Cell> {
    [[20, 20], [21, 20], [20, 21], [21, 21]].into()
}
#[test]
fn same_theater_clipboards_require_matching_resource_and_schema_revision() {
    let mut doc = setup();
    doc.resource_profile = Some("schema-a:mod-a".into());
    let clip = Clipboard::capture(&doc, &rect(), RegionLayers::default()).unwrap();
    assert!(clip.plan(&doc, [20, 20]).is_ok());
    doc.resource_profile = Some("schema-a:mod-b".into());
    assert!(clip.plan(&doc, [20, 20]).is_err());
    doc.resource_profile = Some("schema-b:mod-a".into());
    assert!(clip.plan(&doc, [20, 20]).is_err());
    doc.resource_profile = None;
    assert!(clip.plan(&doc, [20, 20]).is_err());
}
#[test]
fn mixed_region_copy_preserves_opaque_fields_layers_and_unique_record_keys_on_reopen() {
    let mut doc = setup();
    let unit = doc
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    let other = doc
        .add_record("Units", [21, 20], "Neutral", "HTNK")
        .unwrap();
    let mut r = doc.records()[&unit]
        .patched(&[(6, "Hunt".into()), (11, doc.records()[&other].key.clone())])
        .unwrap();
    r.value.extend_from_slice(b",\xd6\xd0,opaque");
    doc.set(Target::Record(unit), Some(Value::Record(r.clone())))
        .unwrap();
    doc.add_record("Terrain", [20, 21], "Neutral", "TREE01")
        .unwrap();
    doc.waypoint(0, [21, 21]).unwrap();
    doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(5, 11)))
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    let clip = Clipboard::capture(&doc, &rect(), RegionLayers::default()).unwrap();
    assert_eq!(clip.records.len(), 3, "waypoints excluded by default");
    let plan = clip.plan(&doc, [23, 23]).unwrap();
    let ids = plan.apply(&mut doc, true).unwrap();
    let all = doc.records();
    let copied = &all[&ids[0]];
    assert_eq!(copied.cell(), Some([23, 23]));
    assert_ne!(copied.key, r.key);
    assert_eq!(copied.fields()[6], "Hunt");
    assert!(copied.value.ends_with(b",\xd6\xd0,opaque"));
    assert_eq!(
        copied.fields()[11],
        all[&ids[1]].key,
        "Follows remapped inside copied group"
    );
    assert_eq!(doc.overlay([23, 23]), (5, 11));
    assert_eq!(
        doc.records()
            .values()
            .filter(|r| r.section == "Waypoints")
            .count(),
        2,
        "initial two player starts retained"
    );
    doc.commit("paste");
    let after = doc.snapshot().bytes().unwrap();
    let (source, _) =
        SourceMap::parse("reopened.map".into(), "fixture".into(), after.clone()).unwrap();
    let reopened = MapDocument::new(source.edit_base.unwrap());
    let records = reopened.records();
    let identities: BTreeSet<_> = records.values().map(|r| (&r.section, &r.key)).collect();
    assert_eq!(identities.len(), records.len());
    assert!(
        records
            .values()
            .any(|r| r.cell() == Some([23, 23]) && r.value.ends_with(b",\xd6\xd0,opaque"))
    );
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}
#[test]
fn overlapping_move_uses_snapshot_and_retains_ids_and_disabled_layers() {
    let mut doc = setup();
    let tree = doc
        .add_record("Terrain", [20, 20], "Neutral", "TREE01")
        .unwrap();
    let unit = doc
        .add_record("Units", [21, 20], "Neutral", "HTNK")
        .unwrap();
    for (at, tile) in [([20, 20], 42), ([21, 20], 43)] {
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                tile,
                sub: 1,
                level: 3,
                ice: 7,
            })),
        )
        .unwrap();
    }
    doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(4, 9)))
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    let layers = RegionLayers {
        overlays: false,
        objects: [false, false, false, false, true, false],
        ..Default::default()
    };
    let clip = Clipboard::capture(&doc, &rect(), layers).unwrap();
    let plan = clip.plan(&doc, [21, 20]).unwrap();
    doc.atomic(|doc| {
        clear(doc, &clip, 0)?;
        plan.apply(doc, false)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(doc.terrain([21, 20]).unwrap().tile, 42);
    assert_eq!(doc.terrain([22, 20]).unwrap().tile, 43);
    assert_eq!(doc.terrain([22, 20]).unwrap().ice, 7);
    assert_eq!(doc.records()[&tree].cell(), Some([21, 20]));
    assert_eq!(
        doc.records()[&unit].cell(),
        Some([21, 20]),
        "disabled unit layer retained"
    );
    assert_eq!(doc.overlay([20, 20]), (4, 9));
    doc.commit("move");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
#[test]
fn boundary_rejection_and_late_record_failure_restore_the_whole_command() {
    let mut doc = setup();
    doc.add_record("Terrain", [20, 20], "Neutral", "TREE01")
        .unwrap();
    doc.commit("fixture");
    let clip = Clipboard::capture(&doc, &rect(), RegionLayers::default()).unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(clip.plan(&doc, [1, 1]).is_err());
    let mut plan = clip.plan(&doc, [23, 23]).unwrap();
    plan.records.push((
        999,
        Record {
            section: "Terrain".into(),
            key: "23023".into(),
            value: b"OTHER".to_vec(),
            coordinate_radix: 1000,
        },
    ));
    assert!(
        plan.apply(&mut doc, true).is_err(),
        "duplicate tree after terrain writes"
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert_eq!(doc.undo_name(), Some("fixture"));
}
#[test]
fn waypoint_copy_allocates_non_player_keys_and_hidden_layers_are_filtered() {
    let mut doc = setup();
    doc.waypoint(3, [20, 20]).unwrap();
    doc.commit("fixture");
    let layers = RegionLayers {
        waypoints: true,
        ..Default::default()
    };
    let hidden = layers.visible([false, false, false]);
    assert!(!hidden.overlays && !hidden.waypoints && hidden.objects.iter().all(|flag| !*flag));
    let clip = Clipboard::capture(&doc, &rect(), layers).unwrap();
    let ids = clip
        .plan(&doc, [23, 23])
        .unwrap()
        .apply(&mut doc, true)
        .unwrap();
    assert_eq!(doc.records()[&ids[0]].key, "30");
    assert_eq!(doc.records()[&ids[0]].cell(), Some([23, 23]));
}
#[test]
fn cloning_does_not_reuse_physical_duplicate_or_deleted_baseline_keys() {
    let mut doc = crate::test_support::document();
    let old = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Infantry")
        .unwrap();
    doc.set(Target::Record(old.0), None).unwrap();
    let new = doc.insert_record(old.1.moved([3, 2]).unwrap()).unwrap();
    assert_eq!(doc.records()[&new].key, "1");
    let saved = doc.snapshot().bytes().unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(saved.clone()).unwrap();
    let reopened = MapDocument::new(Baseline::parse(saved.into(), &map).unwrap());
    assert_eq!(
        reopened
            .records()
            .values()
            .filter(|r| r.section == "Infantry")
            .count(),
        1
    );
}

#[test]
fn cell_tags_copy_clear_move_overlap_and_history_preserve_shared_references() {
    use crate::mission;
    let mut doc = setup();
    for (at, tag) in [([20, 20], "TAGA"), ([21, 20], "TAGB"), ([24, 23], "OLD")] {
        let key = mission::cell_key(&doc, at).unwrap();
        doc.set_ini("CellTags", &key, Some(tag.as_bytes().to_vec()))
            .unwrap();
    }
    doc.commit("fixture");
    let original = doc.snapshot().bytes().unwrap();
    let clip = Clipboard::capture(&doc, &rect(), RegionLayers::default()).unwrap();
    clip.plan(&doc, [23, 23])
        .unwrap()
        .apply(&mut doc, true)
        .unwrap();
    assert_eq!(doc.ini_text("CellTags", "23023"), "TAGA");
    assert_eq!(doc.ini_text("CellTags", "23024"), "TAGB");
    assert_eq!(doc.ini_text("CellTags", "20020"), "TAGA");
    doc.commit("copy tags");
    let after = doc.snapshot().bytes().unwrap();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    let plan = clip.plan(&doc, [21, 20]).unwrap();
    doc.atomic(|doc| {
        clear(doc, &clip, 0)?;
        plan.apply(doc, false)?;
        Ok(())
    })
    .unwrap();
    assert!(doc.ini_value("CellTags", "20020").is_none());
    assert_eq!(doc.ini_text("CellTags", "20021"), "TAGA");
    assert_eq!(doc.ini_text("CellTags", "20022"), "TAGB");
    doc.commit("overlap move");
    let (source, _) = SourceMap::parse(
        "tags.map".into(),
        "test".into(),
        doc.snapshot().bytes().unwrap(),
    )
    .unwrap();
    let reopened = MapDocument::new(source.edit_base.unwrap());
    assert_eq!(reopened.ini_text("CellTags", "20022"), "TAGB");
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    let mut empty = clip.clone();
    for cell in &mut empty.cells {
        cell.cell_tag = None;
    }
    empty
        .plan(&doc, [23, 23])
        .unwrap()
        .apply(&mut doc, true)
        .unwrap();
    assert!(doc.ini_value("CellTags", "23023").is_none());
}
#[test]
fn cell_tag_layer_disable_legacy_radix_and_invalid_value_are_atomic() {
    use crate::mission;
    let mut doc = setup();
    doc.set_ini("CellTags", "20020", Some(b"SHARED".to_vec()))
        .unwrap();
    doc.commit("fixture");
    let hidden = RegionLayers::default().with_cell_tags_visible(false);
    let clip = Clipboard::capture(&doc, &rect(), hidden).unwrap();
    assert!(clip.cells.iter().all(|c| c.cell_tag.is_none()));
    clear(&mut doc, &clip, 0).unwrap();
    assert_eq!(doc.ini_text("CellTags", "20020"), "SHARED");
    let mut clip = Clipboard::capture(&doc, &rect(), RegionLayers::default()).unwrap();
    clip.cells[1].cell_tag = Some(b"bad\nvalue".to_vec());
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        clip.plan(&doc, [23, 23])
            .unwrap()
            .apply(&mut doc, true)
            .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    let bytes = String::from_utf8(
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap()
    .replace("NewINIFormat=4", "NewINIFormat=3");
    let (source, _) =
        SourceMap::parse("legacy.map".into(), "test".into(), bytes.into_bytes()).unwrap();
    let mut old = MapDocument::new(source.edit_base.unwrap());
    let key = mission::cell_key(&old, [20, 20]).unwrap();
    old.set_ini("CellTags", &key, Some(b"SHARED".to_vec()))
        .unwrap();
    let clip = Clipboard::capture(&old, &rect(), RegionLayers::default()).unwrap();
    clip.plan(&old, [23, 23])
        .unwrap()
        .apply(&mut old, true)
        .unwrap();
    assert_eq!(
        old.ini_text("CellTags", &(23 * 128 + 23).to_string()),
        "SHARED"
    );
}

#[test]
fn legacy_empty_tag_layer_does_not_limit_spatial_edits_but_tag_moves_remain_atomic() {
    let bytes = String::from_utf8(NewMapOptions::default().bytes().unwrap())
        .unwrap()
        .replace("NewINIFormat=4", "NewINIFormat=3");
    let (source, _) =
        SourceMap::parse("old.map".into(), "test".into(), bytes.into_bytes()).unwrap();
    let mut doc = MapDocument::new(source.edit_base.unwrap());
    let clip =
        Clipboard::capture(&doc, &BTreeSet::from([[80, 80]]), RegionLayers::default()).unwrap();
    let plan = clip.plan(&doc, [130, 80]).unwrap();
    plan.apply(&mut doc, true).unwrap();
    let destination =
        Clipboard::capture(&doc, &BTreeSet::from([[130, 80]]), RegionLayers::default()).unwrap();
    clear(&mut doc, &destination, 0).unwrap();
    let key = crate::mission::cell_key(&doc, [80, 80]).unwrap();
    doc.set_ini("CellTags", &key, Some(b"SHARED".to_vec()))
        .unwrap();
    let tagged =
        Clipboard::capture(&doc, &BTreeSet::from([[80, 80]]), RegionLayers::default()).unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(tagged.plan(&doc, [130, 80]).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn region_replaces_noncanonical_cell_tag_keys_and_rejects_duplicate_destinations() {
    let mut doc = setup();
    doc.set_ini("CellTags", "020020", Some(b"SHARED".to_vec()))
        .unwrap();
    let clip =
        Clipboard::capture(&doc, &BTreeSet::from([[20, 20]]), RegionLayers::default()).unwrap();
    let mut empty = clip.clone();
    empty.cells[0].cell_tag = None;
    empty
        .plan(&doc, [20, 20])
        .unwrap()
        .apply(&mut doc, true)
        .unwrap();
    assert!(doc.ini_entries("CellTags").is_empty());
    doc.set_ini("CellTags", "23023", Some(b"ONE".to_vec()))
        .unwrap();
    doc.set_ini("CellTags", "023023", Some(b"TWO".to_vec()))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        clip.plan(&doc, [23, 23])
            .unwrap()
            .apply(&mut doc, true)
            .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
