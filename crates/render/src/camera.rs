/// World pixels -> logical UI points. DPI belongs only to target allocation
/// and the 1:1 button, so the inverse stays identical on every monitor.
pub struct MapCamera {
    pub center: egui::Pos2,
    pub zoom: f32,
}
impl MapCamera {
    pub fn world_rect(&self, viewport: egui::Rect) -> egui::Rect {
        egui::Rect::from_center_size(self.center, viewport.size() / self.zoom)
    }
    pub fn world_point(&self, viewport: egui::Rect, point: egui::Pos2) -> egui::Pos2 {
        self.center + (point - viewport.center()) / self.zoom
    }
    pub fn zoom_at(&mut self, viewport: egui::Rect, point: egui::Pos2, next: f32) {
        let anchor = self.world_point(viewport, point);
        self.zoom = next;
        self.center = anchor - (point - viewport.center()) / next;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchored_zoom_and_inverse_survive_pan_fractional_zoom_and_dpi() {
        let rect = egui::Rect::from_min_size(egui::pos2(291., 70.), egui::vec2(970., 670.));
        let pointer = egui::pos2(729.25, 291.75);
        let mut camera = MapCamera {
            center: egui::pos2(-178.5, 2013.25),
            zoom: 0.33,
        };
        let anchor = camera.world_point(rect, pointer);
        for dpi in [1., 1.25, 1.75, 2.] {
            // The real 1:1 scale is one world pixel per physical display pixel.
            camera.zoom_at(rect, pointer, 1. / dpi);
            assert!((camera.world_point(rect, pointer) - anchor).length() < 0.001);
            assert!(
                ((camera.world_point(rect, pointer + egui::vec2(1. / dpi, 0.)) - anchor).x - 1.)
                    .abs()
                    < 0.001
            );
            let world = camera.world_rect(rect);
            let screen = rect.min + (anchor - world.min) * camera.zoom;
            assert!((screen - pointer).length() < 0.001);
        }
    }
}
