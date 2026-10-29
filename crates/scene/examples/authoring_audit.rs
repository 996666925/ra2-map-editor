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
    map::{MapScene, load_scene_bytes},
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
    for (n, (start, end)) in [
        ([67, 74], [67, 79]),
        ([74, 67], [79, 67]),
        ([67, 60], [67, 55]),
        ([60, 67], [55, 67]),
    ]
    .into_iter()
    .enumerate()
    {
        let p = cliff_authoring::ramp(&doc, &scene, start, end, 4)?;
        cases.push(verify(
            install,
            &scene,
            &mut doc,
            &p,
            out,
            &format!("ramp-{n}"),
            true,
        )?);
        doc.undo();
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
        "All terrain authoring cases passed; report: {}",
        out.join("report.json").display()
    );
    Ok(())
}
