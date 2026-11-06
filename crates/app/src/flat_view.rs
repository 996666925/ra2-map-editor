//! Elevation-free terrain workspace. Drawing and picking share the same grid.
use crate::{
    camera::MapCamera,
    document::{Cell, TerrainValue},
    map::MapScene,
};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use ra2_map_scene::{radar, terrain_tools};
use std::collections::HashMap;

pub(crate) struct FlatView {
    scene_key: (usize, usize),
    cells: HashMap<Cell, usize>,
    pub bounds: Rect,
    pub heights: bool,
    pub templates: bool,
    textures: crate::framework_view::Textures,
}

impl Default for FlatView {
    fn default() -> Self {
        Self {
            scene_key: (0, 0),
            cells: HashMap::new(),
            bounds: Rect::NOTHING,
            heights: true,
            templates: true,
            textures: Default::default(),
        }
    }
}

pub(crate) fn position(at: Cell) -> Pos2 {
    radar::cell_position(at)
}

pub(crate) fn set_enabled(
    editor: &mut crate::editing::Editor,
    scene: &MapScene,
    center: &mut Pos2,
    enabled: bool,
) {
    if editor.flat_view == enabled || editor.stroking {
        return;
    }
    *center = if enabled {
        radar::radar_position(scene, *center)
    } else {
        radar::canvas_position(scene, editor.doc.as_ref(), *center)
    };
    editor.flat_view = enabled;
    if enabled {
        editor.set_framework(false);
    }
}

pub(crate) fn points(at: Cell, rect: Rect, camera: &MapCamera) -> [Pos2; 4] {
    let center = rect.min + (position(at) - camera.world_rect(rect).min) * camera.zoom;
    [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
        .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom)
}

fn coordinates(p: Pos2) -> (i32, i32) {
    let (x, y) = vera20k_core::map::terrain::screen_to_iso(p.x, p.y);
    (x.round() as i32, y.round() as i32)
}

fn template_id(scene: &MapScene, at: Cell, v: TerrainValue) -> Option<(u16, i32, i32, i16)> {
    let &tile = scene.palette.raw_ids.get(&v.tile)?;
    let t = scene.palette.templates.get(&(tile, 0))?;
    if t.width == 0 || t.width == 1 && t.height == 1 {
        return None;
    }
    let metadata = t.cells.get(usize::from(v.sub))?.as_ref()?;
    Some((
        tile,
        i32::from(at[0]) - i32::from(v.sub) % i32::from(t.width),
        i32::from(at[1]) - i32::from(v.sub) / i32::from(t.width),
        i16::from(v.level) - i16::from(metadata.height),
    ))
}

/// Fixed height colours make low and high sides distinguishable across strokes.
pub(crate) fn height_color(level: u8) -> Color32 {
    let t = (f32::from((level as i8).max(0)) / 127.).sqrt();
    Color32::from_rgb(
        (40. + 145. * t) as u8,
        (73. + 105. * t) as u8,
        (92. - 25. * t) as u8,
    )
}

impl FlatView {
    pub fn invalidate(&mut self) {
        self.scene_key = (0, 0);
    }
    pub fn prepare(&mut self, scene: &MapScene) {
        let key = (scene.cells.as_ptr() as usize, scene.cells.len());
        if self.scene_key == key {
            return;
        }
        self.scene_key = key;
        self.cells.clear();
        self.bounds = Rect::NOTHING;
        for (i, c) in scene.cells.iter().enumerate() {
            let at = [c.rx, c.ry];
            self.cells.insert(at, i);
            self.bounds = self
                .bounds
                .union(Rect::from_center_size(position(at), egui::vec2(60., 30.)));
        }
    }

    pub fn pick(&self, p: Pos2) -> Option<usize> {
        let (x, y) = coordinates(p);
        let at = [u16::try_from(x).ok()?, u16::try_from(y).ok()?];
        self.cells.get(&at).copied()
    }

    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        editor: &crate::editing::Editor,
        rect: Rect,
        camera: &MapCamera,
    ) {
        let Some(doc) = editor.doc.as_ref() else {
            return;
        };
        let world = camera.world_rect(rect).expand(60.);
        let mut visible = Vec::new();
        let corners = [
            world.left_top(),
            world.right_top(),
            world.right_bottom(),
            world.left_bottom(),
        ]
        .map(coordinates);
        let min_x = corners.iter().map(|p| p.0).min().unwrap().max(0);
        let max_x = corners
            .iter()
            .map(|p| p.0)
            .max()
            .unwrap()
            .min(i32::from(vera20k_core::map::cell_index::MAX_CELL_SIDE) - 1);
        let min_y = corners.iter().map(|p| p.1).min().unwrap().max(0);
        let max_y = corners
            .iter()
            .map(|p| p.1)
            .max()
            .unwrap()
            .min(i32::from(vera20k_core::map::cell_index::MAX_CELL_SIDE) - 1);
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                let at = [x as u16, y as u16];
                let Some(&index) = self.cells.get(&at) else {
                    continue;
                };
                let c = &scene.cells[index];
                let v = terrain_tools::value(doc, scene, at);
                let p = points(at, rect, camera);
                let bounds = Rect::from_min_max(
                    p[3] - egui::vec2(0., 15. * camera.zoom),
                    p[1] + egui::vec2(0., 15. * camera.zoom),
                );
                if !bounds.intersects(rect) {
                    continue;
                }
                if editor.framework {
                    visible.push((at, v));
                }
                let mut color = height_color(v.level);
                let tile = scene.palette.raw_ids.get(&v.tile).copied();
                let category = tile
                    .and_then(|tile| scene.palette.groups.get(&tile))
                    .map(|g| g.category);
                use ra2_map_assets::terrain_library::TerrainCategory;
                color = match category {
                    Some(TerrainCategory::Cliff) => {
                        Color32::from_rgb(color.r().saturating_add(45), color.g(), color.b() / 2)
                    }
                    Some(TerrainCategory::Water) => Color32::from_rgb(35, 80, 135),
                    Some(TerrainCategory::Shore) => Color32::from_rgb(96, 118, 136),
                    Some(TerrainCategory::Road | TerrainCategory::Bridge) => {
                        Color32::from_rgb(100, 103, 109)
                    }
                    _ => color,
                };
                if !scene.is_playable(c) {
                    color = color.gamma_multiply(0.5);
                }
                painter.add(egui::Shape::convex_polygon(
                    p.to_vec(),
                    color,
                    if camera.zoom >= 0.3 {
                        Stroke::new(0.5, Color32::from_black_alpha(90))
                    } else {
                        Stroke::NONE
                    },
                ));
                let own = template_id(scene, at, v);
                for (edge, [dx, dy]) in [[0, -1], [1, 0], [0, 1], [-1, 0]].into_iter().enumerate() {
                    let neighbor = [(x + dx) as u16, (y + dy) as u16];
                    let n = self
                        .cells
                        .get(&neighbor)
                        .map(|_| terrain_tools::value(doc, scene, neighbor));
                    let height_edge = n.is_some_and(|n| n.level != v.level);
                    let template_edge = self.templates
                        && own.is_some()
                        && n.is_none_or(|n| template_id(scene, neighbor, n) != own);
                    if height_edge || template_edge {
                        painter.line_segment(
                            [p[edge], p[(edge + 1) % 4]],
                            Stroke::new(
                                if height_edge { 1.5 } else { 1. },
                                if height_edge {
                                    Color32::from_rgb(250, 201, 100)
                                } else {
                                    Color32::from_rgb(150, 200, 225)
                                },
                            ),
                        );
                    }
                }
                if self.heights && camera.zoom >= 0.65 {
                    let slope = tile
                        .and_then(|tile| scene.palette.templates.get(&(tile, 0)))
                        .and_then(|t| t.cells.get(usize::from(v.sub)))
                        .and_then(|s| *s)
                        .is_some_and(|s| s.slope != 0);
                    painter.text(
                        bounds.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("{}{}", v.level as i8, if slope { "/" } else { "" }),
                        egui::FontId::monospace((11. * camera.zoom).clamp(10., 18.)),
                        Color32::WHITE,
                    );
                }
                if camera.zoom >= 1.5 && self.templates && own.is_some() {
                    painter.text(
                        bounds.center() + egui::vec2(0., 10. * camera.zoom),
                        egui::Align2::CENTER_CENTER,
                        format!("{}:{}", tile.unwrap(), v.sub),
                        egui::FontId::monospace(10.),
                        Color32::LIGHT_BLUE,
                    );
                }
                if doc.overlay(at).0 != 255 {
                    painter.circle_filled(
                        bounds.center() + egui::vec2(10., 0.) * camera.zoom,
                        (2. * camera.zoom).clamp(1., 4.),
                        Color32::LIGHT_RED,
                    );
                }
            }
        }
        if editor.framework {
            visible.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
            for (at, value) in visible {
                self.textures.draw(
                    painter,
                    scene,
                    crate::framework_view::View {
                        rect,
                        camera,
                        flat: true,
                    },
                    at,
                    value,
                    Color32::WHITE,
                );
                if self.heights && camera.zoom >= 0.65 {
                    let center =
                        rect.min + (position(at) - camera.world_rect(rect).min) * camera.zoom;
                    painter.text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        (value.level as i8).to_string(),
                        egui::FontId::monospace(11.),
                        Color32::BLACK,
                    );
                }
            }
        }
        // Keep occupied foundations visible while working on the terrain beneath them.
        for record in editor.records.values().filter(|r| r.section != "Waypoints") {
            if let Ok(cells) = ra2_map_scene::placement::footprint(scene, record) {
                for at in cells {
                    let p = points(at, rect, camera);
                    if !Rect::from_two_pos(p[3], p[1])
                        .expand(30. * camera.zoom)
                        .intersects(rect)
                    {
                        continue;
                    }
                    painter.line_segment([p[0], p[2]], Stroke::new(1., Color32::LIGHT_RED));
                    painter.line_segment([p[1], p[3]], Stroke::new(1., Color32::LIGHT_RED));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_picking_matches_diamond_interiors_at_every_height_zoom_and_pan() {
        let mut scene = crate::editing::tests::terrain_scene();
        let rect = Rect::from_min_size(egui::pos2(271., 93.), egui::vec2(800., 600.));
        for level in [0, 4, 28, 127] {
            for c in &mut scene.cells {
                c.z = level;
            }
            let mut flat = FlatView::default();
            flat.prepare(&scene);
            for zoom in [0.25, 0.65, 1., 2.5] {
                let camera = MapCamera {
                    center: egui::pos2(-47.25, 792.5),
                    zoom,
                };
                for at in [[18, 20], [20, 20], [22, 20]] {
                    for delta in [
                        egui::vec2(0., 0.),
                        egui::vec2(20., 0.),
                        egui::vec2(0., -10.),
                        egui::vec2(-10., 5.),
                    ] {
                        let screen =
                            rect.min + (position(at) + delta - camera.world_rect(rect).min) * zoom;
                        let i = flat.pick(camera.world_point(rect, screen)).unwrap();
                        assert_eq!([scene.cells[i].rx, scene.cells[i].ry], at);
                    }
                }
            }
            assert!(flat.pick(egui::pos2(-50000., -50000.)).is_none());
        }
    }
}
