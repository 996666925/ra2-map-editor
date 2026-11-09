//! Direct object/waypoint movement preserves records and uses placement validation.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editing::tests::{editor, terrain_scene};

    #[test]
    fn direct_drag_and_click_moves_share_history_and_preserve_all_record_fields() {
        let scene = terrain_scene();
        for section in [
            "Structures",
            "Units",
            "Infantry",
            "Aircraft",
            "Terrain",
            "Smudge",
            "Waypoints",
        ] {
            let (mut e, _, root) = editor();
            e.attach(&scene);
            let doc = e.doc.as_mut().unwrap();
            let id = if section == "Waypoints" {
                doc.waypoint(42, [20, 20]).unwrap();
                doc.record_iter()
                    .find(|(_, r)| r.section == "Waypoints" && r.key == "42")
                    .unwrap()
                    .0
            } else {
                doc.add_record(section, [20, 20], "Neutral", "TEST")
                    .unwrap()
            };
            e.commit(&scene, "fixture");
            let original_record = e.records[&id].clone();
            let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            // Drag previews leave the document untouched until mouse-up.
            e.pointer(&scene, Some([20, 20]), true, true, false, Some(id));
            assert_eq!(e.tool, Tool::Move, "{section}");
            e.pointer(&scene, Some([23, 20]), false, true, false, None);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.pointer(&scene, Some([24, 20]), false, false, true, None);
            assert_eq!(e.tool, Tool::Select, "{section}");
            assert_eq!(e.records[&id], original_record.moved([24, 20]).unwrap());
            let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.history(true, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            // A fresh click selects the same record, and another click places it.
            e.pointer(&scene, Some([24, 20]), true, true, false, Some(id));
            e.pointer(&scene, Some([24, 20]), false, false, true, Some(id));
            e.pointer(&scene, Some([25, 20]), false, false, false, None);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            e.pointer(&scene, Some([25, 20]), true, false, true, None);
            assert_eq!(e.tool, Tool::Select, "{section}");
            assert_eq!(e.records[&id], original_record.moved([25, 20]).unwrap());
            drop(e);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn occupied_move_and_cancelled_drag_never_modify_the_source_or_history() {
        let (mut e, _, root) = editor();
        let scene = terrain_scene();
        e.attach(&scene);
        let doc = e.doc.as_mut().unwrap();
        let id = doc
            .add_record("Units", [20, 20], "Neutral", "HTNK")
            .unwrap();
        doc.add_record("Units", [24, 20], "Neutral", "HTNK")
            .unwrap();
        e.commit(&scene, "fixture");
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.pointer(&scene, Some([20, 20]), true, true, false, Some(id));
        e.pointer(&scene, Some([24, 20]), false, false, true, None);
        assert_eq!(e.tool, Tool::Move);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        assert_eq!(e.doc.as_ref().unwrap().undo_name(), Some("fixture"));
        e.cancel_operation(&scene);
        e.pointer(&scene, Some([20, 20]), true, true, false, Some(id));
        e.pointer(&scene, Some([23, 20]), false, true, false, None);
        assert!(e.cancel_active(&scene)); // Window focus loss cancels through this path.
        e.pointer(&scene, Some([23, 20]), false, false, true, None);
        assert_eq!(e.tool, Tool::Select);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        // Moving onto the original cell must not add an undo step.
        e.pointer(&scene, Some([20, 20]), true, false, true, Some(id));
        e.pointer(&scene, Some([20, 20]), true, false, true, None);
        assert_eq!(e.tool, Tool::Select);
        assert_eq!(e.doc.as_ref().unwrap().undo_name(), Some("fixture"));
        drop(e);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn waypoint_move_rejects_invalid_cells_and_preserves_other_selection_tools() {
        let (mut e, _, root) = editor();
        let scene = terrain_scene();
        e.attach(&scene);
        let doc = e.doc.as_mut().unwrap();
        doc.waypoint(42, [20, 20]).unwrap();
        let unit = doc
            .add_record("Units", [21, 20], "Neutral", "HTNK")
            .unwrap();
        e.commit(&scene, "fixture");
        let id = e
            .records
            .iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == "42")
            .map(|(&id, _)| id)
            .unwrap();
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.pointer(&scene, Some([21, 20]), true, false, true, Some(unit));
        assert_eq!(e.tool, Tool::Move);
        e.cancel_operation(&scene);
        e.pointer(&scene, Some([20, 20]), true, false, true, Some(id));
        assert_eq!(e.tool, Tool::Move);
        e.pointer(&scene, Some([0, 0]), true, false, true, None);
        assert_eq!(e.tool, Tool::Move);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.pointer(&scene, None, true, false, true, None);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        // Waypoints may share a cell with objects even when overlap is disabled.
        e.pointer(&scene, Some([21, 20]), true, false, true, Some(unit));
        assert_eq!(e.records[&id].cell(), Some([21, 20]));
        assert_eq!(e.records[&unit].cell(), Some([21, 20]));
        assert_eq!(e.tool, Tool::Select);
        // The waypoint placement tool keeps placing the palette's chosen number.
        e.activate_tool(Tool::Waypoint);
        e.waypoint = 60;
        e.pointer(&scene, Some([21, 20]), true, false, true, Some(id));
        assert_eq!(e.tool, Tool::Waypoint);
        assert!(
            e.records
                .values()
                .any(|r| r.section == "Waypoints" && r.key == "60")
        );
        drop(e);
        std::fs::remove_dir_all(root).unwrap();
    }
}
