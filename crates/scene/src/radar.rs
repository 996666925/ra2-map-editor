//! Shared small-map colours, projection and symbols for navigation and PreviewPack.
use crate::map::MapScene;
use anyhow::Result;
use egui::{Color32, ColorImage, Pos2, Rect};
use ra2_map_document::document::{Cell, MapDocument};
use std::collections::{BTreeMap, HashMap};

pub const MAX_EDGE: u32 = 512;

pub struct RadarImage {
    pub image: ColorImage,
    pub bounds: Rect,
}

#[derive(Clone, Copy)]
pub enum SymbolKind {
    Resource(bool),
    Building,
    Unit,
    Start(u8),
    /// A cell of bridge deck, drawn at map scale beneath optional markers.
    Bridge(Color32),
}

pub struct Symbol {
    pub position: Pos2,
    pub kind: SymbolKind,
}

/// Radar geography is flat: elevation changes colours, never the map frame.
pub fn terrain(scene: &MapScene) -> Result<RadarImage> {
    let mut bounds = Rect::NOTHING;
    let mut cells = HashMap::with_capacity(scene.cells.len());
    for cell in &scene.cells {
        let center = cell_position([cell.rx, cell.ry]);
        bounds = bounds.union(Rect::from_center_size(center, egui::vec2(60., 30.)));
        cells.insert((cell.rx, cell.ry), cell);
    }
    anyhow::ensure!(
        bounds.is_finite() && bounds.is_positive(),
        "没有可生成地图导航预览的地形"
    );
    // Only the perimeter half-diamonds are removed. Interior height steps must
    // never be treated as a reason to crop away a region of the map.
    let inset = bounds.shrink2(egui::vec2(30., 15.));
    if inset.is_positive() {
        bounds = inset;
    }
    let scale = (MAX_EDGE as f32 / bounds.width().max(bounds.height())).min(1.);
    let size = [
        (bounds.width() * scale).round().max(1.) as usize,
        (bounds.height() * scale).round().max(1.) as usize,
    ];
    let mut pixels = Vec::with_capacity(size[0] * size[1]);
    for y in 0..size[1] {
        for x in 0..size[0] {
            let position = bounds.min
                + bounds.size()
                    * egui::vec2(
                        (x as f32 + 0.5) / size[0] as f32,
                        (y as f32 + 0.5) / size[1] as f32,
                    );
            let (rx, ry) = vera20k_core::map::terrain::screen_to_iso(position.x, position.y);
            let color = cells.get(&(rx.round() as u16, ry.round() as u16)).map_or(
                Color32::from_rgb(17, 21, 26),
                |cell| {
                    let center = cell_position([cell.rx, cell.ry]);
                    let rgb = if position.x < center.x {
                        cell.radar_left
                    } else {
                        cell.radar_right
                    };
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                },
            );
            pixels.push(color);
        }
    }
    Ok(RadarImage {
        image: ColorImage::new(size, pixels),
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

pub fn symbols(scene: &MapScene, doc: &MapDocument) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let mut bridges = BTreeMap::new();
    let starts = ra2_map_document::player_starts::from_document(doc);
    for cell in &scene.cells {
        let at = [cell.rx, cell.ry];
        let (id, _) = doc.overlay(at);
        if vera20k_core::rules::overlay_types::is_bridge_overlay_index(id) {
            let color = bridge_color(scene, id);
            if let Some((_, direction)) =
                vera20k_core::map::bridge_facts::high_bridge_stamp_for_overlay(id)
            {
                // High overlays author one anchor per cross row. Include its
                // opposite neighbour and two forward structural deck cells.
                let (dx, dy) = vera20k_core::util::direction::direction_delta(direction).unwrap();
                for step in -1..=2 {
                    let x = i32::from(at[0]) + dx * step;
                    let y = i32::from(at[1]) + dy * step;
                    if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y))
                        && doc.terrain([x, y]).is_some()
                    {
                        bridges.insert([x, y], color);
                    }
                }
            } else {
                // Low bridges already store all three cross-row cells.
                bridges.insert(at, color);
            }
        }
        let gem = scene.catalog.gem_family(id);
        if gem || scene.catalog.ore_family(id) {
            symbols.push(Symbol {
                position: cell_position(at),
                kind: SymbolKind::Resource(gem),
            });
        }
    }
    for record in doc.records().values() {
        let Some(at) = record.cell() else { continue };
        let kind = match record.section.as_str() {
            "Structures" => SymbolKind::Building,
            "Units" | "Infantry" | "Aircraft" => SymbolKind::Unit,
            "Waypoints" => {
                let Ok(number @ 0..=29) = record.key.parse::<u8>() else {
                    continue;
                };
                if record.key != number.to_string() || !starts.contains(&number) {
                    continue;
                }
                SymbolKind::Start(number)
            }
            _ => continue,
        };
        symbols.push(Symbol {
            position: cell_position(at),
            kind,
        });
    }
    symbols.extend(bridges.into_iter().map(|(at, color)| Symbol {
        position: cell_position(at),
        kind: SymbolKind::Bridge(color),
    }));
    symbols
}

fn bridge_color(scene: &MapScene, id: u8) -> Color32 {
    let fallback = if matches!(id, 74..=101 | 122..=125 | 237 | 238) {
        [174, 145, 103]
    } else {
        [184, 188, 192]
    };
    let rgb = scene
        .catalog
        .overlays
        .get(usize::from(id))
        .map_or(fallback, |name| {
            scene
                .catalog
                .rules
                .section_or_empty(name)
                .read_color_rgb("RadarColor", fallback, None)
        });
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Flags are starts/resources/buildings/units; scale is pixels per symbol unit.
/// Bridges are geographic features and remain visible when markers are hidden.
pub fn overlay(
    size: [usize; 2],
    bounds: Rect,
    symbols: &[Symbol],
    flags: [bool; 4],
    scale: f32,
) -> ColorImage {
    let mut image = ColorImage::new(size, vec![Color32::TRANSPARENT; size[0] * size[1]]);
    draw_bridges(&mut image, bounds, symbols);
    for symbol in symbols {
        let center = (symbol.position - bounds.min) / bounds.size()
            * egui::vec2(size[0] as f32, size[1] as f32);
        let (visible, radius, color) = match symbol.kind {
            SymbolKind::Bridge(_) => continue,
            SymbolKind::Resource(gem) => (
                flags[1],
                1.,
                if gem {
                    Color32::from_rgb(230, 80, 220)
                } else {
                    Color32::from_rgb(255, 205, 50)
                },
            ),
            SymbolKind::Building => (flags[2], 1.8, Color32::from_rgb(255, 135, 65)),
            SymbolKind::Unit => (flags[3], 1.1, Color32::from_rgb(255, 135, 65)),
            // Leave padding around both single- and double-digit position labels.
            SymbolKind::Start(_) => (flags[0], 5.5, Color32::BLACK),
        };
        if !visible {
            continue;
        }
        let radius = radius * scale;
        let left = (center.x - radius - 1.).floor().max(0.) as usize;
        let top = (center.y - radius - 1.).floor().max(0.) as usize;
        let right = ((center.x + radius + 1.).ceil().max(0.) as usize).min(size[0]);
        let bottom = ((center.y + radius + 1.).ceil().max(0.) as usize).min(size[1]);
        for y in top..bottom {
            for x in left..right {
                let a = (radius + 0.5
                    - (egui::vec2(x as f32 + 0.5, y as f32 + 0.5) - center).length())
                .clamp(0., 1.);
                let color = Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (a * 255.) as u8,
                );
                let pixel = &mut image.pixels[y * size[0] + x];
                *pixel = pixel.blend(color);
            }
        }
    }
    // Labels stay above all resource and object symbols.
    for symbol in symbols {
        if let SymbolKind::Start(number) = symbol.kind
            && flags[0]
        {
            let center = (symbol.position - bounds.min) / bounds.size()
                * egui::vec2(size[0] as f32, size[1] as f32);
            // Player positions are one-based; map waypoint IDs remain zero-based.
            label(&mut image, center, number + 1, scale);
        }
    }
    image
}

/// Sample the union of deck cells, so adjacent cells form a continuous strip
/// instead of dots or overlapping outlines. Subpixel samples retain thin spans
/// when a large map is reduced to a small navigator.
fn draw_bridges(image: &mut ColorImage, bounds: Rect, symbols: &[Symbol]) {
    let mut cells = HashMap::<Cell, Color32>::new();
    let mut deck_bounds = Rect::NOTHING;
    for symbol in symbols {
        if let SymbolKind::Bridge(color) = symbol.kind {
            let (x, y) =
                vera20k_core::map::terrain::screen_to_iso(symbol.position.x, symbol.position.y);
            cells.insert([x.round() as u16, y.round() as u16], color);
            deck_bounds = deck_bounds.union(Rect::from_center_size(
                symbol.position,
                egui::vec2(60., 30.),
            ));
        }
    }
    if cells.is_empty() {
        return;
    }
    let [width, height] = image.size;
    let pixels = egui::vec2(width as f32, height as f32);
    let start = (deck_bounds.min - bounds.min) / bounds.size() * pixels;
    let end = (deck_bounds.max - bounds.min) / bounds.size() * pixels;
    for y in (start.y.floor().max(0.) as usize)..(end.y.ceil().max(0.) as usize).min(height) {
        for x in (start.x.floor().max(0.) as usize)..(end.x.ceil().max(0.) as usize).min(width) {
            let mut sum = [0u32; 3];
            let mut hits = 0;
            for dy in [0.125, 0.375, 0.625, 0.875] {
                for dx in [0.125, 0.375, 0.625, 0.875] {
                    let p = bounds.min
                        + bounds.size() * (egui::vec2(x as f32 + dx, y as f32 + dy) / pixels);
                    let (rx, ry) = vera20k_core::map::terrain::screen_to_iso(p.x, p.y);
                    if rx >= 0.
                        && ry >= 0.
                        && let Some(color) = cells.get(&[rx.round() as u16, ry.round() as u16])
                    {
                        for (total, channel) in
                            sum.iter_mut().zip([color.r(), color.g(), color.b()])
                        {
                            *total += u32::from(channel);
                        }
                        hits += 1;
                    }
                }
            }
            if let Some(red) = sum[0].checked_div(hits) {
                image.pixels[y * width + x] = Color32::from_rgba_unmultiplied(
                    red as u8,
                    (sum[1] / hits) as u8,
                    (sum[2] / hits) as u8,
                    (hits * 255 / 16) as u8,
                );
            }
        }
    }
}

fn label(image: &mut ColorImage, center: egui::Vec2, number: u8, scale: f32) {
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    let text = number.to_string();
    let origin = center - egui::vec2((text.len() * 4 - 1) as f32, 5.) * (scale * 0.5);
    for (digit, byte) in text.bytes().enumerate() {
        for (row, bits) in DIGITS[(byte - b'0') as usize].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                let start = origin + egui::vec2((digit * 4 + col) as f32, row as f32) * scale;
                let end = start + egui::Vec2::splat(scale);
                for y in start.y.round().max(0.) as usize
                    ..(end.y.round().max(0.) as usize).min(image.size[1])
                {
                    for x in start.x.round().max(0.) as usize
                        ..(end.x.round().max(0.) as usize).min(image.size[0])
                    {
                        image.pixels[y * image.size[0] + x] = Color32::WHITE;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(width: u16, height: u16, level: u8) -> MapScene {
        let fixture = ra2_map_assets::test_support::Fixture::new();
        let mut install = fixture.install();
        crate::map::load_scene_bytes(
            &mut install,
            "radar.map",
            "test".into(),
            ra2_map_document::new_map::NewMapOptions {
                width,
                height,
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
    fn radar_uses_both_cell_colours_and_preserves_black_land() {
        let mut scene = scene(20, 20, 0);
        for cell in &mut scene.cells {
            cell.radar_left = [0, 0, 0];
            cell.radar_right = [20, 40, 200];
        }
        let radar = terrain(&scene).unwrap();
        let blue = Color32::from_rgb(20, 40, 200);
        assert!(
            radar
                .image
                .pixels
                .iter()
                .all(|p| *p == Color32::BLACK || *p == blue)
        );
        assert!(radar.image.pixels.contains(&Color32::BLACK));
        assert!(radar.image.pixels.contains(&blue));
    }

    #[test]
    fn raised_plateau_does_not_crop_away_other_regions_or_player_starts() {
        let mut scene = scene(120, 120, 0);
        let flat = terrain(&scene).unwrap();
        let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let flat_starts: Vec<_> = symbols(&scene, &doc)
            .into_iter()
            .filter(|s| matches!(s.kind, SymbolKind::Start(_)))
            .map(|s| s.position)
            .collect();
        for cell in &mut scene.cells {
            if (35..=100).contains(&cell.rx) && (35..=100).contains(&cell.ry) {
                cell.z = 12;
                (cell.screen_x, cell.screen_y) =
                    vera20k_core::map::terrain::iso_to_screen(cell.rx, cell.ry, cell.z);
            }
        }
        let raised = terrain(&scene).unwrap();
        assert_eq!(
            raised.bounds, flat.bounds,
            "interior height changes must not change radar framing"
        );
        assert_eq!(raised.image.size, flat.image.size);
        assert!(raised.image.pixels.iter().all(|p| p.a() == 255));
        let raised_starts: Vec<_> = symbols(&scene, &doc)
            .into_iter()
            .filter(|s| matches!(s.kind, SymbolKind::Start(_)))
            .map(|s| s.position)
            .collect();
        assert_eq!(
            flat_starts, raised_starts,
            "terrain and markers must use the same flat projection"
        );
        for position in flat_starts {
            assert!(
                raised.bounds.contains(position),
                "player start was cropped away"
            );
        }
    }

    #[test]
    fn rectangular_and_elevated_radar_maps_have_no_empty_border_pixels() {
        for (width, height, level) in [(80, 20, 0), (20, 80, 14)] {
            let elevated = scene(width, height, level);
            let radar = terrain(&elevated).unwrap();
            assert!(radar.image.size.iter().all(|n| *n > 0 && *n <= 512));
            assert!(radar.image.pixels.iter().all(|p| p.a() == 255));
            let flat = terrain(&scene(width, height, 0)).unwrap();
            assert!(radar.bounds.is_positive());
            assert_eq!(radar.bounds, flat.bounds);
            assert_eq!(radar.image.size, flat.image.size);
        }
    }

    #[test]
    fn radar_clicks_follow_elevated_cells_and_pending_height_edits() {
        use ra2_map_document::document::{Target, Value};
        let scene = scene(20, 20, 4);
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let at = [20, 20];
        let flat = cell_position(at);
        assert_eq!(
            canvas_position(&scene, None, flat),
            flat - egui::vec2(0., 60.)
        );
        let mut terrain = doc.terrain(at).unwrap();
        terrain.level = 10;
        doc.set(Target::Terrain(at), Some(Value::Terrain(terrain)))
            .unwrap();
        assert_eq!(
            canvas_position(&scene, Some(&doc), flat),
            flat - egui::vec2(0., 150.)
        );
        let center = canvas_position(&scene, None, flat);
        assert_eq!(radar_position(&scene, center), flat);
    }

    #[test]
    fn radar_symbols_follow_pending_edits_categories_and_thirty_player_starts() {
        use ra2_map_document::document::{Target, Value};
        let mut scene = scene(20, 20, 0);
        std::sync::Arc::make_mut(&mut scene.catalog).overlays =
            vec!["TIB01".into(), "GEM01".into()];
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(0, 0)))
            .unwrap();
        doc.set(Target::Overlay([21, 20]), Some(Value::Overlay(1, 0)))
            .unwrap();
        doc.add_record("Structures", [20, 20], "Neutral", "MISSING")
            .unwrap();
        doc.add_record("Infantry", [22, 20], "Neutral", "MISSING")
            .unwrap();
        doc.waypoint(29, [23, 20]).unwrap();
        doc.waypoint(30, [24, 20]).unwrap();
        doc.set_ini("Basic", "MaxPlayers", Some(b"30".to_vec()))
            .unwrap();
        let symbols = symbols(&scene, &doc);
        assert_eq!(
            symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Resource(false)))
                .count(),
            1
        );
        assert_eq!(
            symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Resource(true)))
                .count(),
            1
        );
        assert_eq!(
            symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Building))
                .count(),
            1
        );
        assert_eq!(
            symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Unit))
                .count(),
            1
        );
        assert!(
            symbols
                .iter()
                .any(|s| matches!(s.kind, SymbolKind::Start(29)))
        );
        assert!(
            !symbols
                .iter()
                .any(|s| matches!(s.kind, SymbolKind::Start(30)))
        );
        let radar = terrain(&scene).unwrap();
        let hidden = overlay(radar.image.size, radar.bounds, &symbols, [false; 4], 1.);
        assert!(hidden.pixels.iter().all(|p| p.a() == 0));
        for flag in 0..4 {
            let mut flags = [false; 4];
            flags[flag] = true;
            let image = overlay(radar.image.size, radar.bounds, &symbols, flags, 1.);
            assert!(image.pixels.iter().any(|p| p.a() != 0));
            if flag == 0 {
                assert!(image.pixels.contains(&Color32::WHITE));
            }
        }
    }

    #[test]
    fn mission_waypoints_are_excluded_from_preview_and_navigation_starts() {
        let scene = scene(20, 20, 0);
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        for n in [8, 29, 30, 50] {
            doc.waypoint(n, [20, 20]).unwrap();
        }
        let starts = |doc: &MapDocument| {
            symbols(&scene, doc)
                .into_iter()
                .filter_map(|s| {
                    if let SymbolKind::Start(n) = s.kind {
                        Some(n)
                    } else {
                        None
                    }
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(starts(&doc), std::collections::BTreeSet::from([0, 1]));
        // Native ordinary maps recognize authored start slots 0 through 7.
        doc.waypoint(7, [21, 20]).unwrap();
        assert_eq!(starts(&doc), std::collections::BTreeSet::from([0, 1, 7]));
        // Pending metadata changes affect every caller of the shared symbols.
        doc.set_ini("Basic", "MaxPlayers", Some(b"30".to_vec()))
            .unwrap();
        assert_eq!(
            starts(&doc),
            std::collections::BTreeSet::from([0, 1, 7, 8, 29])
        );
    }

    #[test]
    fn bridge_decks_cover_both_axes_and_materials_at_navigator_and_preview_sizes() {
        use ra2_map_document::document::{Target, Value};
        let scene = scene(20, 20, 0);
        let radar = terrain(&scene).unwrap();
        for id in [24, 25, 237, 238, 74, 83, 101, 122, 205, 214, 232, 233] {
            let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
            let high = vera20k_core::rules::overlay_types::is_high_bridge_index(id);
            let along_x = !matches!(id, 25 | 238 | 83 | 214);
            let rotate = |[x, y]: Cell| if along_x { [x, y] } else { [y, x] };
            for x in 16..=24 {
                if high {
                    doc.set(
                        Target::Overlay(rotate([x, 20])),
                        Some(Value::Overlay(id, if along_x { 0 } else { 9 })),
                    )
                    .unwrap();
                } else {
                    for y in 19..=21 {
                        doc.set(
                            Target::Overlay(rotate([x, y])),
                            Some(Value::Overlay(id, (y - 19) as u8)),
                        )
                        .unwrap();
                    }
                }
            }
            let symbols = symbols(&scene, &doc);
            assert_eq!(
                symbols
                    .iter()
                    .filter(|s| matches!(s.kind, SymbolKind::Bridge(_)))
                    .count(),
                if high { 36 } else { 27 }
            );
            for size in [[96, 48], [256, 128], radar.image.size] {
                // Bridge geography is independent of optional marker toggles.
                let image = overlay(size, radar.bounds, &symbols, [false; 4], 1.);
                for step in 0..=64 {
                    let a = cell_position(rotate([16, 20]));
                    let b = cell_position(rotate([24, 20]));
                    let position = a.lerp(b, step as f32 / 64.);
                    let pixel = (position - radar.bounds.min) / radar.bounds.size()
                        * egui::vec2(size[0] as f32, size[1] as f32);
                    assert!(
                        image.pixels[pixel.y as usize * size[0] + pixel.x as usize].a() > 0,
                        "missing bridge {id} at {step} in {size:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn bridge_surface_tracks_pending_edits_history_and_rules_colors() {
        use ra2_map_document::document::{Target, Value};
        let mut scene = scene(20, 20, 0);
        let catalog = std::sync::Arc::make_mut(&mut scene.catalog);
        catalog.overlays.resize(256, String::new());
        catalog.overlays[24] = "BRIDGE1".into();
        catalog.rules = vera20k_core::rules::ini_parser::IniFile::from_str(
            "[BRIDGE1]\nRadarColor=90,160,200\n",
        );
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let radar = terrain(&scene).unwrap();
        let render = |doc: &MapDocument| {
            overlay(
                radar.image.size,
                radar.bounds,
                &symbols(&scene, doc),
                [false; 4],
                1.,
            )
        };
        let before = render(&doc);
        assert!(before.pixels.iter().all(|p| p.a() == 0));
        doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(24, 0)))
            .unwrap();
        let after = render(&doc);
        assert!(after.pixels.contains(&Color32::from_rgb(90, 160, 200)));
        doc.commit("bridge");
        doc.undo();
        assert_eq!(render(&doc), before);
        doc.redo();
        assert_eq!(render(&doc), after);
        doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(255, 0)))
            .unwrap();
        assert_eq!(render(&doc), before);
    }
}
