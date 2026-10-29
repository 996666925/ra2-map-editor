//! Exercise the same draft/pick/apply/history path as the mission window.
use super::*;
use anyhow::{Context, Result, ensure};
impl MissionUi {
    pub(crate) fn authoring_smoke(
        &mut self,
        editor: &mut Editor,
        scene: &MapScene,
    ) -> Result<serde_json::Value> {
        let doc = editor.doc.as_ref().context("document")?;
        let before = doc.snapshot().bytes()?;
        let trigger = self.selected.clone();
        let team = mission::ids(doc, Kind::Team)[0].clone();
        let records = doc.records();
        let (&wp_id, wp) = records
            .iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == "1")
            .context("waypoint 1")?;
        let (&object_id, object) = records
            .iter()
            .find(|(_, r)| r.section == "Infantry")
            .context("infantry")?;
        let mut row = mission::schema::row(false, 80).unwrap();
        row[2] = team;
        self.draft
            .insert("Actions".into(), mission::encode_list(&[row]));
        self.pick = Some(forms::Pick {
            key: "Actions".into(),
            row: Some(0),
            index: 7,
            kind: mission::schema::ParameterKind::Waypoint { letters: true },
        });
        self.pick_canvas(editor, wp.cell().context("waypoint cell")?, Some(wp_id));
        ensure!(
            !self.picking() && mission::decode_list(&self.draft["Actions"], false)?[0][7] == "B",
            "letter waypoint pick"
        );
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "pick modified document"
        );
        let bytes = preserve_csv(
            self.original.get("Actions").map(Vec::as_slice),
            &self.draft["Actions"],
        );
        mission::write_fields(
            editor.doc.as_mut().unwrap(),
            Kind::Trigger,
            &trigger,
            &[("Actions".into(), bytes)],
        )?;
        editor.commit(scene, "烟测：拾取动作航点");
        let after = editor.doc.as_ref().unwrap().snapshot().bytes()?;
        editor.history(false, scene);
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "apply undo"
        );
        editor.history(true, scene);
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == after,
            "apply redo"
        );
        editor.history(false, scene);
        self.load(editor);
        self.draft.insert(
            "Events".into(),
            mission::encode_list(&[mission::schema::row(true, 60).unwrap()]),
        );
        self.pick = Some(forms::Pick {
            key: "Events".into(),
            row: Some(0),
            index: 3,
            kind: mission::schema::ParameterKind::Type(""),
        });
        self.pick_canvas(
            editor,
            object.cell().context("object cell")?,
            Some(object_id),
        );
        ensure!(
            !self.picking()
                && mission::decode_list(&self.draft["Events"], true)?[0][3] == object.type_id(),
            "object type pick"
        );
        self.load(editor);
        let script = mission::ids(editor.doc.as_ref().unwrap(), Kind::Script)[0].clone();
        self.kind = Kind::Script;
        self.selected = script;
        self.load(editor);
        self.pick = Some(forms::Pick {
            key: "0".into(),
            row: None,
            index: 1,
            kind: mission::schema::ParameterKind::Waypoint { letters: false },
        });
        self.pick_canvas(editor, wp.cell().unwrap(), Some(wp_id));
        ensure!(self.draft["0"] == "3,1", "integer script waypoint pick");
        self.load(editor);
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "smoke restoration"
        );
        ensure!(
            scene
                .mission_resources
                .choices
                .values()
                .all(|rows| !rows.is_empty()),
            "resource catalogs"
        );
        self.kind = Kind::Trigger;
        self.selected = trigger;
        self.load(editor);
        Ok(
            serde_json::json!({"letter_waypoint_pick":true,"integer_script_pick":true,"object_type_pick":true,"draft_only":true,"apply_undo_redo":true,"resource_choices":scene.mission_resources.choices.iter().map(|(key,rows)|(key,rows.len())).collect::<BTreeMap<_,_>>()}),
        )
    }
}
