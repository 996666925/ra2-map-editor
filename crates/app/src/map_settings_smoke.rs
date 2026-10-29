//! Shared geometry/lighting commands through the real loader and canvas attachment.
use super::*;
use ra2_map_document::{
    lighting::Profile,
    map_settings::{self, Options},
};

impl MapView {
    pub(super) fn map_settings_smoke_step(
        &mut self,
        _ctx: &egui::Context,
        state: &egui_wgpu::RenderState,
        output: &Path,
    ) -> Result<bool> {
        let scene = self.scene.as_ref().context("no scene")?;
        match self.p2.stage {
            90 => {
                let doc = self.editor.doc.as_mut().unwrap();
                self.p2.settings_before = doc.snapshot().bytes()?;
                let mut o = Options::from_document(doc);
                o.width += 4;
                o.height += 4;
                o.offset = [2, 2];
                o.local = [3, 5, i32::from(o.width) - 6, i32::from(o.height) - 12];
                let p =
                    map_settings::plan(doc, o, &crate::map_settings_ui::footprints(scene, doc))?;
                ensure!(
                    p.can_apply(),
                    "window expansion has losses: {:?}",
                    p.summary
                );
                p.apply(doc)?;
                self.p2.settings_after = doc.snapshot().bytes()?;
                self.editor.structural_changed(scene);
                self.p2.stage = 91;
            }
            91 => {
                let doc = self.editor.doc.as_ref().unwrap();
                ensure!(
                    scene.source.header.width == doc.base.header.width
                        && scene.playfield.off_fc == 3,
                    "geometry scene stale"
                );
                self.editor.history(false, scene);
                self.p2.stage = 92;
            }
            92 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.settings_before,
                    "structural undo bytes"
                );
                ensure!(
                    scene.source.bytes() == self.p2.settings_before,
                    "undo scene stale"
                );
                self.editor.history(true, scene);
                self.p2.stage = 93;
            }
            93 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()? == self.p2.settings_after,
                    "structural redo bytes"
                );
                ensure!(
                    scene.source.bytes() == self.p2.settings_after,
                    "redo scene stale"
                );
                let saved = output.join("map-settings.map");
                std::fs::write(&saved, &self.p2.settings_after)?;
                let bytes = std::fs::read(saved)?;
                let map = crate::editor_io::validate(&bytes)?;
                let reopened = crate::document::Baseline::parse(bytes.into(), &map)?;
                ensure!(
                    reopened.header.local_left == 3
                        && reopened.header.width == scene.source.header.width,
                    "settings reopen"
                );
                let cell = scene
                    .cells
                    .iter()
                    .filter(|c| scene.is_playable(c))
                    .min_by(|a, b| {
                        scene
                            .cell_rect(a)
                            .unwrap()
                            .center()
                            .distance_sq(scene.bounds.center())
                            .total_cmp(
                                &scene
                                    .cell_rect(b)
                                    .unwrap()
                                    .center()
                                    .distance_sq(scene.bounds.center()),
                            )
                    })
                    .context("no sample terrain")?;
                self.p2.at = [cell.rx, cell.ry];
                self.center = scene.cell_rect(cell).context("no tile")?.center();
                self.zoom = 1.;
                self.fit = false;
                self.p2.viewport_size = self.target.as_ref().map(|t| t.size);
                self.p2.stage = 94;
            }
            94 => {
                self.p2.settings_checks.push(check_viewport(
                    self,
                    state,
                    self.p2.at,
                    output,
                    "settings-normal",
                )?);
                let doc = self.editor.doc.as_ref().unwrap();
                self.workbench.map_settings.smoke_preview(doc, Profile::Ion);
                ensure!(
                    !self.workbench.map_settings.allow_leave(),
                    "lighting draft was not protected"
                );
                self.editor.refresh = true;
                self.p2.stage = 95;
            }
            95 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()? == self.p2.settings_after,
                    "preview modified live document"
                );
                ensure!(
                    scene.source.bytes() != self.p2.settings_after,
                    "lighting draft not loaded"
                );
                self.p2.settings_checks.push(check_viewport(
                    self,
                    state,
                    self.p2.at,
                    output,
                    "settings-ion",
                )?);
                ensure!(
                    std::fs::read(output.join("canvas-settings-normal-actual.png"))?
                        != std::fs::read(output.join("canvas-settings-ion-actual.png"))?,
                    "Ion canvas unchanged"
                );
                self.workbench.map_settings.profile = Profile::Dominator;
                self.editor.refresh = true;
                self.p2.stage = 96;
            }
            96 => {
                self.p2.settings_checks.push(check_viewport(
                    self,
                    state,
                    self.p2.at,
                    output,
                    "settings-dominator",
                )?);
                ensure!(
                    std::fs::read(output.join("canvas-settings-ion-actual.png"))?
                        != std::fs::read(output.join("canvas-settings-dominator-actual.png"))?,
                    "Dominator canvas unchanged"
                );
                self.workbench
                    .map_settings
                    .apply_lighting(self.editor.doc.as_mut().unwrap())?;
                self.editor.commit(scene, "smoke lighting");
                self.workbench.map_settings = Default::default();
                self.editor.refresh = true;
                self.p2.stage = 97;
            }
            97 => {
                ensure!(
                    self.editor
                        .doc
                        .as_ref()
                        .unwrap()
                        .ini_text("Lighting", "IonAmbient")
                        == "0.45",
                    "lighting apply"
                );
                self.editor.history(false, scene);
                self.p2.stage = 98;
            }
            98 => {
                ensure!(
                    scene.source.bytes() == self.p2.settings_after,
                    "lighting undo preview stale"
                );
                self.p2.settings_checks.push(check_viewport(
                    self,
                    state,
                    self.p2.at,
                    output,
                    "settings-restored",
                )?);
                ensure!(
                    std::fs::read(output.join("canvas-settings-normal-actual.png"))?
                        == std::fs::read(output.join("canvas-settings-restored-actual.png"))?,
                    "normal canvas not restored"
                );
                self.editor.history(false, scene);
                self.p2.stage = 99;
            }
            99 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.settings_before,
                    "settings smoke failed to restore map"
                );
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["map_settings"] = serde_json::json!({"passed":true,"shared_commands":true,"pointer_click_test":false,"local_size":true,"expand_undo_redo":true,"raw_save_reopen":true,"draft_guard":true,"lighting_preview_read_only":true,"lighting_apply_undo":true,"normal_canvas_restored":true,"canvas_profiles":self.p2.settings_checks});
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.p2.stage = 100;
            }
            100 => {
                let snapshot = self.editor.doc.as_ref().unwrap().snapshot();
                self.p2.p5_identity = (snapshot.document_id, snapshot.state_id);
                self.refresh_resources(self.directory.clone());
                ensure!(self.editor.refresh, "resource refresh was blocked");
                self.p2.stage = 101;
            }
            101 => {
                let doc = self.editor.doc.as_mut().unwrap();
                let snapshot = doc.snapshot();
                ensure!(
                    (snapshot.document_id, snapshot.state_id) == self.p2.p5_identity,
                    "refresh replaced document/history identity"
                );
                ensure!(
                    snapshot.bytes()? == self.p2.settings_before,
                    "refresh modified map"
                );
                ensure!(
                    doc.resource_profile.as_deref() == Some(scene.revision.id().as_str()),
                    "refreshed resource identity not attached"
                );
                ensure!(doc.redo(), "refresh discarded redo history");
                ensure!(
                    doc.snapshot().bytes()? == self.p2.settings_after,
                    "redo after refresh"
                );
                ensure!(
                    doc.undo() && doc.snapshot().bytes()? == self.p2.settings_before,
                    "undo after refresh"
                );
                let at = scene
                    .cells
                    .iter()
                    .find(|c| scene.is_playable(c))
                    .map(|c| [c.rx, c.ry])
                    .context("cell")?;
                let mut clip = ra2_map_document::region::Clipboard::capture(
                    doc,
                    &std::collections::BTreeSet::from([at]),
                    Default::default(),
                )?;
                ensure!(clip.plan(doc, at).is_ok(), "same Mod clipboard refused");
                clip.resource_profile = Some("different-mod-same-theater".into());
                ensure!(clip.plan(doc, at).is_err(), "cross Mod clipboard admitted");
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["p5_profiles"] = serde_json::json!({"passed":true,"resource_refresh":true,"document_and_history_preserved":true,"cross_mod_clipboard_rejected":true,"profile":scene.revision,"timings":self.timings});
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.p2.stage = 80;
            }
            _ => unreachable!(),
        }
        Ok(false)
    }
}
