//! FA2-style Size and LocalSize guides on the map's projection plane.
use crate::camera::MapCamera;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use vera20k_core::map::map_file::MapHeader;

/// LocalSize is a rectangle in projected map coordinates, not cell X/Y.
/// One map unit spans 60 × 30 pixels. The origin includes the same row bias
/// as terrain::iso_to_screen plus its half-tile centre offset. Do not derive
/// this frame from sprite bounds: cliffs and objects can extend beyond Size.
fn projected_rect(map_width: u32, [left, top, width, height]: [i32; 4]) -> Option<Rect> {
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(Rect::from_min_size(
        egui::pos2(
            (2. * left as f32 - map_width as f32) * 30.,
            (2. * top as f32 + map_width as f32) * 15. + 30.,
        ),
        egui::vec2(width as f32 * 60., height as f32 * 30.),
    ))
}

pub(crate) fn draw(
    painter: &egui::Painter,
    header: &MapHeader,
    viewport: Rect,
    camera: &MapCamera,
) {
    let painter = painter.with_clip_rect(viewport);
    let world = camera.world_rect(viewport);
    for (area, color) in [
        (
            [0, 0, header.width as i32, header.height as i32],
            Color32::from_rgb(255, 64, 64),
        ),
        (
            [
                header.local_left as i32,
                header.local_top as i32,
                header.local_width as i32,
                header.local_height as i32,
            ],
            Color32::from_rgb(64, 112, 255),
        ),
    ] {
        let Some(bounds) = projected_rect(header.width, area) else {
            continue;
        };
        let screen = |p: Pos2| viewport.min + (p - world.min) * camera.zoom;
        let bounds = Rect::from_min_max(screen(bounds.min), screen(bounds.max));
        painter.rect_stroke(bounds, 0, Stroke::new(1., color), egui::StrokeKind::Middle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangular_map_and_offset_local_size_share_the_terrain_projection() {
        let size = projected_rect(80, [0, 0, 80, 50]).unwrap();
        assert_eq!(size.min, egui::pos2(-2400., 1230.));
        assert_eq!(size.size(), egui::vec2(4800., 1500.));
        let local = projected_rect(80, [3, 5, 70, 36]).unwrap();
        assert_eq!(local.min - size.min, egui::vec2(180., 150.));
        assert_eq!(local.size(), egui::vec2(4200., 1080.));
        // Local (u,v) -> cell ((u+v), (Size.width-u+v)) at even rows.
        // The guide corner lies at that cell's level-zero centre.
        assert_eq!(local.min, crate::flat_view::position([8, 82]));
        assert!(size.contains_rect(local));
        assert!(projected_rect(80, [0, 0, 0, 50]).is_none());
        assert!(projected_rect(80, [0, 0, 70, -1]).is_none());
    }
}
