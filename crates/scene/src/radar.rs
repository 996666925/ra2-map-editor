//! The engine's complete traditional preview, shared by navigation and PreviewPack.
use crate::map::MapScene;
use anyhow::Result;
use egui::{ColorImage, Pos2, Rect};
use ra2_map_document::document::MapDocument;

pub struct RadarImage {
    pub image: ColorImage,
    pub bounds: Rect,
}

/// Radar geography is flat: elevation changes colours, never the map frame.
pub fn terrain(scene: &MapScene) -> Result<RadarImage> {
    render(scene, None)
}

/// Current document overlays and waypoints use the same engine render as saves.
pub fn render(scene: &MapScene, doc: Option<&MapDocument>) -> Result<RadarImage> {
    use anyhow::Context;
    use vera20k_core::map::{resolved_terrain::ResolvedTerrainGrid, rmg::preview};

    let bytes = match doc {
        Some(doc) => doc.snapshot().bytes()?,
        None => scene.source.bytes().to_vec(),
    };
    let map = vera20k_core::map::map_file::load_from_bytes(bytes)?;
    let resolved = scene
        .resolved
        .as_deref()
        .context("没有可生成地图导航预览的地形")?;
    // The editor supports heights 13/14; the engine preview's TMP colour table
    // ends at 12. Saturate only this derived colour input, never authored heights.
    let saturated;
    let resolved = if scene.cells.iter().any(|cell| cell.z > 12) {
        let cells = resolved
            .cells()
            .iter()
            .cloned()
            .map(|mut cell| {
                cell.level = cell.level.min(12);
                cell
            })
            .collect();
        saturated = ResolvedTerrainGrid::from_cells(resolved.width(), resolved.height(), cells);
        &saturated
    } else {
        resolved
    };
    let palette = &scene.catalog.radar;
    let mut heads = std::collections::HashMap::new();
    // Native Terrain placements prepend ground content; the last decoration
    // covering a cell supplies its type colour, as in VERA's preview adapter.
    for object in &map.terrain_objects {
        if (object.rx, object.ry) == (0, 0) {
            continue;
        }
        let Some(kind) = palette.terrain.get(&object.name.to_ascii_uppercase()) else {
            continue;
        };
        if let Ok(offsets) = kind.occupied_cell_offsets() {
            for (dx, dy) in offsets {
                heads.insert(
                    (
                        object.rx.wrapping_add_signed(dx),
                        object.ry.wrapping_add_signed(dy),
                    ),
                    kind.radar_color,
                );
            }
        }
    }
    let mut cells = preview::preview_cells_from_map(
        &map,
        resolved,
        &|id, stage, ground| palette.overlay(id, stage, ground),
        &|x, y, ground| {
            heads
                .get(&(x, y))
                .map(|rgb| ra2_map_assets::radar::Palette::terrain_color(*rgb, ground))
        },
    );
    for cell in &mut cells {
        (cell.left, cell.right) = ra2_map_assets::radar::Palette::quantize((cell.left, cell.right));
    }
    let starts: Vec<_> = map
        .waypoints
        .values()
        .filter(|w| w.index < u32::from(preview::MARKER_WAYPOINT_COUNT) && (w.rx, w.ry) != (0, 0))
        .map(|w| (w.index as u8, w.rx, w.ry))
        .collect();
    let waypoints = preview::marker_waypoints(&starts);
    let mut rendered =
        preview::render_preview(&cells, &waypoints).context("没有可生成地图导航预览的地形")?;
    // Convert the engine's surface origin to the editor's flat canvas frame.
    // Each pair spans 60 world pixels and each row 30. Keep the same integer
    // projection as GenerateTerrainPreview so clicks and annotations stay aligned.
    let min_x = cells
        .iter()
        .map(|c| (i32::from(c.x) - i32::from(c.y) + 512) / 2 * 60 - 15360)
        .min()
        .unwrap();
    let min_y = cells
        .iter()
        .map(|c| (i32::from(c.x) + i32::from(c.y) + 1) / 2 * 30 + 15)
        .min()
        .unwrap();
    let bounds = Rect::from_min_size(
        egui::pos2(min_x as f32, min_y as f32),
        egui::vec2(rendered.width as f32 * 30., rendered.height as f32 * 30.),
    );
    // PreviewPack stores RGB, including black for unpainted pixels. Give the
    // navigator the same opaque background as the image decoded from the pack.
    for pixel in rendered.rgba.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let size = [rendered.width as usize, rendered.height as usize];
    Ok(RadarImage {
        image: ColorImage::from_rgba_unmultiplied(size, &rendered.rgba),
        bounds,
    })
}

pub fn cell_position(at: [u16; 2]) -> Pos2 {
    let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], 0);
    egui::pos2(x + 30., y + 15.)
}

/// Convert a radar click back to the elevated canvas at the selected cell.
pub fn canvas_position(scene: &MapScene, doc: Option<&MapDocument>, position: Pos2) -> Pos2 {
    let (rx, ry) = vera20k_core::map::terrain::screen_to_iso(position.x, position.y);
    let at = [rx.round().max(0.) as u16, ry.round().max(0.) as u16];
    let level = doc
        .and_then(|doc| doc.terrain(at).map(|terrain| terrain.level))
        .or_else(|| {
            scene
                .cells
                .iter()
                .find(|cell| [cell.rx, cell.ry] == at)
                .map(|cell| cell.z)
        })
        .unwrap_or(0);
    position - egui::vec2(0., f32::from(level as i8) * 15.)
}

/// Use the terrain under the viewport centre to place its radar outline.
pub fn radar_position(scene: &MapScene, position: Pos2) -> Pos2 {
    let level = scene
        .pick(position)
        .map(|index| scene.cells[index].z)
        .unwrap_or(0);
    position + egui::vec2(0., f32::from(level as i8) * 15.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra2_map_document::{
        document::{Target, Value},
        new_map::NewMapOptions,
    };
    use std::sync::Arc;

    fn scene(level: u8) -> MapScene {
        let fixture = ra2_map_assets::test_support::Fixture::new();
        crate::map::load_scene_bytes(
            &mut fixture.install(),
            "radar.map",
            "test".into(),
            NewMapOptions {
                width: 40,
                height: 25,
                level,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            std::time::Instant::now(),
        )
        .unwrap()
    }

    #[test]
    fn engine_preview_uses_overlay_metadata_and_red_starts_without_editor_symbols() {
        use vera20k_core::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};
        let mut scene = scene(0);
        let palette = ra2_map_assets::radar::Palette {
            overlays: OverlayTypeRegistry::from_ini(
                &IniFile::from_str(
                    "[OverlayTypes]\n0=GEM\n[GEM]\nTiberium=yes\nRadarColor=40,160,80\n",
                ),
                None,
            ),
            ..Default::default()
        };
        Arc::make_mut(&mut scene.catalog).radar = Arc::new(palette);
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let before = render(&scene, Some(&doc)).unwrap();
        for x in 24..=29 {
            for y in 24..=29 {
                doc.set(Target::Overlay([x, y]), Some(Value::Overlay(0, 8)))
                    .unwrap();
            }
        }
        let after = render(&scene, Some(&doc)).unwrap();
        assert_ne!(
            before.image, after.image,
            "overlay colour callback must reach the rendered image"
        );
        assert!(
            after
                .image
                .pixels
                .contains(&egui::Color32::from_rgb(32, 160, 64))
        );
        assert!(
            !after
                .image
                .pixels
                .contains(&egui::Color32::from_rgb(230, 80, 220)),
            "old editor gem dots must be absent"
        );
        assert!(
            after
                .image
                .pixels
                .contains(&egui::Color32::from_rgb(240, 0, 0)),
            "engine's red start squares must be present"
        );
        doc.add_record("Units", [25, 25], "Neutral", "MISSING")
            .unwrap();
        assert_eq!(
            after.image,
            render(&scene, Some(&doc)).unwrap().image,
            "traditional previews do not invent unit dots"
        );
    }

    #[test]
    fn elevated_rectangular_maps_keep_preview_bounds_and_pending_click_height() {
        let flat = terrain(&scene(0)).unwrap();
        let raised = scene(14);
        let radar = terrain(&raised).unwrap();
        assert_eq!(flat.bounds, radar.bounds);
        assert_eq!(flat.image.size, radar.image.size);
        let mut doc = MapDocument::new(raised.source.edit_base.as_ref().unwrap().clone());
        let at = [25, 25];
        let position = cell_position(at);
        let mut cell = doc.terrain(at).unwrap();
        cell.level = 10;
        doc.set(Target::Terrain(at), Some(Value::Terrain(cell)))
            .unwrap();
        assert_eq!(
            canvas_position(&raised, Some(&doc), position),
            position - egui::vec2(0., 150.)
        );
    }
}
