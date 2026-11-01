//! Actual-theater authoring, engine terrain reloads and reviewable map/PNGs.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{MapDocument, Target, TerrainValue, Value},
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{
    bridge_tools::{self, BridgeKind},
    cliff_tools::{self, PlateauPlan},
    coastal_cliffs,
    map::{MapScene, load_scene_bytes},
    road_tools, shore_tools, terrain_tools,
    visual::ViewOptions,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};
fn document(scene: &MapScene) -> MapDocument {
    MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone())
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
    }
    Ok(
        serde_json::json!({"case":name,"terrain_cells":p.edits.len(),"overlay_cells":p.overlays.len(),"engine_reload":true,"undo_redo":true}),
    )
}
fn audit(install: &mut Install, theater: &str, out: &Path) -> Result<serde_json::Value> {
    std::fs::create_dir_all(out)?;
    let scene = load_scene_bytes(
        install,
        "joining.map",
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
    let mut doc = document(&scene);
    let ids = scene.palette.tile_ids.unwrap();
    if ids.water_base < 0 || ids.shore < 0 {
        ensure!(
            shore_tools::plan(
                &doc,
                &scene,
                cliff_tools::rectangle([60, 60], [72, 72]),
                Some(true)
            )
            .is_err(),
            "missing shore accepted"
        );
        return Ok(serde_json::json!({"theater":theater,"unsupported_shores_rejected":true}));
    }
    let mut cases = Vec::new();
    for (name, a, b, paint) in [
        ("lake", [60, 60], [72, 72], Some(true)),
        ("extend", [68, 65], [80, 78], Some(true)),
        ("fill-cut", [70, 70], [78, 76], Some(false)),
        ("island", [64, 64], [66, 66], Some(false)),
        ("repair", [59, 59], [81, 79], None),
        ("fill-all", [55, 55], [85, 85], Some(false)),
    ] {
        let p = shore_tools::plan(&doc, &scene, cliff_tools::rectangle(a, b), paint)
            .with_context(|| name)?;
        if name == "lake" && ids.green >= 0 && ids.green_lat >= 0 {
            for at in [
                [58, 66],
                [74, 66],
                [66, 58],
                [66, 74],
                [58, 58],
                [74, 58],
                [74, 74],
                [58, 74],
            ] {
                let v = p.edits.get(&at).context("missing outer shore transition")?;
                let tile = i32::from(scene.palette.raw_ids[&v.tile]);
                ensure!(
                    (ids.green_lat..ids.green_lat + 16).contains(&tile),
                    "{theater}: missing ground LAT at {at:?}"
                );
            }
        }
        if p.edits.is_empty() {
            cases.push(serde_json::json!({"case":name,"idempotent":true}));
            continue;
        }
        cases.push(verify(install, &scene, &mut doc, &p, out, name, true)?);
    }
    if let (Some(_), Some(water_base)) = (ids.cliffs.cliff_set, ids.cliffs.water_cliffs) {
        let mut d = document(&scene);
        cliff_tools::plan(&d, &scene, cliff_tools::rectangle([55, 55], [85, 70]), 4)?
            .apply(&mut d)?;
        d.commit("plateau by coast");
        let p = shore_tools::plan(
            &d,
            &scene,
            cliff_tools::rectangle([60, 73], [80, 85]),
            Some(true),
        )
        .context("coastal water")?;

        if ra2_map_scene::coastal_cliffs::supports(&scene, 7) {
            ensure!(
                p.cliffs
                    .iter()
                    .any(|s| (water_base..water_base + 22).contains(&s.tile)),
                "water failed to adapt cliffs"
            );
        }
        cases.push(verify(
            install,
            &scene,
            &mut d,
            &p,
            out,
            "coastal-cliffs",
            true,
        )?);
        let p = shore_tools::plan(
            &d,
            &scene,
            cliff_tools::rectangle([58, 73], [82, 87]),
            Some(false),
        )
        .context("coast refill")?;
        cases.push(verify(
            install,
            &scene,
            &mut d,
            &p,
            out,
            "coastal-refill",
            true,
        )?);
    }
    if let Some(land) = ids.cliffs.cliff_set {
        let mut d = document(&scene);
        let mut origins = std::collections::BTreeSet::new();
        for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
            let p = cliff_tools::plan(&d, &scene, cliff_tools::rectangle(a, b), 4)?;
            origins.extend(
                p.cliffs
                    .iter()
                    .filter(|s| s.tile < land + 22)
                    .map(|s| s.origin),
            );
            p.apply(&mut d)?;
            d.commit("bank");
        }
        let water = (ids.water_base..ids.water_base + 14)
            .find(|&t| {
                scene
                    .palette
                    .templates
                    .get(&(t as u16, 0))
                    .is_some_and(|t| {
                        t.width == 1 && t.height == 1 && t.cells[0].is_some_and(|s| s.terrain == 9)
                    })
            })
            .context("water template absent")? as u16;
        for at in cliff_tools::rectangle([80, 60], [93, 74]) {
            let old = terrain_tools::value(&d, &scene, at);
            if old.level == 4 && scene.palette.raw_ids.get(&old.tile) == Some(&(ids.clear as u16)) {
                d.set(
                    Target::Terrain(at),
                    Some(Value::Terrain(TerrainValue {
                        tile: scene.catalog.raw_tiles[&water],
                        sub: 0,
                        ..old
                    })),
                )?;
            }
        }
        d.commit("water channel");
        if coastal_cliffs::supports(&scene, 7) {
            coastal_cliffs::plan(&d, &scene, origins)?.apply(&mut d)?;
            d.commit("water cliffs");
        }
        let p = bridge_tools::plan(&d, &scene, [80, 67], [94, 67], BridgeKind::HighConcrete)?;
        cases.push(verify(
            install,
            &scene,
            &mut d,
            &p,
            out,
            "water-cliff-bridge",
            true,
        )?);
    }
    if ids.paved_roads >= 0 && ids.paved_road_ends >= 0 {
        let mut d = document(&scene);
        let p = road_tools::plan(&d, &scene, [70, 55], [70, 85], false)?;
        cases.push(verify(
            install,
            &scene,
            &mut d,
            &p,
            out,
            "road-straight-y",
            true,
        )?);
        for (n, (a, b, c)) in [
            ([60, 60], [80, 60], [80, 80]),
            ([80, 60], [60, 60], [60, 80]),
            ([60, 80], [80, 80], [80, 60]),
            ([80, 80], [60, 80], [60, 60]),
        ]
        .into_iter()
        .enumerate()
        {
            let mut d = document(&scene);
            road_tools::plan(&d, &scene, a, b, false)?.apply(&mut d)?;
            d.commit("first road");
            let p =
                road_tools::plan(&d, &scene, b, c, false).with_context(|| format!("curve {n}"))?;
            cases.push(verify(
                install,
                &scene,
                &mut d,
                &p,
                out,
                &format!("road-curve-{n}"),
                true,
            )?);
        }
        let mut d = document(&scene);
        for (name, a, b, cut) in [
            ("road-straight", [55, 70], [85, 70], false),
            ("road-T", [70, 70], [70, 90], false),
            ("road-cross", [70, 70], [70, 50], false),
            ("road-cut", [66, 70], [74, 70], true),
        ] {
            let p = road_tools::plan(&d, &scene, a, b, cut).with_context(|| name)?;
            cases.push(verify(install, &scene, &mut d, &p, out, name, true)?);
        }
    }
    Ok(serde_json::json!({"theater":theater,"cases":cases}))
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/joining-audit".into()));
    std::fs::create_dir_all(&out)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    let filter = args.next().map(|s| s.to_string_lossy().to_uppercase());
    for (theater, _) in THEATERS {
        if filter.as_ref().is_some_and(|f| f != theater) {
            continue;
        }
        println!("Auditing {theater}...");
        reports.push(
            audit(&mut install, theater, &out.join(theater)).with_context(|| theater.to_owned())?,
        );
    }
    ensure!(!reports.is_empty(), "unknown theater filter");
    let report = serde_json::json!({"passed":true,"theaters":reports,"scope":"Actual TMP/SHP resources, saved map and engine terrain reload, undo/redo, CPU static previews. No in-game movement or bridge destruction validation."});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "All joining cases passed; report: {}",
        out.join("report.json").display()
    );
    Ok(())
}
