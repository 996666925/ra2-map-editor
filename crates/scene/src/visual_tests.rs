use super::*;

#[test]
fn placed_building_uses_render_location_and_keeps_it_after_save_reopen() {
    use ra2_map_assets::test_support::Fixture;
    use ra2_map_document::new_map::NewMapOptions;
    use std::time::Instant;
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[BuildingTypes]\n0=GI\n[InfantryTypes]\n0=SOLDIER\n[SOLDIER]\nImage=GI\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("artmd.ini"),
        b"[GI]\nFoundation=3x2\nXDrawOffset=3\nYDrawOffset=4\n",
    )
    .unwrap();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    for level in [0, 4] {
        let scene = crate::map::load_scene_bytes(
            &mut install,
            "anchor.map",
            "test".into(),
            NewMapOptions {
                width: 20,
                height: 20,
                level,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        doc.add_record("Structures", [20, 20], "Neutral", "GI")
            .unwrap();
        doc.add_record("Infantry", [22, 22], "Neutral", "SOLDIER")
            .unwrap();
        doc.commit("place");
        let saved = doc.snapshot().bytes().unwrap();
        let scene = crate::map::load_scene_bytes(
            &mut install,
            "anchor.map",
            "test".into(),
            saved.clone(),
            Instant::now(),
        )
        .unwrap();
        let reopened = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        assert_eq!(reopened.snapshot().bytes().unwrap(), saved);
        for (id, record) in reopened.records() {
            if record.section == "Waypoints" {
                continue;
            }
            let at = record.cell().unwrap();
            // Building 459EF0 subtracts half a cell on both axes from
            // Location (NW cell centre). Foot objects retain their centre.
            let sub = if record.section == "Structures" {
                0
            } else {
                128
            };
            let (x, y) = vera20k_core::util::lepton::absolute_leptons_to_screen(
                i32::from(at[0]) * 256 + sub,
                i32::from(at[1]) * 256 + sub,
                i32::from(level) * 104,
            );
            let image = scene.sprites.objects[&(record.section.clone(), record.type_id())].image(0);
            let draw = scene
                .visual
                .draws
                .iter()
                .find(|d| d.owner == Some(id))
                .unwrap();
            assert_eq!(
                draw.position,
                [
                    (x + 3. + image.info.offset[0] as f32).round(),
                    (y + 4. + image.info.offset[1] as f32).round(),
                ],
                "{} at level {level}",
                record.section
            );
            assert_eq!(
                at,
                if record.section == "Structures" {
                    [20, 20]
                } else {
                    [22, 22]
                }
            );
        }
    }
}

fn fixture() -> VisualScene {
    let mut s = VisualScene::default();
    s.surfaces.push(std::sync::Arc::new(Surface {
        size: [2, 1],
        rgba: vec![50, 100, 150, 255, 60, 120, 180, 255],
        original: Vec::new(),
        depth: vec![200, 200],
    }));
    s.surfaces.push(std::sync::Arc::new(Surface {
        size: [2, 1],
        rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        original: vec![0, 0, 255, 255, 0, 0, 255, 255],
        depth: vec![201, 199],
    }));
    s.draws.push(Draw {
        surface: 0,
        position: [0., 0.],
        base: 0,
        tint: [1.; 3],
        layer: 0,
        writes: true,
        owner: None,
    });
    s.draws.push(Draw {
        surface: 1,
        position: [0., 0.],
        base: 0,
        tint: [0.5; 3],
        layer: 2,
        writes: false,
        owner: Some(9),
    });
    s.index();
    s
}

#[test]
fn incremental_bucket_moves_preserve_painter_order_and_full_raster_pixels() {
    let old = fixture();
    let mut updated = old.clone();
    updated.draws[1].position = [130., 0.];
    updated.index_from(Some(&old));
    let mut rebuilt = updated.clone();
    rebuilt.buckets.clear();
    rebuilt.index();
    let rect = egui::Rect::from_min_size(egui::pos2(-1., -1.), egui::vec2(134., 4.));
    assert_eq!(
        updated.raster(rect, [134, 4], Default::default()).unwrap(),
        rebuilt.raster(rect, [134, 4], Default::default()).unwrap()
    );
    for x in [-0.5, 0.5, 1.5, 129.5, 130.5, 131.5] {
        assert_eq!(
            updated.sample(egui::pos2(x, 0.5), Default::default()),
            rebuilt.sample(egui::pos2(x, 0.5), Default::default())
        );
    }
    let count = std::cell::Cell::new(0);
    assert!(
        updated
            .raster_controlled(rect, [134, 4], Default::default(), &|| {
                count.set(count.get() + 1);
                count.get() > 1
            })
            .is_err()
    );
    assert!(count.get() > 1);
}
#[test]
fn shared_depth_hides_pixels_behind_cliffs_and_picking_agrees_with_publication() {
    let s = fixture();
    let o = ViewOptions::default();
    assert_eq!(
        s.sample(egui::pos2(0.5, 0.5), o),
        ([50, 100, 150, 255], None)
    );
    assert_eq!(
        s.sample(egui::pos2(1.5, 0.5), o),
        ([0, 128, 0, 255], Some(9))
    );
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2., 1.));
    let rgba = s.raster(rect, [2, 1], o).unwrap();
    assert_eq!(rgba, [50, 100, 150, 255, 0, 128, 0, 255]);
}
#[test]
fn layer_lighting_remap_toggles_and_transparent_holes_preserve_ground() {
    let mut s = fixture();
    let p = egui::pos2(1.5, 0.5);
    assert_eq!(
        s.sample(
            p,
            ViewOptions {
                objects: false,
                ..Default::default()
            }
        )
        .0,
        [60, 120, 180, 255]
    );
    assert_eq!(
        s.sample(
            p,
            ViewOptions {
                remap: false,
                lighting: false,
                ..Default::default()
            }
        )
        .0,
        [0, 0, 255, 255]
    );
    std::sync::Arc::make_mut(&mut s.surfaces[1]).rgba[7] = 0;
    assert_eq!(s.sample(p, ViewOptions::default()).1, None);
    assert!(
        s.raster(egui::Rect::EVERYTHING, [9000, 9000], ViewOptions::default())
            .is_err()
    );
}
#[test]
fn owner_color_resolves_house_override_country_and_neutral() {
    let schemes = color_scheme::parse_color_schemes(&IniFile::from_str(
        "[Colors]\nRed=0,255,255\nBlue=160,255,255\n",
    ));
    let rules = IniFile::from_str("[Russians]\nColor=Red\n");
    let ini =
        IniFile::from_str("[Custom]\nCountry=Russians\nColor=Blue\n[Other]\nCountry=Russians\n");
    assert_eq!(
        resolve_house_color(&ini, &rules, &schemes, "Custom"),
        Some(1)
    );
    assert_eq!(
        resolve_house_color(&ini, &rules, &schemes, "Other"),
        Some(0)
    );
    assert_eq!(resolve_house_color(&ini, &rules, &schemes, "Neutral"), None);
}
