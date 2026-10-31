//! Object painting uses one pending document transaction for the entire drag.
use super::*;
use ra2_map_scene::placement;

#[derive(Default)]
pub(super) struct ObjectBrush {
    gesture: Option<Gesture>,
}
struct Settings {
    section: String,
    type_id: String,
    owner: String,
    subcell: u8,
    overlap: bool,
    offsets: Vec<(i16, i16)>,
}
struct Gesture {
    settings: Settings,
    last: Option<Cell>,
    visited: BTreeSet<Cell>,
    occupied: BTreeSet<Cell>,
    created: BTreeSet<u64>,
    selected: Option<u64>,
    skipped: usize,
    error: Option<String>,
}
impl ObjectBrush {
    pub fn active(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn cancel(&mut self) {
        self.gesture = None;
    }
}

fn place(doc: &mut MapDocument, scene: &MapScene, settings: &Settings, at: Cell) -> Result<u64> {
    doc.atomic(|doc| {
        // Terrain coordinates are INI keys: overlap must never replace a tree.
        ensure!(
            settings.section != "Terrain"
                || !doc
                    .records()
                    .values()
                    .any(|r| r.section == "Terrain" && r.cell() == Some(at)),
            "每格只能保存一条地形对象记录，请先移动或删除原对象"
        );
        let id = doc.add_record(&settings.section, at, &settings.owner, &settings.type_id)?;
        let mut record = doc.records()[&id].clone();
        if record.section == "Infantry" {
            record = record.patched(&[(5, settings.subcell.to_string())])?;
            doc.set(Target::Record(id), Some(Value::Record(record.clone())))?;
        }
        placement::validate(
            doc,
            scene,
            &[record],
            &BTreeSet::from([id]),
            settings.overlap,
            &BTreeMap::new(),
        )?;
        Ok(id)
    })
}

impl Editor {
    pub(super) fn object_pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
    ) -> bool {
        if self.object_brush.gesture.is_none() && (self.tool != Tool::Place || self.stroking) {
            return false;
        }
        if press && at.is_some() && self.object_brush.gesture.is_none() {
            if !scene
                .catalog
                .object_types
                .get(&self.section)
                .is_some_and(|types| types.contains(&self.type_id))
            {
                self.status = "对象类型不在当前 rulesmd.ini 注册表中".into();
                return true;
            }
            self.feedback.clear();
            self.stroking = true;
            self.object_brush.gesture = Some(Gesture {
                settings: Settings {
                    section: self.section.clone(),
                    type_id: self.type_id.clone(),
                    owner: self.owner.clone(),
                    subcell: self.infantry_subcell,
                    overlap: self.allow_overlap,
                    offsets: placement::offsets(scene, &self.section, &self.type_id),
                },
                last: None,
                visited: BTreeSet::new(),
                occupied: BTreeSet::new(),
                created: BTreeSet::new(),
                selected: None,
                skipped: 0,
                error: None,
            });
        }
        let Some(g) = self.object_brush.gesture.as_mut() else {
            return true;
        };
        let mut changed = false;
        if down || press || release {
            if let Some(at) = at {
                let line = cell_line(g.last.unwrap_or(at), at);
                g.last = Some(at);
                for cell in line {
                    if !g.visited.insert(cell) {
                        continue;
                    }
                    let footprint = g
                        .settings
                        .offsets
                        .iter()
                        .map(|&(dx, dy)| {
                            let x = i32::from(cell[0]) + i32::from(dx);
                            let y = i32::from(cell[1]) + i32::from(dy);
                            ensure!(
                                (0..i32::from(MAX_CELL_SIDE)).contains(&x)
                                    && (0..i32::from(MAX_CELL_SIDE)).contains(&y),
                                "对象占地超出固定网格"
                            );
                            Ok([x as u16, y as u16])
                        })
                        .collect::<Result<Vec<_>>>();
                    if footprint
                        .as_ref()
                        .is_ok_and(|cells| cells.iter().any(|c| g.occupied.contains(c)))
                    {
                        continue;
                    }
                    let result = footprint.and_then(|cells| {
                        let id = place(self.doc.as_mut().unwrap(), scene, &g.settings, cell)?;
                        Ok((id, cells))
                    });
                    match result {
                        Ok((id, cells)) => {
                            g.created.insert(id);
                            g.selected = Some(id);
                            self.feedback.extend(cells.iter().copied());
                            g.occupied.extend(cells);
                            changed = true;
                        }
                        Err(e) => {
                            g.skipped += 1;
                            g.error = Some(format!("{e:#}"));
                        }
                    }
                }
            } else {
                // Re-entering the canvas must not paint a line across the sidebar.
                g.last = None;
            }
        }
        self.status = format!(
            "对象连刷 · {} 个 · 跳过 {} 处；松手提交，Esc / 右键取消",
            g.created.len(),
            g.skipped
        );
        if release || !down {
            let g = self.object_brush.gesture.take().unwrap();
            self.stroking = false;
            self.commit(scene, "刷对象");
            if let Some(id) = g.selected {
                self.select(Some(id));
            }
            self.status = if g.created.is_empty() {
                g.error.map_or_else(
                    || "本笔未放置对象".into(),
                    |e| format!("本笔未放置对象：{e}"),
                )
            } else {
                format!("刷对象 · {} 个 · 跳过 {} 处", g.created.len(), g.skipped)
            };
        } else if changed {
            self.cache(scene);
        }
        true
    }

    /// Pending objects are visible while the published scene still represents
    /// the last committed map. Use the shared marker geometry for their sprites.
    pub(super) fn object_brush_preview(
        &mut self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
    ) {
        let Some(g) = self.object_brush.gesture.as_ref() else {
            return;
        };
        if !self.layers[1] {
            return;
        }
        let world = camera.world_rect(rect);
        for marker in &self.marker_cache.markers {
            if !g.created.contains(&marker.id) {
                continue;
            }
            if let (Some(image), Some(bounds)) = (&marker.image, marker.image_rect) {
                let target = egui::Rect::from_min_size(
                    rect.min + (bounds.min - world.min) * camera.zoom,
                    bounds.size() * camera.zoom,
                );
                if !target.intersects(rect) {
                    continue;
                }
                let texture = editing_ui::map_sprite_texture(
                    &mut self.ui_state.map_sprites,
                    painter.ctx(),
                    image,
                );
                painter.image(
                    texture,
                    target,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::from_white_alpha(160),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "object_brush_tests.rs"]
mod tests;
