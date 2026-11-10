//! Size follows the projected map perimeter; LocalSize is a logical plane guide.
use crate::{camera::MapCamera, map::MapScene};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use vera20k_core::map::{
    map_file::MapHeader,
    playfield::size_diamond_contains,
    terrain::{TILE_HEIGHT, TILE_WIDTH, TerrainCell, iso_to_screen},
};

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

/// Bound the Size perimeter's diamond frames, including the final half row.
/// Interior hills and sprite overhangs do not move the map's outer guide.
fn size_rect(header: &MapHeader, cells: &[TerrainCell], flat: bool) -> Option<Rect> {
    let width = header.width as i32;
    let height = header.height as i32;
    let mut bounds = Rect::NOTHING;
    for cell in cells {
        let (x, y) = (i32::from(cell.rx), i32::from(cell.ry));
        if !size_diamond_contains(width, height, (cell.rx as i16, cell.ry as i16))
            || !(x + y == width + 1 || x + y == width + 2 * height || (x - y).abs() == width - 1)
        {
            continue;
        }
        let (sx, sy) = if flat {
            iso_to_screen(cell.rx, cell.ry, 0)
        } else {
            (cell.screen_x, cell.screen_y)
        };
        bounds = bounds.union(Rect::from_min_size(
            egui::pos2(sx, sy),
            egui::vec2(TILE_WIDTH, TILE_HEIGHT),
        ));
    }
    bounds.is_positive().then_some(bounds)
}

/// Derived from the scene's terrain geometry. Cache both view modes so drawing
/// and toggling guides never scan a large map each frame. Scene replacement
/// changes the cell allocation; opening/restoring also explicitly invalidates it.
#[derive(Default)]
pub(crate) struct SizeGuide {
    key: Option<(usize, usize, u32, u32)>,
    normal: Option<Rect>,
    flat: Option<Rect>,
}

impl SizeGuide {
    pub(crate) fn invalidate(&mut self) {
        self.key = None;
    }

    pub(crate) fn draw(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        flat: bool,
        viewport: Rect,
        camera: &MapCamera,
    ) {
        let header = &scene.source.header;
        let key = (
            scene.cells.as_ptr() as usize,
            scene.cells.len(),
            header.width,
            header.height,
        );
        if self.key != Some(key) {
            self.normal = size_rect(header, &scene.cells, false);
            self.flat = size_rect(header, &scene.cells, true);
            self.key = Some(key);
        }
        let size = if flat { self.flat } else { self.normal };
        let painter = painter.with_clip_rect(viewport);
        let world = camera.world_rect(viewport);
        for (bounds, color) in [
            (size, Color32::from_rgb(255, 64, 64)),
            (
                projected_rect(
                    header.width,
                    [
                        header.local_left as i32,
                        header.local_top as i32,
                        header.local_width as i32,
                        header.local_height as i32,
                    ],
                ),
                Color32::from_rgb(64, 112, 255),
            ),
        ] {
            let Some(bounds) = bounds else {
                continue;
            };
            let screen = |p: Pos2| viewport.min + (p - world.min) * camera.zoom;
            let bounds = Rect::from_min_max(screen(bounds.min), screen(bounds.max));
            painter.rect_stroke(bounds, 0, Stroke::new(1., color), egui::StrokeKind::Middle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain(width: u16, height: u16, level: u8) -> (MapHeader, Vec<TerrainCell>) {
        let bytes = crate::new_map::NewMapOptions {
            width,
            height,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes).unwrap();
        let cells = map
            .cells
            .iter()
            .map(|cell| {
                let (screen_x, screen_y) = iso_to_screen(cell.rx, cell.ry, level);
                TerrainCell {
                    screen_x,
                    screen_y,
                    tile_id: 0,
                    sub_tile: 0,
                    z: level,
                    rx: cell.rx,
                    ry: cell.ry,
                    variant: 0,
                    tint: [1.; 3],
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                }
            })
            .collect();
        (map.header, cells)
    }

    #[test]
    fn size_guide_encloses_elevated_perimeter_and_final_half_row() {
        let flat = Rect::from_min_max(egui::pos2(-2400., 1230.), egui::pos2(2400., 2745.));
        for level in [0, 4, 127, 255] {
            let (header, cells) = terrain(80, 50, level);
            // Random maps keep header Level=0 while their border cells are 4.
            assert_eq!(header.level, 0);
            let raised = flat.translate(egui::vec2(0., -15. * f32::from(level as i8)));
            assert_eq!(size_rect(&header, &cells, false), Some(raised));
            assert_eq!(size_rect(&header, &cells, true), Some(flat));
            // Test actual projected tile extents, not only the guide formula.
            for cell in &cells {
                assert!(raised.contains_rect(Rect::from_min_size(
                    egui::pos2(cell.screen_x, cell.screen_y),
                    egui::vec2(60., 30.)
                )));
            }
        }
    }

    #[test]
    fn interior_peaks_leave_size_guide_fixed_but_perimeter_edits_move_it() {
        let (header, mut cells) = terrain(80, 50, 4);
        let before = size_rect(&header, &cells, false).unwrap();
        let peak = cells.iter_mut().find(|c| (c.rx, c.ry) == (65, 65)).unwrap();
        peak.z = 127;
        peak.screen_y = iso_to_screen(peak.rx, peak.ry, peak.z).1;
        assert!(peak.screen_y < before.top());
        assert_eq!(size_rect(&header, &cells, false), Some(before));
        // Raising the complete top edge moves only the top of the red frame.
        for cell in &mut cells {
            if u32::from(cell.rx) + u32::from(cell.ry) == header.width + 1 {
                cell.z = 5;
                cell.screen_y = iso_to_screen(cell.rx, cell.ry, cell.z).1;
            }
        }
        let after = size_rect(&header, &cells, false).unwrap();
        assert_eq!(after.top(), before.top() - 15.);
        assert_eq!(after.bottom(), before.bottom());
        assert_eq!(after.x_range(), before.x_range());
        assert_eq!(size_rect(&header, &[], false), None);
    }

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
