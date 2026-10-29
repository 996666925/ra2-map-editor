//! State-keyed authoring labels. Kept out of published previews and map bytes.
use crate::{
    camera::MapCamera,
    document::{Cell, MapDocument},
};
use eframe::egui::{self, Color32};
use ra2_map_document::mission::{self, Kind};

#[derive(Default)]
pub(crate) struct MissionOverlay {
    key: Option<(u64, u64)>,
    marks: Vec<Mark>,
}
struct Mark {
    at: Cell,
    kind: Kind,
    id: String,
    label: String,
}
impl MissionOverlay {
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        doc: &MapDocument,
        rect: egui::Rect,
        camera: &MapCamera,
        pointer: Option<egui::Pos2>,
        objects: bool,
    ) -> Option<(Kind, String)> {
        if self.key != Some((doc.id, doc.state_id)) {
            self.marks.clear();
            for (_, id, value) in doc.ini_entries("CellTags") {
                if let Some(at) = mission::cell_at(doc, &id) {
                    self.marks.push(Mark {
                        at,
                        kind: Kind::CellTag,
                        id,
                        label: format!("格子 Tag {}", String::from_utf8_lossy(&value)),
                    });
                }
            }
            for record in doc.records().values() {
                let col = match record.section.as_str() {
                    "Structures" => 6,
                    "Infantry" => 8,
                    "Units" | "Aircraft" => 7,
                    _ => continue,
                };
                let fields = record.fields();
                if let Some(tag) = fields.get(col).filter(|tag| {
                    !tag.is_empty() && !matches!(tag.as_str(), "None" | "none" | "<none>" | "-1")
                }) && let Some(at) = record.cell()
                {
                    self.marks.push(Mark {
                        at,
                        kind: Kind::Tag,
                        id: tag.clone(),
                        label: format!("{} {} · Tag {tag}", record.section, record.key),
                    });
                }
            }
            self.key = Some((doc.id, doc.state_id));
        }
        let world = camera.world_rect(rect);
        let mut hit = None;
        let mut stacked = std::collections::BTreeMap::<Cell, usize>::new();
        for mark in &self.marks {
            if mark.kind == Kind::Tag && !objects {
                continue;
            }
            let level = doc.terrain(mark.at).map_or(0, |t| t.level);
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(mark.at[0], mark.at[1], level);
            let at = rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom;
            if !rect.expand(30.).contains(at) {
                continue;
            }
            let slot = stacked.entry(mark.at).or_default();
            let position = at + egui::vec2(10., -20. - 18. * (*slot as f32));
            *slot += 1;
            let text = painter.layout_no_wrap(
                mark.label.clone(),
                egui::FontId::proportional(11.),
                Color32::from_rgb(216, 145, 255),
            );
            let label_rect = egui::Rect::from_min_size(position, text.size()).expand(3.);
            let hovered = pointer.is_some_and(|p| {
                (camera.zoom >= 0.35 && label_rect.contains(p)) || p.distance(at) < 8.
            });
            let color = if hovered {
                Color32::WHITE
            } else {
                Color32::from_rgb(216, 145, 255)
            };
            painter.circle_stroke(at, 6., egui::Stroke::new(1.5, color));
            if camera.zoom >= 0.35 || hovered {
                painter.rect_filled(label_rect, 2., Color32::from_black_alpha(210));
                painter.galley(position, text, color);
            }
            if hovered {
                hit = Some((mark.kind, mark.id.clone()));
            }
        }
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra2_map_document::{
        document::{Baseline, Target, Value},
        new_map::NewMapOptions,
    };

    #[test]
    fn marker_hits_follow_pan_zoom_layer_visibility_and_history() {
        let bytes = NewMapOptions::default().bytes().unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
        doc.set_ini("CellTags", "80080", Some(b"CELL_TAG".to_vec()))
            .unwrap();
        let object = doc
            .add_record("Units", [82, 80], "Neutral", "HTNK")
            .unwrap();
        let record = doc.records()[&object]
            .patched(&[(7, "OBJECT_TAG".into())])
            .unwrap();
        doc.set(Target::Record(object), Some(Value::Record(record)))
            .unwrap();
        doc.commit("labels");
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(123., 75.), egui::vec2(800., 600.));
        let mut overlay = MissionOverlay::default();
        let mut draw = |doc: &MapDocument, at: Cell, zoom, objects| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], 0);
            let camera = MapCamera {
                center: egui::pos2(x + 30., y + 15.),
                zoom,
            };
            let mut hit = None;
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                hit = overlay.draw(
                    ui.painter(),
                    doc,
                    rect,
                    &camera,
                    Some(rect.center()),
                    objects,
                );
            });
            hit
        };
        for zoom in [0.2, 0.75, 2.] {
            assert_eq!(
                draw(&doc, [80, 80], zoom, true),
                Some((Kind::CellTag, "80080".into()))
            );
            assert_eq!(
                draw(&doc, [82, 80], zoom, true),
                Some((Kind::Tag, "OBJECT_TAG".into()))
            );
            assert_eq!(draw(&doc, [82, 80], zoom, false), None);
        }
        doc.set_ini("CellTags", "80080", None).unwrap();
        doc.commit("remove label");
        assert_eq!(draw(&doc, [80, 80], 1., true), None);
        doc.undo();
        assert_eq!(
            draw(&doc, [80, 80], 1., true),
            Some((Kind::CellTag, "80080".into()))
        );
    }
}
