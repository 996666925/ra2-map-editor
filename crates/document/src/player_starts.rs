//! Player start classification shared by navigation, publication and playtesting.
use crate::{
    document::{Cell, MapDocument, Target},
    new_map::MAX_PLAYERS,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use vera20k_core::map::playfield::PlayfieldBounds;

/// Ordinary maps use the native first eight slots. Extended declarations may
/// be sparse; older extended maps require a contiguous roster.
pub fn indices(declared: Option<u8>, starts: impl IntoIterator<Item = u8>) -> Vec<u8> {
    let starts: BTreeSet<_> = starts.into_iter().filter(|&n| n < MAX_PLAYERS).collect();
    let contiguous = (0..MAX_PLAYERS).take_while(|n| starts.contains(n)).count() as u8;
    let limit = declared
        .filter(|n| (9..=MAX_PLAYERS).contains(n))
        .unwrap_or(contiguous.max(8));
    starts.into_iter().filter(|&n| n < limit).collect()
}

pub fn from_document(doc: &MapDocument) -> Vec<u8> {
    indices(
        doc.ini_text("Basic", "MaxPlayers").parse().ok(),
        doc.records().values().filter_map(|r| {
            let slot = r.key.parse::<u8>().ok()?;
            (r.section == "Waypoints"
                && r.key == slot.to_string()
                && r.cell()
                    .is_some_and(|at| at != [0, 0] && doc.terrain(at).is_some()))
            .then_some(slot)
        }),
    )
}

/// Keep existing positions, fill missing slots and remove only excess starts.
/// The caller owns the history transaction; failed changes leave it intact.
pub fn set_count(doc: &mut MapDocument, count: u8) -> Result<()> {
    ensure!(
        (2..=MAX_PLAYERS).contains(&count),
        "出生点数量须在 2～{MAX_PLAYERS} 之间"
    );
    doc.atomic(|doc| {
        let old = from_document(doc);
        let records = doc.records();
        let mut positions = Vec::new();
        let mut missing = Vec::new();
        for slot in 0..count {
            let position = records
                .values()
                .find(|r| r.section == "Waypoints" && r.key == slot.to_string())
                .and_then(|r| r.cell())
                .filter(|&at| at != [0, 0] && doc.terrain(at).is_some());
            if let Some(at) = position {
                positions.push(at);
            } else {
                missing.push(slot);
            }
        }
        if !missing.is_empty() {
            let header = &doc.base.header;
            let bounds = PlayfieldBounds::from_map_header(header);
            let side = header.cell_grid().side();
            let mut candidates = Vec::new();
            for x in 1..side.min(1000) {
                for y in 1..side {
                    let at = [x, y];
                    if let Some(terrain) = doc.terrain(at)
                        && bounds.contains_geometry_packed(i32::from(x), i32::from(y))
                        && bounds.contains_height_aware_packed(
                            i32::from(x),
                            i32::from(y),
                            terrain.level as i8,
                            0,
                        )
                        && !positions.contains(&at)
                    {
                        candidates.push(at);
                    }
                }
            }
            for slot in missing {
                let at = candidates
                    .iter()
                    .copied()
                    .max_by_key(|&at| {
                        positions
                            .iter()
                            .map(|&other| distance(at, other))
                            .min()
                            .unwrap_or(0)
                    })
                    .context("可玩区域无法容纳所需的不同出生点")?;
                doc.waypoint(u32::from(slot), at)?;
                positions.push(at);
                candidates.retain(|&candidate| candidate != at);
            }
        }
        for (&id, record) in &records {
            if record.section == "Waypoints"
                && record
                    .key
                    .parse::<u8>()
                    .ok()
                    .is_some_and(|n| n >= count && old.contains(&n))
            {
                doc.set(Target::Record(id), None)?;
            }
        }
        doc.set_ini("Basic", "MaxPlayers", Some(count.to_string().into_bytes()))?;
        if !doc.ini_text("RandomMap", "NumPlayers").is_empty() {
            doc.set_ini(
                "RandomMap",
                "NumPlayers",
                Some(count.to_string().into_bytes()),
            )?;
        }
        Ok(())
    })
}

fn distance(a: Cell, b: Cell) -> i64 {
    let dx = i64::from(a[0]) - i64::from(b[0]);
    let dy = i64::from(a[1]) - i64::from(b[1]);
    4 * (dx - dy).pow(2) + (dx + dy).pow(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Baseline, new_map::NewMapOptions};
    use std::sync::Arc;

    fn document(players: u8) -> MapDocument {
        let bytes = NewMapOptions {
            width: 20,
            height: 20,
            level: 14,
            players,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        MapDocument::new(Baseline::parse(Arc::from(bytes), &map).unwrap())
    }

    #[test]
    fn declarations_distinguish_mission_waypoints_from_starts() {
        assert_eq!(indices(Some(2), [0, 1, 2, 7, 8, 29, 30]), vec![0, 1, 2, 7]);
        assert_eq!(indices(Some(30), [0, 29, 30]), vec![0, 29]);
        assert_eq!(indices(None, [0, 2, 7, 8, 29]), vec![0, 2, 7]);
        assert_eq!(indices(None, 0..30), (0..30).collect::<Vec<_>>());
    }

    #[test]
    fn changing_count_preserves_positions_and_mission_waypoints_and_roundtrips_history() {
        let mut doc = document(2);
        doc.waypoint(50, [20, 20]).unwrap();
        doc.set_ini("RandomMap", "NumPlayers", Some(b"2".to_vec()))
            .unwrap();
        doc.commit("mission waypoint");
        let before = doc.snapshot().bytes().unwrap();
        let original = doc.records();
        set_count(&mut doc, 30).unwrap();
        doc.commit("30 starts");
        assert_eq!(from_document(&doc), (0..30).collect::<Vec<_>>());
        let map =
            vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
        assert_eq!(
            vera20k_core::map::waypoints::skirmish_player_capacity(&map.ini),
            30
        );
        assert_eq!(map.waypoints.len(), 31);
        let bounds = PlayfieldBounds::from_map_header(&map.header);
        let mut positions = BTreeSet::new();
        for n in 0..30 {
            let w = map.waypoints[&n];
            assert!(positions.insert([w.rx, w.ry]));
            assert!(bounds.contains_height_aware_packed(i32::from(w.rx), i32::from(w.ry), 14, 0));
        }
        for r in original.values() {
            assert!(doc.records().values().any(|now| now == r));
        }
        let expanded = doc.snapshot().bytes().unwrap();
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), expanded);
        set_count(&mut doc, 2).unwrap();
        doc.commit("2 starts");
        let reduced = doc.snapshot().bytes().unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(reduced.clone()).unwrap();
        assert_eq!(
            map.waypoints.keys().copied().collect::<BTreeSet<_>>(),
            BTreeSet::from([0, 1, 50])
        );
        assert_eq!(
            vera20k_core::map::waypoints::skirmish_player_capacity(&map.ini),
            2
        );
        assert_eq!(doc.ini_text("RandomMap", "NumPlayers"), "2");
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), expanded);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), reduced);
    }

    #[test]
    fn failed_count_changes_preserve_pending_edits() {
        let mut doc = crate::test_support::document();
        doc.set_ini("Basic", "Name", Some(b"pending".to_vec()))
            .unwrap();
        let before = doc.snapshot().bytes().unwrap();
        for count in [0, 1, 30, 31] {
            assert!(set_count(&mut doc, count).is_err());
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
    }
}
