use crate::{
    document::*,
    raw_ini::{RawIni, lcw_encode, pack_lines},
};
use std::sync::Arc;
use vera20k_core::{
    map::map_file,
    util::{base64::base64_decode, lcw, lzo},
};

use crate::test_support::{document, fixture};

fn numbered_document(section: &str, keys: &[&str]) -> MapDocument {
    let mut bytes = fixture();
    bytes.extend_from_slice(format!("\r\n[{section}]\r\n").as_bytes());
    let value = if section == "Smudge" {
        "CR1,2,2,0"
    } else {
        "Neutral,HTNK,256,2,2,0,Guard,None,0,-1,0,-1,1,1"
    };
    for key in keys {
        bytes.extend_from_slice(format!("{key}={value}\r\n").as_bytes());
    }
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap())
}

#[test]
fn added_records_use_the_smallest_section_local_key_and_preserve_legacy_spellings() {
    for section in ["Units", "Structures", "Infantry", "Aircraft", "Smudge"] {
        let mut doc = numbered_document(
            section,
            &["0", "2", "01", "+1", "3legacy", "999999999999999999999999"],
        );
        let before = doc.snapshot().bytes().unwrap();
        let original_records = doc.records();
        let other_section = if section == "Units" {
            "Aircraft"
        } else {
            "Units"
        };
        for _ in 0..2 {
            doc.add_record(other_section, [2, 2], "Neutral", "HTNK")
                .unwrap();
        }
        let state = doc.state_id;
        let first = doc.add_record(section, [2, 2], "Neutral", "HTNK").unwrap();
        let second = doc.add_record(section, [3, 2], "Neutral", "HTNK").unwrap();
        let current = doc.records();
        assert_eq!(current[&first].key, "1", "{section}");
        assert_eq!(current[&second].key, "3", "{section}");
        assert_eq!(
            doc.state_id, state,
            "pending records must reserve their keys"
        );
        for (id, original) in &original_records {
            assert_eq!(&current[id], original);
        }
        doc.cancel();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    let mut doc = numbered_document("Units", &["00", "+0", "legacy"]);
    let id = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&id].key, "0");
}

#[test]
fn record_key_allocation_tracks_deletion_cancel_undo_redo_and_reopen() {
    let mut doc = numbered_document("Units", &["0", "1", "2", "4"]);
    let before = doc.snapshot().bytes().unwrap();
    let deleted = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Units" && r.key == "1")
        .unwrap()
        .0;
    doc.set(Target::Record(deleted), None).unwrap();
    let replacement = doc.add_record("Units", [2, 2], "Neutral", "MTNK").unwrap();
    let next = doc.add_record("Units", [3, 2], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&replacement].key, "1");
    assert_eq!(doc.records()[&next].key, "3");
    assert!(doc.commit("replace and add"));
    let after = doc.snapshot().bytes().unwrap();
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);

    let id = doc.add_record("Units", [2, 3], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&id].key, "5");
    doc.cancel();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    let map = map_file::load_from_bytes(after.clone()).unwrap();
    let mut reopened = MapDocument::new(Baseline::parse(after.into(), &map).unwrap());
    let id = reopened
        .add_record("Units", [2, 3], "Neutral", "HTNK")
        .unwrap();
    assert_eq!(reopened.records()[&id].key, "5");

    doc.set(Target::Record(replacement), None).unwrap();
    assert!(doc.commit("delete"));
    assert!(doc.undo());
    let id = doc.add_record("Units", [2, 3], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&id].key, "5");
    doc.cancel();
    assert!(doc.redo());
    let id = doc.add_record("Units", [2, 3], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&id].key, "1");
}

#[test]
fn record_key_allocation_tracks_pending_checkpoints_and_failed_atomic_edits() {
    let mut doc = numbered_document("Units", &["0", "2"]);
    let prefix = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&prefix].key, "1");
    let checkpoint = doc.edit_checkpoint();
    let suffix = doc.add_record("Units", [3, 2], "Neutral", "HTNK").unwrap();
    assert_eq!(doc.records()[&suffix].key, "3");
    doc.restore_edit(&checkpoint).unwrap();
    let result: anyhow::Result<()> = doc.atomic(|doc| {
        let id = doc.add_record("Units", [3, 2], "Neutral", "HTNK")?;
        assert_eq!(doc.records()[&id].key, "3");
        anyhow::bail!("roll back this placement")
    });
    assert!(result.is_err());
    let id = doc.add_record("Units", [3, 2], "Neutral", "HTNK").unwrap();
    assert_eq!(id, suffix);
    assert_eq!(doc.records()[&id].key, "3");
    assert_eq!(doc.records()[&prefix].key, "1");
}

#[test]
fn edit_checkpoint_preserves_pending_prefix_record_ids_and_history() {
    let mut doc = document();
    let original = doc.snapshot().bytes().unwrap();
    doc.set(Target::Overlay([2, 2]), Some(Value::Overlay(1, 2)))
        .unwrap();
    let checkpoint = doc.edit_checkpoint();
    let prefix = doc.snapshot().bytes().unwrap();
    let id = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    doc.set(Target::Overlay([2, 2]), Some(Value::Overlay(3, 4)))
        .unwrap();
    doc.restore_edit(&checkpoint).unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), prefix);
    assert!(doc.undo_name().is_none());
    assert_eq!(
        doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap(),
        id
    );
    let after = doc.snapshot().bytes().unwrap();
    doc.commit("whole brush");
    assert!(doc.restore_edit(&checkpoint).is_err());
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.restore_edit(&checkpoint).is_err());
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    let mut other = document();
    assert!(other.restore_edit(&checkpoint).is_err());
}
#[test]
fn scratch_fork_preserves_pending_state_without_copying_history() {
    let mut doc = document();
    doc.set(Target::Overlay([2, 2]), Some(Value::Overlay(1, 2)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    let mut scratch = doc.fork();
    assert_ne!(scratch.id, doc.id);
    assert_eq!(scratch.snapshot().bytes().unwrap(), before);
    scratch
        .set(Target::Overlay([2, 2]), Some(Value::Overlay(2, 3)))
        .unwrap();
    scratch.commit("scratch");
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.commit("original");
    doc.undo();
    assert_eq!(doc.overlay([2, 2]), (255, 0));
}
#[test]
fn terrain_objects_smudges_and_overlays_move_save_reopen_and_undo() {
    let bytes = crate::new_map::NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut doc = MapDocument::new(Baseline::parse(Arc::from(bytes.clone()), &map).unwrap());
    let tree = doc
        .add_record("Terrain", [30, 30], "Neutral", "TREE01")
        .unwrap();
    let mark = doc
        .add_record("Smudge", [31, 30], "Neutral", "CR1")
        .unwrap();
    doc.set(Target::Overlay([32, 30]), Some(Value::Overlay(0, 3)))
        .unwrap();
    doc.commit("environment objects");
    let records = doc.records();
    assert_eq!(records[&tree].key, "30030");
    assert_eq!(records[&mark].value, b"CR1,31,30,0");
    doc.set(
        Target::Record(tree),
        Some(Value::Record(records[&tree].moved([30, 31]).unwrap())),
    )
    .unwrap();
    doc.set(
        Target::Record(mark),
        Some(Value::Record(records[&mark].moved([31, 31]).unwrap())),
    )
    .unwrap();
    doc.commit("move");
    let saved = doc.snapshot().bytes().unwrap();
    let parsed = map_file::load_from_bytes(saved.clone()).unwrap();
    assert_eq!(parsed.terrain_objects.len(), 1);
    assert_eq!(
        (parsed.terrain_objects[0].rx, parsed.terrain_objects[0].ry),
        (30, 31)
    );
    assert_eq!(parsed.smudges.len(), 1);
    assert_eq!((parsed.smudges[0].rx, parsed.smudges[0].ry), (31, 31));
    let reopened = MapDocument::new(Baseline::parse(Arc::from(saved.clone()), &parsed).unwrap());
    assert_eq!(reopened.overlay([32, 30]), (0, 3));
    assert_eq!(reopened.snapshot().bytes().unwrap(), saved);
    doc.set(Target::Record(tree), None).unwrap();
    doc.commit("delete");
    assert!(
        map_file::load_from_bytes(doc.snapshot().bytes().unwrap())
            .unwrap()
            .terrain_objects
            .is_empty()
    );
    assert!(doc.undo());
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
}

#[test]
fn moving_original_coordinate_key_removes_shadow_duplicates_and_preserves_old_radix() {
    for (format, key) in [(4, 30030), (3, 3870)] {
        let bytes = crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let text = String::from_utf8(bytes)
            .unwrap()
            .replace("NewINIFormat=4", &format!("NewINIFormat={format}"));
        let bytes = format!("{text}[Terrain]\r\n{key}=TREE01\r\n{key}=TREE02\r\n").into_bytes();
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc = MapDocument::new(Baseline::parse(Arc::from(bytes.clone()), &map).unwrap());
        let (id, record) = doc
            .records()
            .into_iter()
            .find(|(_, r)| r.section == "Terrain")
            .unwrap();
        assert_eq!(record.cell(), Some([30, 30]));
        let obstacle = doc
            .add_record("Terrain", [31, 31], "Neutral", "TREE03")
            .unwrap();
        assert!(
            doc.set(
                Target::Record(id),
                Some(Value::Record(record.moved([31, 31]).unwrap()))
            )
            .is_err()
        );
        doc.set(Target::Record(obstacle), None).unwrap();
        doc.set(
            Target::Record(id),
            Some(Value::Record(record.moved([30, 31]).unwrap())),
        )
        .unwrap();
        doc.commit("move original");
        let saved = doc.snapshot().bytes().unwrap();
        let parsed = map_file::load_from_bytes(saved).unwrap();
        assert_eq!(parsed.terrain_objects.len(), 1);
        assert_eq!(
            (parsed.terrain_objects[0].rx, parsed.terrain_objects[0].ry),
            (30, 31)
        );
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }
}
#[test]
fn formats_roundtrip_chunk_edges_literals_and_full_overlay_grid() {
    for len in [0, 1, 2, 3, 4, 62, 63, 64, 8191, 8192, 8193, 512 * 512] {
        for data in [
            vec![255u8; len],
            (0..len).map(|i| ((i * 37 + i / 11) % 256) as u8).collect(),
        ] {
            assert_eq!(base64_decode(&crate::raw_ini::base64(&data)).unwrap(), data);
            assert_eq!(
                lcw::decompress_chunks(&lcw_encode(&data), 512 * 512).unwrap(),
                data
            );
        }
    }
}
#[test]
fn pack_rewrite_replaces_nonnumeric_payload_keys_and_preserves_comments() {
    let raw = RawIni::parse(Arc::from(
        b"[OverlayPack]\r\nweird=AAAA\r\n; comment\r\n2=BBBB\r\n[Other]\r\nweird=untouched"
            .as_slice(),
    ))
    .unwrap();
    let packed = lcw_encode(&[255; 800]);
    let changes =
        std::collections::BTreeMap::from([("OverlayPack".into(), pack_lines(&packed, b"\r\n"))]);
    let bytes = raw.patch(&Default::default(), &changes, &Default::default(), true);
    let again = RawIni::parse(Arc::from(bytes.clone())).unwrap();
    assert_eq!(again.packed("OverlayPack").unwrap(), packed);
    assert!(bytes.windows(11).any(|w| w == b"; comment\r\n"));
    assert!(bytes.ends_with(b"weird=untouched"));
}
#[test]
fn sparse_terrain_edit_keeps_all_eleven_bytes_duplicate_and_sentinel_tail() {
    let mut doc = document();
    let original = doc.snapshot().bytes().unwrap();
    assert_eq!(original, fixture());
    assert_eq!(doc.terrain([2, 2]).unwrap().ice, 7);
    let value = TerrainValue {
        tile: 30,
        sub: 1,
        level: 4,
        ice: 7,
    };
    doc.set(Target::Terrain([2, 2]), Some(Value::Terrain(value)))
        .unwrap();
    doc.set(
        Target::Terrain([3, 2]),
        Some(Value::Terrain(TerrainValue { ice: 99, ..value })),
    )
    .unwrap();
    doc.commit("terrain");
    let bytes = doc.snapshot().bytes().unwrap();
    let raw = RawIni::parse(Arc::from(bytes.clone())).unwrap();
    let decoded = lzo::decompress_chunks(&raw.packed("IsoMapPack5").unwrap(), 1024).unwrap();
    let before = RawIni::parse(Arc::from(original)).unwrap();
    let before = lzo::decompress_chunks(&before.packed("IsoMapPack5").unwrap(), 1024).unwrap();
    assert_eq!(&decoded[..11], &before[..11]);
    assert_eq!(&decoded[11..15], &before[11..15]);
    assert_eq!(decoded[21], 7);
    assert_eq!(decoded[32], 99);
    assert_eq!(&decoded[33..], &before[22..]);
    assert!(
        !raw.lines
            .iter()
            .any(|line| matches!(line.section.as_str(), "Digest" | "Preview" | "PreviewPack"))
    );
    assert!(bytes.windows(15).any(|w| w == b"A=first\r\nA=seco"));
    let parsed = map_file::load_from_bytes(bytes).unwrap();
    assert!(
        parsed
            .cells
            .iter()
            .any(|c| c.rx == 3 && c.ry == 2 && c.tile_index == 30)
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), fixture());
}
#[test]
fn overlay_changes_preserve_unedited_raw_ids_and_frames() {
    let mut doc = document();
    doc.set(Target::Overlay([2, 2]), Some(Value::Overlay(250, 197)))
        .unwrap();
    doc.commit("unknown overlay");
    let bytes = doc.snapshot().bytes().unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut doc = MapDocument::new(Baseline::parse(Arc::from(bytes), &map).unwrap());
    doc.set(Target::Overlay([3, 2]), Some(Value::Overlay(102, 11)))
        .unwrap();
    doc.commit("ore");
    let bytes = doc.snapshot().bytes().unwrap();
    let raw = RawIni::parse(Arc::from(bytes)).unwrap();
    let ids = lcw::decompress_chunks(&raw.packed("OverlayPack").unwrap(), 512 * 512).unwrap();
    let frames =
        lcw::decompress_chunks(&raw.packed("OverlayDataPack").unwrap(), 512 * 512).unwrap();
    assert_eq!((ids[1026], frames[1026]), (250, 197));
    assert_eq!((ids[1027], frames[1027]), (102, 11));
    assert_eq!(ids[0], 255);
    assert_eq!(ids.len(), 512 * 512);
}
#[test]
fn stroke_coalesces_cancel_and_saved_state_survive_deterministic_redo_and_branch() {
    let mut doc = document();
    let initial = doc.terrain([2, 2]).unwrap();
    for level in [3, 4, 5] {
        doc.set(
            Target::Terrain([2, 2]),
            Some(Value::Terrain(TerrainValue { level, ..initial })),
        )
        .unwrap();
    }
    doc.commit("one stroke");
    assert!(doc.dirty());
    let state = doc.state_id;
    let bytes = doc.snapshot().bytes().unwrap();
    doc.saved_state = Some(state);
    doc.set(Target::Overlay([3, 2]), Some(Value::Overlay(102, 11)))
        .unwrap();
    doc.cancel();
    assert!(!doc.dirty());
    assert!(doc.undo());
    assert_eq!(doc.terrain([2, 2]), Some(initial));
    assert!(doc.dirty());
    assert!(!doc.undo());
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    assert!(!doc.dirty());
    doc.undo();
    doc.waypoint(1, [3, 2]).unwrap();
    doc.commit("branch");
    assert!(!doc.redo());
    assert!(doc.dirty());
    // An older asynchronous save cannot clear a newer state's dirty marker.
    doc.saved_state = Some(state);
    assert!(doc.dirty());
}
#[test]
fn moving_and_deleting_records_preserve_extra_fields_and_duplicate_undo() {
    let mut doc = document();
    let id = *doc
        .records()
        .iter()
        .find(|(_, r)| r.section == "Infantry")
        .unwrap()
        .0;
    let record = doc.records()[&id].clone();
    doc.set(
        Target::Record(id),
        Some(Value::Record(record.moved([3, 2]).unwrap())),
    )
    .unwrap();
    doc.commit("move");
    let bytes = doc.snapshot().bytes().unwrap();
    assert!(
        bytes
            .windows(b"\xD6\xD0 ; keep".len())
            .any(|w| w == b"\xD6\xD0 ; keep")
    );
    let map = map_file::load_from_bytes(bytes).unwrap();
    assert_eq!(map.entities.iter().filter(|e| e.type_id == "E1").count(), 1);
    doc.set(Target::Record(id), None).unwrap();
    doc.commit("delete");
    let bytes = doc.snapshot().bytes().unwrap();
    assert!(!bytes.windows(6).any(|w| w == b"SHADOW"));
    doc.undo();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), fixture());
    doc.waypoint(0, [3, 2]).unwrap();
    doc.add_record("Structures", [2, 2], "Neutral", "GAPOWR")
        .unwrap();
    doc.commit("placements");
    let map = map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    assert_eq!((map.waypoints[&0].rx, map.waypoints[&0].ry), (3, 2));
    assert!(map.entities.iter().any(|e| e.type_id == "GAPOWR"));
    assert!(doc.waypoint(702, [2, 2]).is_err());
    assert!(
        doc.add_record("Structures", [511, 511], "Neutral", "GAPOWR")
            .is_err()
    );
    assert!(
        doc.set(Target::Terrain([2, 2]), Some(Value::Overlay(1, 1)))
            .is_err()
    );
}
