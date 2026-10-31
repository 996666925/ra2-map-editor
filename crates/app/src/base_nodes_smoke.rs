//! Real eframe pointer input -> MapView -> BaseNodesUi -> one history command.
use super::*;
use ra2_map_document::{
    base_nodes,
    mission::{self, Kind},
};
impl MapView {
    pub(super) fn base_nodes_smoke_step(
        &mut self,
        ctx: &egui::Context,
        output: &Path,
    ) -> Result<bool> {
        let scene = self.scene.as_ref().context("scene")?;
        let rect = self.workbench.canvas_rect.context("canvas")?;
        let pos = rect.center() + egui::vec2(rect.width() * 0.24, 0.);
        match self.p2.stage {
            74 => {
                let doc = self.editor.doc.as_ref().unwrap();
                self.p2.base_house = mission::ids(doc, Kind::House)
                    .last()
                    .context("House")?
                    .clone();
                self.p2.base_index = base_nodes::read(doc, &self.p2.base_house)?.len();
                self.p2.interaction_before = doc.snapshot().bytes()?;
                let cell = scene
                    .cells
                    .iter()
                    .filter(|c| {
                        doc.terrain([c.rx, c.ry]).is_some()
                            && doc.terrain([c.rx + 2, c.ry]).is_some()
                    })
                    .min_by(|a, b| {
                        egui::pos2(a.screen_x, a.screen_y)
                            .distance_sq(scene.bounds.center())
                            .total_cmp(
                                &egui::pos2(b.screen_x, b.screen_y)
                                    .distance_sq(scene.bounds.center()),
                            )
                    })
                    .context("base node sample")?;
                self.p2.at = [cell.rx, cell.ry];
                self.center = egui::pos2(cell.screen_x + 30., cell.screen_y + 15.)
                    - egui::vec2(rect.width() * 0.24, 0.);
                self.zoom = 1.;
                self.fit = false;
                self.workbench.mission.open = true;
                self.workbench
                    .mission
                    .base
                    .start(self.p2.base_house.clone());
                self.p2.input_events = vec![
                    egui::Event::WindowFocused(true),
                    egui::Event::PointerMoved(pos),
                ];
                self.p2.stage = 75;
            }
            75 => {
                self.p2.input_events = pointer_events(pos, &[true, false]);
                self.p2.stage = 76;
            }
            76 => {
                let doc = self.editor.doc.as_ref().unwrap();
                let nodes = base_nodes::read(doc, &self.p2.base_house)?;
                ensure!(
                    nodes.len() == self.p2.base_index + 1
                        && nodes[self.p2.base_index].cell == self.p2.at,
                    "base pointer click did not create node at requested cell"
                );
                self.p2.interaction_after_click = doc.snapshot().bytes()?;
                self.p2.input_events = pointer_events(pos, &[true]);
                self.p2.stage = 77;
            }
            77 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_click,
                    "node press wrote document before release"
                );
                self.p2.input_events = vec![egui::Event::PointerMoved(pos + egui::vec2(60., 30.))];
                self.p2.stage = 78;
            }
            78 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_click,
                    "node drag wrote document before release"
                );
                self.p2.input_events = pointer_events(pos + egui::vec2(60., 30.), &[false]);
                self.p2.stage = 79;
            }
            79 => {
                let doc = self.editor.doc.as_ref().unwrap();
                let nodes = base_nodes::read(doc, &self.p2.base_house)?;
                ensure!(
                    nodes[self.p2.base_index].cell == [self.p2.at[0] + 2, self.p2.at[1]],
                    "node drag did not move node"
                );
                let after = doc.snapshot().bytes()?;
                self.p2.interaction_after_drag = after.clone();
                std::fs::write(output.join("base-nodes.map"), &after)?;
                let map = vera20k_core::map::map_file::load_from_bytes(after.clone())?;
                let reopened = crate::document::MapDocument::new(crate::document::Baseline::parse(
                    after.clone().into(),
                    &map,
                )?);
                ensure!(
                    base_nodes::read(&reopened, &self.p2.base_house)? == nodes,
                    "base nodes did not roundtrip"
                );
                self.editor.history(false, scene);
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_click,
                    "node move undo not atomic"
                );
                self.editor.history(true, scene);
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()? == after,
                    "node move redo mismatch"
                );
                // Undo/redo requested a background scene refresh. Ask for a
                // capture after that refresh, so loading cannot consume it.
                self.p2.stage = 82;
            }
            82 => {
                crate::screen_capture::request(ctx);
                self.p2.stage = 81;
            }
            81 => {
                if !save_capture(ctx, &output.join("base-nodes-panel.png"))? {
                    return Ok(false);
                }
                self.p2.input_events = pointer_events(pos + egui::vec2(60., 30.), &[true]);
                self.p2.stage = 83;
            }
            83 => {
                self.p2.input_events = pointer_events(pos + egui::vec2(120., 60.), &[false]);
                self.p2.input_events.extend([
                    key_event(egui::Key::Escape, true),
                    key_event(egui::Key::Escape, false),
                ]);
                self.p2.stage = 84;
            }
            84 => {
                ensure!(
                    self.workbench.mission.base.active,
                    "Escape exited instead of cancelling node drag"
                );
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_drag,
                    "Escape with release committed node drag"
                );
                self.p2.input_events = pointer_events(pos + egui::vec2(60., 30.), &[true]);
                self.p2.stage = 85;
            }
            85 => {
                self.p2.input_events = vec![egui::Event::WindowFocused(false)];
                self.p2
                    .input_events
                    .extend(pointer_events(pos + egui::vec2(120., 60.), &[false]));
                self.p2.stage = 86;
            }
            86 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_drag,
                    "focus loss committed node drag"
                );
                self.p2.input_events = vec![egui::Event::WindowFocused(true)];
                self.p2.stage = 87;
            }
            87 => {
                self.editor.history(false, scene);
                self.editor.history(false, scene);
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_before,
                    "base gesture smoke did not restore document"
                );
                self.workbench.mission.base.stop();
                self.workbench.mission.open = false;
                // The UI gallery now also captures a real local AI form.
                mission::create(
                    self.editor.doc.as_mut().unwrap(),
                    Kind::AiTrigger,
                    "WINDOW_AI",
                    &self.p2.base_house,
                    None,
                )?;
                self.editor.commit(scene, "烟测：AI 触发器表单样例");
                std::fs::write(
                    output.join("ai-authoring.map"),
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?,
                )?;
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["base_nodes"] = serde_json::json!({"passed":true,"native_pointer_click":true,"native_pointer_drag":true,"draft_until_release":true,"escape_and_release_cancels":true,"focus_loss_cancels":true,"undo_redo_atomic":true,"save_reopen":true,"map":"base-nodes.map","panel":"base-nodes-panel.png"});
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.p2.stage = 90;
            }
            _ => unreachable!(),
        }
        Ok(false)
    }
}
