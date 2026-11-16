//! Actual-theater authoring, engine terrain reloads and reviewable map/PNGs.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::MapDocument,
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{
    bridge_tools::{self, BridgeKind},
    cliff_authoring,
    cliff_tools::{self, PlateauPlan},
    flatten_tools, height_tools,
    map::{MapScene, load_scene_bytes},
    visual::ViewOptions,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Instant,
};
fn document(scene: &MapScene) -> MapDocument {
    MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone())
}
fn with_lat(doc: &MapDocument, scene: &MapScene, mut plan: PlateauPlan) -> Result<PlateauPlan> {
    let mut copy = doc.fork();
    let mut changed: std::collections::BTreeSet<_> = plan.apply(&mut copy)?.into_iter().collect();
    changed.extend(ra2_map_scene::terrain_tools::recalculate(
        &mut copy, scene, &changed,
    )?);
    plan.edits = changed
        .into_iter()
        .filter_map(|at| {
            let v = ra2_map_scene::terrain_tools::value(&copy, scene, at);
            (v != ra2_map_scene::terrain_tools::value(doc, scene, at)).then_some((at, v))
        })
        .collect();
    Ok(plan)
}
fn verify_joint_pixels(scene: &MapScene, p: &PlateauPlan, out: &Path, name: &str) -> Result<()> {
    let mut joint_cells = BTreeSet::new();
    for s in &p.cliffs {
        let dx = i32::from(s.origin[0]) - 80;
        let dy = i32::from(s.origin[1]) - 80;
        if dx * dx + dy * dy > 16 {
            continue;
        }
        let t = &scene.palette.templates[&(s.tile, 0)];
        for (sub, c) in t.cells.iter().enumerate() {
            if c.is_some() {
                joint_cells.insert([
                    s.origin[0] + sub as u16 % t.width,
                    s.origin[1] + sub as u16 / t.width,
                ]);
            }
        }
    }
    let cells: Vec<_> = scene
        .cells
        .iter()
        .filter(|c| joint_cells.contains(&[c.rx, c.ry]))
        .collect();
    ensure!(!cells.is_empty(), "{name}: no joint cells to render");
    let mut bounds = egui::Rect::NOTHING;
    for c in &cells {
        bounds = bounds.union(scene.cell_rect(c).context("missing native joint image")?);
    }
    bounds = bounds.expand(30.);
    let size = [bounds.width() as u32, bounds.height() as u32];
    let rgba = scene.visual.raster(
        bounds,
        size,
        ViewOptions {
            lighting: false,
            ..Default::default()
        },
    )?;
    let mut checked = 0;
    for c in cells {
        let image = scene.image(c).context("missing joint subtile")?;
        let x = (c.screen_x + image.offset_x as f32 - bounds.min.x) as usize;
        let y = (c.screen_y + image.offset_y as f32 - bounds.min.y) as usize;
        for (i, pixel) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let at =
                ((y + i / image.width as usize) * size[0] as usize + x + i % image.width as usize)
                    * 4;
            ensure!(
                rgba[at..at + 4] != [11, 15, 20, 255],
                "{name}: native joint pixel became a background hole"
            );
            checked += 1;
        }
    }
    ensure!(checked > 0, "{name}: empty native joint images");
    image::save_buffer(
        out.join(format!("{name}-detail.png")),
        &rgba,
        size[0],
        size[1],
        image::ColorType::Rgba8,
    )?;
    Ok(())
}
fn verify(
    install: &mut Install,
    scene: &MapScene,
    doc: &mut MapDocument,
    p: &PlateauPlan,
    out: &Path,
    name: &str,
    png: bool,
) -> Result<serde_json::Value> {
    let before = doc.snapshot().bytes()?;
    p.apply(doc)?;
    doc.commit(name);
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(out.join(format!("{name}.map")), &bytes)?;
    let reloaded = load_scene_bytes(
        install,
        name,
        "audit reload".into(),
        bytes.clone(),
        Instant::now(),
    )?;
    let reopened = document(&reloaded);
    let cells: BTreeMap<_, _> = reloaded.cells.iter().map(|c| ([c.rx, c.ry], c)).collect();
    if name.starts_with("open-") {
        let ids = scene
            .palette
            .tile_ids
            .context("missing terrain configuration")?;
        let mut stamped = BTreeSet::new();
        for s in &p.cliffs {
            let id = u32::from(s.tile);
            ensure!(
                ids.cliffs
                    .cliff_set
                    .is_some_and(|start| (u32::from(start)..u32::from(start) + 40).contains(&id))
                    || ids
                        .cliffs
                        .water_cliffs
                        .is_some_and(
                            |start| (u32::from(start)..u32::from(start) + 22).contains(&id)
                        ),
                "{name}: open path generated a non-cliff template"
            );
            let t = &scene.palette.templates[&(s.tile, 0)];
            for (sub, c) in t.cells.iter().enumerate() {
                if c.is_some() {
                    stamped.insert([
                        s.origin[0] + sub as u16 % t.width,
                        s.origin[1] + sub as u16 / t.width,
                    ]);
                }
            }
        }
        ensure!(
            stamped == p.edits.keys().copied().collect(),
            "{name}: open path modified ground outside the cliff templates"
        );
        ensure!(
            p.interior.is_empty(),
            "{name}: open path filled an interior"
        );
        for c in &scene.cells {
            let at = [c.rx, c.ry];
            if !stamped.contains(&at) {
                let actual = cells.get(&at).context("missing surrounding terrain cell")?;
                ensure!(
                    (actual.tile_id, actual.sub_tile, actual.z) == (c.tile_id, c.sub_tile, c.z),
                    "{name}: surrounding terrain changed at {at:?}"
                );
            }
        }
    }
    for (&at, &v) in &p.edits {
        ensure!(
            reopened.terrain(at) == Some(v),
            "{name}: saved terrain changed at {at:?}"
        );
        let c = cells.get(&at).context("engine cell absent")?;
        ensure!(
            (c.tile_id, c.sub_tile, c.z) == (scene.palette.raw_ids[&v.tile], v.sub, v.level),
            "{name}: engine terrain changed at {at:?}: actual {:?}, expected {:?}",
            (c.tile_id, c.sub_tile, c.z),
            (scene.palette.raw_ids[&v.tile], v.sub, v.level)
        );
    }
    for (&at, &v) in &p.overlays {
        ensure!(
            reopened.overlay(at) == v,
            "{name}: saved bridge overlay changed at {at:?}"
        );
    }
    doc.undo();
    ensure!(doc.snapshot().bytes()? == before, "{name}: undo mismatch");
    doc.redo();
    ensure!(doc.snapshot().bytes()? == bytes, "{name}: redo mismatch");
    if png {
        if name.starts_with("open-joint-") {
            verify_joint_pixels(&reloaded, p, out, name)?;
        }
        let at: Vec<_> = p.edits.keys().chain(p.overlays.keys()).copied().collect();
        let lo = [
            at.iter().map(|p| p[0]).min().unwrap().saturating_sub(4),
            at.iter().map(|p| p[1]).min().unwrap().saturating_sub(4),
        ];
        let hi = [
            at.iter().map(|p| p[0]).max().unwrap() + 4,
            at.iter().map(|p| p[1]).max().unwrap() + 4,
        ];
        let mut bounds = egui::Rect::NOTHING;
        for c in &reloaded.cells {
            if c.rx >= lo[0]
                && c.ry >= lo[1]
                && c.rx <= hi[0]
                && c.ry <= hi[1]
                && let Some(r) = reloaded.cell_rect(c)
            {
                bounds = bounds.union(r);
            }
        }
        bounds = bounds.expand(90.);
        let scale = (1200. / bounds.width().max(bounds.height())).min(1.);
        let size = [
            (bounds.width() * scale).ceil() as u32,
            (bounds.height() * scale).ceil() as u32,
        ];
        let rgba = reloaded.visual.raster(
            bounds,
            size,
            ViewOptions {
                lighting: false,
                ..Default::default()
            },
        )?;
        image::save_buffer(
            out.join(format!("{name}.png")),
            &rgba,
            size[0],
            size[1],
            image::ColorType::Rgba8,
        )?;
        if name.starts_with("closed-") {
            let original = scene.visual.raster(
                bounds,
                size,
                ViewOptions {
                    lighting: false,
                    ..Default::default()
                },
            )?;
            let holes = original
                .as_chunks::<4>()
                .0
                .iter()
                .zip(rgba.as_chunks::<4>().0.iter())
                .filter(|(old, new)| **old != [11, 15, 20, 255] && **new == [11, 15, 20, 255])
                .count();
            ensure!(
                holes == 0,
                "{name}: generated terrain exposes {holes} background pixels"
            );
        }
    }
    Ok(
        serde_json::json!({"case":name,"terrain_cells":p.edits.len(),"overlay_cells":p.overlays.len(),"engine_reload":true,"undo_redo":true,"cliff_only_checked":name.starts_with("open-"),"pixel_coverage_checked":png && name.starts_with("closed-"),"joint_pixel_coverage_checked":png && name.starts_with("open-joint-")}),
    )
}
fn audit(
    install: &mut Install,
    theater: &str,
    out: &Path,
    height_only: bool,
) -> Result<serde_json::Value> {
    std::fs::create_dir_all(out)?;
    let scene = load_scene_bytes(
        install,
        "authoring.map",
        "audit".into(),
        NewMapOptions {
            theater: theater.into(),
            width: 80,
            height: 80,
            level: 4,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let mut cases = Vec::new();
    let mut doc = document(&scene);
    if scene
        .palette
        .tile_ids
        .and_then(|ids| ids.cliffs.cliff_set)
        .is_none()
    {
        ensure!(
            cliff_authoring::line(&doc, &scene, [75, 75], [87, 75], 4, false).is_err(),
            "unsupported cliff theater accepted"
        );
        return Ok(serde_json::json!({"theater":theater,"unsupported_cliffs_rejected":true}));
    }
    let mut standard_templates = 0;
    let mut standard_subtiles = 0;
    for &tile in scene.palette.groups.keys() {
        let ids = scene.palette.tile_ids.context("missing tile ids")?;
        let standard = ids
            .cliffs
            .cliff_set
            .is_some_and(|base| (base..base + 40).contains(&tile))
            || ids.cliffs.water_cliffs.is_some_and(|base| {
                (base..base + 22).contains(&tile)
                    && ra2_map_scene::coastal_cliffs::supports(&scene, tile - base)
            });
        if !standard {
            continue;
        }
        let Some(t) = scene.palette.templates.get(&(tile, 0)) else {
            continue;
        };
        let sub = t
            .cells
            .iter()
            .position(Option::is_some)
            .context("empty standard cliff")?;
        let brush = ra2_map_assets::catalog::TerrainBrush {
            key: vera20k_core::map::theater::TileKey {
                tile_id: tile,
                sub_tile: sub as u8,
                variant: 0,
            },
            raw: scene.catalog.raw_tiles[&tile],
            label: "height audit".into(),
        };
        let mut probe = document(&scene);
        let edits = ra2_map_scene::terrain_tools::template_placement(
            &probe,
            &scene,
            &brush,
            [70, 70],
            Some(4),
        )?;
        for &(at, v) in &edits {
            probe.set(
                ra2_map_document::document::Target::Terrain(at),
                Some(ra2_map_document::document::Value::Terrain(v)),
            )?;
        }
        for &(at, _) in &edits {
            ensure!(
                height_tools::sample(&probe, &scene, at)? == 8,
                "{theater}: cliff {tile} at {at:?} picked the wrong cliff top"
            );
            standard_subtiles += 1;
        }
        standard_templates += 1;
    }
    // A pre-existing cliff ring whose interior has not yet been raised.
    let mut fill_doc = document(&scene);
    let mut ring = cliff_tools::plan(
        &fill_doc,
        &scene,
        cliff_tools::rectangle([60, 60], [74, 74]),
        4,
    )?;
    for at in &ring.interior {
        ring.edits
            .get_mut(at)
            .context("interior cell absent")?
            .level = 4;
    }
    cases.push(verify(
        install,
        &scene,
        &mut fill_doc,
        &ring,
        out,
        "height-fill-before",
        true,
    )?);
    let top = height_tools::sample(&fill_doc, &scene, ring.cliffs[0].origin)?;
    ensure!(top == 8, "cliff top sample mismatch");
    let filled = height_tools::fill(&fill_doc, &scene, [67, 67], top)?;
    ensure!(
        filled.interior == ring.interior,
        "fill crossed the original cliff boundary"
    );
    cases.push(verify(
        install,
        &scene,
        &mut fill_doc,
        &filled,
        out,
        "height-fill-after",
        true,
    )?);
    for (&at, &v) in &ring.edits {
        if !ring.interior.contains(&at) {
            ensure!(
                fill_doc.terrain(at) == Some(v),
                "fill changed an existing cliff"
            );
        }
    }
    let stamp = ring
        .cliffs
        .iter()
        .find(|s| {
            let t = &scene.palette.templates[&(s.tile, 0)];
            t.cells.iter().flatten().any(|s| s.height == 0)
                && t.cells.iter().flatten().any(|s| s.height == 4)
        })
        .context("no cliff template with both high and low subtiles")?;
    let template = &scene.palette.templates[&(stamp.tile, 0)];
    let selected = template
        .cells
        .iter()
        .position(Option::is_some)
        .context("empty cliff template")?;
    let selected = [
        stamp.origin[0] + selected as u16 % template.width,
        stamp.origin[1] + selected as u16 / template.width,
    ];
    let changed = ra2_map_scene::terrain_tools::height(
        &mut fill_doc,
        &scene,
        &std::collections::BTreeSet::from([selected]),
        ra2_map_scene::terrain_tools::TerrainMode::Raise,
        0,
        false,
    )?;
    ensure!(
        changed.len() == template.cells.iter().flatten().count() && changed.len() > 1,
        "cliff raise did not include the whole template"
    );
    let moved = PlateauPlan {
        edits: changed
            .into_iter()
            .map(|at| (at, fill_doc.terrain(at).unwrap()))
            .collect(),
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior: Default::default(),
        base: 5,
    };
    fill_doc.cancel();
    cases.push(verify(
        install,
        &scene,
        &mut fill_doc,
        &moved,
        out,
        "height-cliff-raised",
        true,
    )?);
    for (name, mode, change, smooth, cells) in [
        (
            "height-custom-smooth-step",
            ra2_map_scene::terrain_tools::TerrainMode::Raise,
            ra2_map_scene::terrain_tools::HeightChange::Step(4),
            true,
            cliff_tools::rectangle([60, 60], [64, 64]),
        ),
        (
            "height-custom-smooth-target",
            ra2_map_scene::terrain_tools::TerrainMode::Lower,
            ra2_map_scene::terrain_tools::HeightChange::To(2),
            true,
            cliff_tools::rectangle([60, 60], [64, 64]),
        ),
        (
            "height-custom-cliff-step",
            ra2_map_scene::terrain_tools::TerrainMode::Raise,
            ra2_map_scene::terrain_tools::HeightChange::Step(4),
            false,
            std::collections::BTreeSet::from([selected]),
        ),
        (
            "height-custom-cliff-target",
            ra2_map_scene::terrain_tools::TerrainMode::Raise,
            ra2_map_scene::terrain_tools::HeightChange::To(12),
            false,
            std::collections::BTreeSet::from([selected]),
        ),
    ] {
        let mut custom_doc = document(&scene);
        if !smooth {
            ring.apply(&mut custom_doc)?;
            custom_doc.commit("cliff source");
        }
        let before = custom_doc.fork();
        let mut changed: std::collections::BTreeSet<_> = if smooth {
            let target = change.target(mode, 4);
            flatten_tools::plan(&custom_doc, &scene, &cells, target, true)?
                .apply(&mut custom_doc)?
        } else {
            ra2_map_scene::terrain_tools::height_with_change(
                &mut custom_doc,
                &scene,
                &cells,
                mode,
                0,
                smooth,
                change,
            )?
        }
        .into_iter()
        .collect();
        changed.extend(ra2_map_scene::terrain_tools::recalculate(
            &mut custom_doc,
            &scene,
            &changed,
        )?);
        for &at in &cells {
            let expected = change.target(
                mode,
                ra2_map_scene::terrain_tools::value(&before, &scene, at).level,
            );
            if smooth {
                ensure!(
                    ra2_map_scene::terrain_tools::value(&custom_doc, &scene, at).level == expected,
                    "custom smooth target mismatch"
                );
            } else {
                ensure!(
                    height_tools::sample(&custom_doc, &scene, at)? == 12,
                    "custom cliff top mismatch"
                );
            }
        }
        let plan = PlateauPlan {
            edits: changed
                .into_iter()
                .map(|at| {
                    (
                        at,
                        ra2_map_scene::terrain_tools::value(&custom_doc, &scene, at),
                    )
                })
                .collect(),
            overlays: BTreeMap::new(),
            cliffs: vec![],
            interior: cells,
            base: 0,
        };
        custom_doc.cancel();
        cases.push(verify(
            install,
            &scene,
            &mut custom_doc,
            &plan,
            out,
            name,
            true,
        )?);
    }
    for (name, options, cliff_source) in [
        (
            "fa2-common-steep",
            flatten_tools::Options {
                steep: true,
                ..Default::default()
            },
            false,
        ),
        (
            "fa2-common-vertex",
            flatten_tools::Options {
                vertices: true,
                ..Default::default()
            },
            false,
        ),
        (
            "fa2-common-force",
            flatten_tools::Options {
                force: true,
                ..Default::default()
            },
            true,
        ),
    ] {
        let mut doc = document(&scene);
        if cliff_source {
            ring.apply(&mut doc)?;
            doc.commit("cliff source");
        }
        let cells =
            std::collections::BTreeSet::from([if cliff_source { selected } else { [65, 65] }]);
        let initial = flatten_tools::plan_with_options(&doc, &scene, &cells, 8, true, options)?;
        ensure!(!initial.edits.is_empty(), "{name}: no terrain changed");
        let plan = with_lat(&doc, &scene, initial)?;
        cases.push(verify(install, &scene, &mut doc, &plan, out, name, true)?);
        if options.vertices {
            ensure!(
                flatten_tools::vertex_height(&doc, &scene, [65, 65], false)? == Some(8),
                "vertex target changed after LAT"
            );
        } else {
            ensure!(
                ra2_map_scene::terrain_tools::value(&doc, &scene, *cells.first().unwrap()).level
                    == 8,
                "height target changed after LAT"
            );
        }
    }
    let mut connected_doc = document(&scene);
    ring.apply(&mut connected_doc)?;
    connected_doc.commit("cliff group");
    let connected = height_tools::cliff_selection(&connected_doc, &scene, selected, true)?
        .context("no cliff group")?;
    ensure!(
        connected.len() > template.cells.iter().flatten().count(),
        "Ctrl did not expand beyond one template"
    );
    let changed = ra2_map_scene::terrain_tools::height_with_change(
        &mut connected_doc,
        &scene,
        &connected,
        ra2_map_scene::terrain_tools::TerrainMode::Raise,
        0,
        false,
        ra2_map_scene::terrain_tools::HeightChange::Step(1),
    )?;
    let plan = PlateauPlan {
        edits: changed
            .into_iter()
            .map(|at| {
                (
                    at,
                    ra2_map_scene::terrain_tools::value(&connected_doc, &scene, at),
                )
            })
            .collect(),
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior: connected,
        base: 5,
    };
    connected_doc.cancel();
    let plan = with_lat(&connected_doc, &scene, plan)?;
    cases.push(verify(
        install,
        &scene,
        &mut connected_doc,
        &plan,
        out,
        "fa2-common-ctrl-cliff-group",
        true,
    )?);
    let mut fixed_doc = document(&scene);
    let brush = scene
        .brushes
        .iter()
        .find(|b| b.key.tile_id == stamp.tile && b.key.sub_tile == 0 && b.key.variant == 0)
        .context("cliff brush absent")?;
    let at = [50, 50];
    let v = ra2_map_scene::terrain_tools::subtile_placement_at_height(
        &fixed_doc,
        &scene,
        brush,
        at,
        Some(7),
    )?;
    let plan = PlateauPlan {
        edits: BTreeMap::from([(at, v)]),
        overlays: BTreeMap::new(),
        cliffs: vec![],
        interior: std::collections::BTreeSet::from([at]),
        base: 7,
    };
    cases.push(verify(
        install,
        &scene,
        &mut fixed_doc,
        &plan,
        out,
        "fa2-common-subtile-fixed-base",
        true,
    )?);
    let ids = scene.palette.tile_ids.context("tile ids absent")?;
    if let (Some(rough), Some(clear)) = (
        scene.brushes.iter().find(|b| {
            i32::from(b.key.tile_id) == ids.rough && b.key.sub_tile == 0 && b.key.variant == 0
        }),
        scene.brushes.iter().find(|b| {
            i32::from(b.key.tile_id) == ids.clear && b.key.sub_tile == 0 && b.key.variant == 0
        }),
    ) {
        let mut flood_doc = document(&scene);
        ra2_map_scene::fill_tools::plan(
            &flood_doc,
            &scene,
            rough,
            cliff_tools::rectangle([60, 60], [64, 64]),
            true,
        )?
        .apply(&mut flood_doc)?;
        flood_doc.commit("rough group with LAT");
        let cells = ra2_map_scene::fill_tools::connected(&flood_doc, &scene, [62, 62])?;
        ensure!(cells.len() >= 25, "Ctrl flood dropped source cells");
        let p = ra2_map_scene::fill_tools::plan(&flood_doc, &scene, clear, cells, true)?;
        let plan = PlateauPlan {
            edits: p.edits,
            overlays: BTreeMap::new(),
            cliffs: vec![],
            interior: p.cells,
            base: 4,
        };
        cases.push(verify(
            install,
            &scene,
            &mut flood_doc,
            &plan,
            out,
            "fa2-common-ctrl-ground-fill",
            true,
        )?);
    }
    let mut manual_doc = document(&scene);
    let mut manual_ring = cliff_tools::plan(
        &manual_doc,
        &scene,
        cliff_tools::rectangle([60, 60], [74, 74]),
        4,
    )?;
    for value in manual_ring.edits.values_mut() {
        value.level = 4;
    }
    cases.push(verify(
        install,
        &scene,
        &mut manual_doc,
        &manual_ring,
        out,
        "subtile-cliff-before-repair",
        true,
    )?);
    let repaired = height_tools::repair(
        &manual_doc,
        &scene,
        &cliff_tools::rectangle([55, 55], [80, 80]),
        4,
    )?;
    ensure!(
        !repaired.edits.is_empty(),
        "repair did not restore cliff heights"
    );
    cases.push(verify(
        install,
        &scene,
        &mut manual_doc,
        &repaired,
        out,
        "subtile-cliff-repaired",
        true,
    )?);
    let filled = height_tools::fill(&manual_doc, &scene, [67, 67], 8)?;
    cases.push(verify(
        install,
        &scene,
        &mut manual_doc,
        &filled,
        out,
        "subtile-cliff-filled",
        true,
    )?);
    let mut brush_doc = document(&scene);
    ring.apply(&mut brush_doc)?;
    brush_doc.commit("unraised interior");
    let source = *ring
        .edits
        .iter()
        .find(|(at, v)| !ring.interior.contains(*at) && v.level == 8)
        .context("cliff top absent")?
        .0;
    let mut selected = std::collections::BTreeSet::new();
    for center in ra2_map_document::document::cell_line(source, [67, 67]) {
        selected.extend(flatten_tools::window(&brush_doc, &scene, center, 3, 8)?);
    }
    let mut brush_plan = flatten_tools::plan(&brush_doc, &scene, &selected, 8, true)?;
    let mut scratch = brush_doc.fork();
    let mut changed: std::collections::BTreeSet<_> =
        brush_plan.apply(&mut scratch)?.into_iter().collect();
    changed.extend(ra2_map_scene::terrain_tools::recalculate(
        &mut scratch,
        &scene,
        &changed,
    )?);
    brush_plan.edits = changed
        .into_iter()
        .map(|at| {
            (
                at,
                ra2_map_scene::terrain_tools::value(&scratch, &scene, at),
            )
        })
        .collect();
    ensure!(
        brush_plan
            .edits
            .get(&[67, 67])
            .is_some_and(|v| v.level == 8),
        "drag brush failed to flatten the interior"
    );
    cases.push(verify(
        install,
        &scene,
        &mut brush_doc,
        &brush_plan,
        out,
        "fa2-flatten-drag-from-cliff",
        true,
    )?);
    for (&at, &v) in &ring.edits {
        if !ring.interior.contains(&at) {
            ensure!(
                brush_doc.terrain(at) == Some(v),
                "flatten brush changed a cliff"
            );
        }
    }
    // Finish brushing the whole interior, rather than treating one narrow
    // stroke as evidence that the entire platform meets the surrounding rim.
    for &center in &ring.interior {
        selected.extend(flatten_tools::window(&brush_doc, &scene, center, 3, 8)?);
    }
    let mut complete = flatten_tools::plan(&brush_doc, &scene, &selected, 8, true)?;
    let mut scratch = brush_doc.fork();
    let mut changed: std::collections::BTreeSet<_> =
        complete.apply(&mut scratch)?.into_iter().collect();
    changed.extend(ra2_map_scene::terrain_tools::recalculate(
        &mut scratch,
        &scene,
        &changed,
    )?);
    complete.edits = changed
        .into_iter()
        .map(|at| {
            (
                at,
                ra2_map_scene::terrain_tools::value(&scratch, &scene, at),
            )
        })
        .collect();
    for &at in &ring.interior {
        let v = ra2_map_scene::terrain_tools::value(&scratch, &scene, at);
        ensure!(
            v.level == 8,
            "complete brush left an unraised cell at {at:?}"
        );
        let tile = scene.palette.raw_ids[&v.tile];
        ensure!(
            scene.palette.templates[&(tile, 0)].cells[usize::from(v.sub)]
                .as_ref()
                .is_some_and(|s| s.slope == 0),
            "complete brush left a slope at {at:?}"
        );
    }
    cases.push(verify(
        install,
        &scene,
        &mut brush_doc,
        &complete,
        out,
        "fa2-flatten-complete-interior",
        true,
    )?);
    for (&at, &v) in &ring.edits {
        if !ring.interior.contains(&at) {
            ensure!(
                brush_doc.terrain(at) == Some(v),
                "complete brush changed a cliff"
            );
        }
    }
    if height_only {
        let mut census = Vec::new();
        for (&tile, group) in &scene.palette.groups {
            if group.category != ra2_map_assets::terrain_library::TerrainCategory::Cliff {
                continue;
            }
            let Some(t) = scene.palette.templates.get(&(tile, 0)) else {
                continue;
            };
            census.push(serde_json::json!({"tile":tile,"heights":t.cells.iter().flatten().map(|s| s.height).collect::<Vec<_>>() }));
        }
        std::fs::write(
            out.join("cliff-height-census.json"),
            serde_json::to_vec_pretty(&census)?,
        )?;
        return Ok(serde_json::json!({"theater":theater,"cases":cases,
            "standard_cliff_templates_checked":standard_templates,"standard_cliff_subtiles_checked":standard_subtiles}));
    }
    for back in [false, true] {
        for (dx, dy) in [
            (12, 0),
            (0, 12),
            (-12, 0),
            (0, -12),
            (12, 12),
            (-12, 12),
            (12, -12),
            (-12, -12),
        ] {
            let p = cliff_authoring::line(
                &doc,
                &scene,
                [75, 75],
                [(75 + dx) as u16, (75 + dy) as u16],
                4,
                back,
            )?;
            cases.push(verify(
                install,
                &scene,
                &mut doc,
                &p,
                out,
                &format!("line-{dx}-{dy}-{back}"),
                dx == 12 && dy == 0,
            )?);
            doc.undo();
        }
    }
    let p = cliff_authoring::line(&doc, &scene, [65, 70], [75, 70], 4, false)?;
    p.apply(&mut doc)?;
    doc.commit("line one");
    let p = cliff_authoring::line(&doc, &scene, [75, 70], [85, 70], 4, false)?;
    cases.push(verify(
        install,
        &scene,
        &mut doc,
        &p,
        out,
        "line-continued",
        true,
    )?);
    doc.undo();
    doc.undo();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([60, 60], [74, 74]), 4)?
        .apply(&mut doc)?;
    doc.commit("plateau");
    let mut ramp_doc = document(&scene);
    cliff_tools::plan(
        &ramp_doc,
        &scene,
        cliff_tools::rectangle([50, 50], [84, 84]),
        4,
    )?
    .apply(&mut ramp_doc)?;
    ramp_doc.commit("wide ramp bank");
    for (n, (start, end)) in [
        ([67, 84], [67, 89]),
        ([84, 67], [89, 67]),
        ([67, 50], [67, 45]),
        ([50, 67], [45, 67]),
    ]
    .into_iter()
    .enumerate()
    {
        let p = cliff_authoring::ramp(&ramp_doc, &scene, start, end, 4)?;
        cases.push(verify(
            install,
            &scene,
            &mut ramp_doc,
            &p,
            out,
            &format!("ramp-{n}"),
            true,
        )?);
        ramp_doc.undo();
        let axis = if start[0] == end[0] { 0 } else { 1 };
        for delta in [-4, -2, 2, 4] {
            let mut wide_end = start;
            wide_end[axis] = (i32::from(start[axis]) + delta) as u16;
            let wide = cliff_authoring::ramp(&ramp_doc, &scene, start, wide_end, 4)?;
            ensure!(
                wide.edits.len() == p.edits.len() + delta.unsigned_abs() as usize * 4,
                "wide ramp must fill the full dragged width"
            );
            cases.push(verify(
                install,
                &scene,
                &mut ramp_doc,
                &wide,
                out,
                &format!("ramp-{n}-wide-{delta}"),
                true,
            )?);
            ramp_doc.undo();
        }
    }
    for (n, (corner, signs)) in [
        ([50, 50], [1, 1]),
        ([84, 50], [-1, 1]),
        ([50, 84], [1, -1]),
        ([84, 84], [-1, -1]),
    ]
    .into_iter()
    .enumerate()
    {
        for axis in 0..3 {
            let mut end = corner;
            if axis < 2 {
                end[axis] = (i32::from(end[axis]) + signs[axis] * 4) as u16;
            }
            let p = cliff_authoring::ramp(&ramp_doc, &scene, corner, end, 4)
                .with_context(|| format!("corner {corner:?} -> {end:?}"))?;
            let v = p
                .edits
                .get(&corner)
                .context("corner itself was not converted")?;
            let id = scene.palette.raw_ids[&v.tile];
            let shape = scene.palette.templates[&(id, 0)].cells[usize::from(v.sub)].unwrap();
            ensure!(
                (5..=8).contains(&shape.slope) && v.level == 7,
                "corner must become a descending slope"
            );
            cases.push(verify(
                install,
                &scene,
                &mut ramp_doc,
                &p,
                out,
                &format!("ramp-corner-{n}-width-{axis}"),
                true,
            )?);
            ramp_doc.undo();
        }
    }
    for (n, (corner, inward)) in [
        ([50, 50], [1, 1]),
        ([84, 50], [-1, 1]),
        ([50, 84], [1, -1]),
        ([84, 84], [-1, -1]),
    ]
    .into_iter()
    .enumerate()
    {
        for axis in 0..2 {
            for corner_first in [true, false] {
                let mut start = corner;
                start[axis] = (i32::from(start[axis]) + inward[axis] * 10) as u16;
                let mut end = start;
                end[axis] = (i32::from(start[axis]) - inward[axis] * 6) as u16;
                let (a, b, c, d) = if corner_first {
                    (corner, corner, start, end)
                } else {
                    (start, end, corner, corner)
                };
                cliff_authoring::ramp(&ramp_doc, &scene, a, b, 4)?.apply(&mut ramp_doc)?;
                ramp_doc.commit("first ramp");
                // Recover the second operation from a saved map, without any
                // editor-only geometry retained from the first operation.
                let bytes = ramp_doc.snapshot().bytes()?;
                let (source, _) = ra2_map_document::source::SourceMap::parse(
                    "join.map".into(),
                    "audit".into(),
                    bytes,
                )?;
                let mut reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
                let p = cliff_authoring::ramp(&reopened, &scene, c, d, 4).with_context(|| {
                    format!("join corner {corner:?}, axis {axis}, corner_first {corner_first}")
                })?;
                cases.push(verify(
                    install,
                    &scene,
                    &mut reopened,
                    &p,
                    out,
                    &format!("ramp-joined-{n}-{axis}-{corner_first}"),
                    true,
                )?);
                ramp_doc.undo();
            }
        }
    }
    let p = cliff_authoring::reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([70, 60], [85, 74]),
        4,
        false,
    )?;
    cases.push(verify(
        install,
        &scene,
        &mut doc,
        &p,
        out,
        "plateau-expanded",
        true,
    )?);
    let p = cliff_authoring::reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([79, 57], [89, 77]),
        4,
        true,
    )?;
    cases.push(verify(
        install,
        &scene,
        &mut doc,
        &p,
        out,
        "plateau-cut",
        true,
    )?);
    for (a, b, region) in [
        (
            [80, 67],
            [94, 67],
            cliff_tools::rectangle([94, 60], [108, 74]),
        ),
        (
            [67, 75],
            [67, 89],
            cliff_tools::rectangle([60, 89], [74, 103]),
        ),
    ] {
        let mut doc = document(&scene);
        cliff_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle([60, 60], if a[0] == 80 { [79, 74] } else { [74, 74] }),
            4,
        )?
        .apply(&mut doc)?;
        doc.commit("bank one");
        cliff_tools::plan(&doc, &scene, region, 4)?.apply(&mut doc)?;
        doc.commit("bank two");
        for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
            let p = bridge_tools::plan(&doc, &scene, a, b, kind)?;
            cases.push(verify(
                install,
                &scene,
                &mut doc,
                &p,
                out,
                &format!("{kind:?}-{}", a[0]),
                true,
            )?);
            doc.undo();
        }
    }
    for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
        for b in [[80, 60], [60, 80]] {
            let mut doc = document(&scene);
            let p = bridge_tools::plan(&doc, &scene, [60, 60], b, kind)?;
            cases.push(verify(
                install,
                &scene,
                &mut doc,
                &p,
                out,
                &format!("{kind:?}-{}", b[0]),
                true,
            )?);
        }
    }
    Ok(serde_json::json!({"theater":theater,"cases":cases,
        "standard_cliff_templates_checked":standard_templates,"standard_cliff_subtiles_checked":standard_subtiles}))
}
fn audit_polyline(install: &mut Install, theater: &str, out: &Path) -> Result<serde_json::Value> {
    std::fs::create_dir_all(out)?;
    let scene = load_scene_bytes(
        install,
        "polyline.map",
        "audit".into(),
        NewMapOptions {
            theater: theater.into(),
            width: 80,
            height: 80,
            level: 4,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let closed = [[50, 50], [78, 50], [78, 78], [50, 78], [50, 50]];
    if !scene
        .palette
        .tile_ids
        .is_some_and(|ids| ids.cliffs.cliff_set.is_some())
    {
        for path in [&closed[..2], &closed[..]] {
            ensure!(
                cliff_authoring::polyline(&document(&scene), &scene, path, 4, true).is_err(),
                "unsupported theater accepted cliffs"
            );
        }
        return Ok(serde_json::json!({"theater":theater,"unsupported_cliffs_rejected":true}));
    }
    let mut cases = Vec::new();
    for high_left in [false, true] {
        for (i, (dx, dy)) in [
            (20, 0),
            (0, 20),
            (-20, 0),
            (0, -20),
            (20, 20),
            (-20, 20),
            (20, -20),
            (-20, -20),
        ]
        .into_iter()
        .enumerate()
        {
            let path = [[80, 80], [(80 + dx) as u16, (80 + dy) as u16]];
            let mut doc = document(&scene);
            let before = doc.snapshot().bytes()?;
            let p = cliff_authoring::polyline(&doc, &scene, &path, 4, high_left)
                .with_context(|| format!("open-{i}-left-{high_left}"))?;
            ensure!(
                doc.snapshot().bytes()? == before && p.interior.is_empty(),
                "open preview changed the map or filled ground"
            );
            cases.push(verify(
                install,
                &scene,
                &mut doc,
                &p,
                out,
                &format!("open-{i}-left-{high_left}"),
                true,
            )?);
        }
        let mut doc = document(&scene);
        let p = cliff_authoring::polyline(&doc, &scene, &closed[..4], 4, high_left)?;
        cases.push(verify(
            install,
            &scene,
            &mut doc,
            &p,
            out,
            &format!("open-turns-left-{high_left}"),
            true,
        )?);
        for (kind, vertices, back) in [
            ("axis", [[60, 80], [80, 80], [80, 60]], [60, 60]),
            ("oblique", [[60, 58], [80, 80], [84, 60]], [64, 38]),
        ] {
            for rotation in 0..4 {
                let rotate = |at: [u16; 2]| {
                    let (mut x, mut y) = (i32::from(at[0]) - 80, i32::from(at[1]) - 80);
                    for _ in 0..rotation {
                        (x, y) = (-y, x);
                    }
                    [(80 + x) as u16, (80 + y) as u16]
                };
                let path = vertices.map(rotate);
                let reference_path = [path[0], path[1], path[2], rotate(back), path[0]];
                let mut doc = document(&scene);
                let reference =
                    cliff_authoring::polyline(&doc, &scene, &reference_path, 4, high_left)?;
                let p = cliff_authoring::polyline(&doc, &scene, &path, 4, high_left)?;
                let corners: Vec<_> = reference
                    .cliffs
                    .iter()
                    .filter(|s| {
                        let dx = i32::from(s.origin[0]) - 80;
                        let dy = i32::from(s.origin[1]) - 80;
                        dx * dx + dy * dy <= 16
                    })
                    .collect();
                ensure!(!corners.is_empty(), "missing native reference corner");
                for corner in corners {
                    ensure!(
                        p.cliffs
                            .iter()
                            .any(|s| s.origin == corner.origin && s.tile == corner.tile),
                        "open joint omitted native corner: rotation={rotation}, left={high_left}, origin={:?}",
                        corner.origin
                    );
                }
                let mut result = verify(
                    install,
                    &scene,
                    &mut doc,
                    &p,
                    out,
                    &format!("open-joint-{kind}-{rotation}-left-{high_left}"),
                    true,
                )?;
                result["native_joint_checked"] = true.into();
                result["joint_kind"] = kind.into();
                cases.push(result);
            }
        }
        for reversed in [false, true] {
            let mut path = closed;
            if reversed {
                path.reverse();
            }
            let mut doc = document(&scene);
            let at = [64, 64];
            doc.set(
                ra2_map_document::document::Target::Terrain(at),
                Some(ra2_map_document::document::Value::Terrain(
                    ra2_map_document::document::TerrainValue {
                        level: 12,
                        ..ra2_map_scene::terrain_tools::value(&doc, &scene, at)
                    },
                )),
            )?;
            doc.commit("uneven ground");
            let p = cliff_authoring::polyline(&doc, &scene, &path, 4, high_left)?;
            let expected_level = if cliff_authoring::polyline_is_pit(&path, high_left) {
                0
            } else {
                8
            };
            ensure!(
                !p.interior.is_empty()
                    && p.interior
                        .iter()
                        .all(|at| p.edits[at].level == expected_level),
                "closed interior not level"
            );
            ensure!(
                !p.edits.contains_key(&[40, 40]),
                "closed contour changed exterior ground"
            );
            cases.push(verify(
                install,
                &scene,
                &mut doc,
                &p,
                out,
                &format!("closed-reversed-{reversed}-left-{high_left}"),
                true,
            )?);
        }
        let mut doc = document(&scene);
        let p = cliff_authoring::polyline(&doc, &scene, &[[24, 79], [24, 83]], 4, high_left)?;
        cases.push(verify(
            install,
            &scene,
            &mut doc,
            &p,
            out,
            &format!("open-grid-edge-left-{high_left}"),
            true,
        )?);
    }
    Ok(serde_json::json!({"theater":theater,"cases":cases}))
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let out = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/authoring-audit".into()),
    );
    std::fs::create_dir_all(&out)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    let filter = args
        .next()
        .map(|s| s.to_string_lossy().to_uppercase())
        .filter(|s| s != "ALL");
    let scope = args.next();
    ensure!(
        scope
            .as_ref()
            .is_none_or(|s| s == "--height-only" || s == "--polyline-only"),
        "unknown audit scope"
    );
    let height_only = scope.as_ref().is_some_and(|s| s == "--height-only");
    let polyline_only = scope.as_ref().is_some_and(|s| s == "--polyline-only");
    for (theater, _) in THEATERS {
        if filter.as_ref().is_some_and(|f| f != theater) {
            continue;
        }
        println!("Auditing {theater}...");
        reports.push(
            (if polyline_only {
                audit_polyline(&mut install, theater, &out.join(theater))
            } else {
                audit(&mut install, theater, &out.join(theater), height_only)
            })
            .with_context(|| theater.to_owned())?,
        );
    }
    ensure!(!reports.is_empty(), "unknown theater filter");
    let report = serde_json::json!({"passed":true,"theaters":reports,"scope":"Actual TMP/SHP resources, saved map and engine terrain reload, undo/redo, CPU static previews. No in-game movement or bridge destruction validation."});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "All terrain authoring cases passed; report: {}",
        out.join("report.json").display()
    );
    Ok(())
}
