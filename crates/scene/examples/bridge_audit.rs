//! Installed-asset regression audit: cliff, coastal and river bridges.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Cell, MapDocument, Target, TerrainValue, Value},
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{
    bridge_tools::{self, BridgeKind},
    cliff_tools::{self, PlateauPlan},
    coastal_cliffs,
    map::{MapScene, load_scene_bytes},
    shore_tools,
    terrain_tools::value,
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
    install.activate_theater(theater)?;
    let scene = load_scene_bytes(
        install,
        "bridge.map",
        "bridge audit".into(),
        NewMapOptions {
            theater: theater.into(),
            width: 80,
            height: 80,
            level: 0,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let ids = scene.palette.tile_ids.context("tile configuration")?;
    let mut cases = Vec::new();
    if ids.cliffs.cliff_set.is_none() {
        let doc = document(&scene);
        ensure!(
            bridge_tools::plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighConcrete).is_err(),
            "unsupported high bridge accepted"
        );
        return Ok(serde_json::json!({"theater":theater,"unsupported_cliffs_rejected":true}));
    }
    let water = (ids.water_base..ids.water_base + 14).find_map(|id| {
        if id < 0 {
            return None;
        }
        scene
            .palette
            .templates
            .get(&(id as u16, 0))
            .filter(|t| t.width == 1 && t.height == 1 && t.cells[0].is_some_and(|s| s.terrain == 9))
            .map(|_| id as u16)
    });
    for axis in 0..2 {
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        for span in 3..8 {
            let mut doc = document(&scene);
            for (a, b) in [([60, 60], [79, 74]), ([80 + span, 60], [98 + span, 74])] {
                cliff_tools::plan(
                    &doc,
                    &scene,
                    cliff_tools::rectangle(rotate(a), rotate(b)),
                    0,
                )?
                .apply(&mut doc)?;
                doc.commit("short bank");
            }
            for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
                let result = bridge_tools::plan(
                    &doc,
                    &scene,
                    rotate([77, 67]),
                    rotate([84 + span, 67]),
                    kind,
                );
                let p = match result {
                    Ok(p) => p,
                    // New Urban wood heads are larger than the other retail
                    // heads. Reject spans that overlap their actual TMP cells.
                    Err(e) if e.to_string().contains("两端桥头重叠") => {
                        cases.push(serde_json::json!({"case":format!("{kind:?}-axis{axis}-span{span}"),"overlapping_heads_rejected":true}));
                        continue;
                    }
                    Err(e) => {
                        return Err(e)
                            .with_context(|| format!("short bridge {span}/{axis}/{kind:?}"));
                    }
                };
                if span == 3 {
                    let name = format!("{kind:?}-axis{axis}-short");
                    cases.push(verify(install, &scene, &mut doc, &p, out, &name, true)?);
                    doc.undo();
                }
            }
        }
        for wet in [false, true] {
            if wet && water.is_none() {
                continue;
            }
            let mut doc = document(&scene);
            for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
                cliff_tools::plan(
                    &doc,
                    &scene,
                    cliff_tools::rectangle(rotate(a), rotate(b)),
                    0,
                )?
                .apply(&mut doc)?;
                doc.commit("bank");
            }
            if wet {
                let mut origins = std::collections::BTreeSet::new();
                for at in cliff_tools::rectangle(rotate([78, 60]), rotate([95, 74])) {
                    let old = value(&doc, &scene, at);
                    let tile = scene.palette.raw_ids[&old.tile];
                    if old.level == 0 && i32::from(tile) == ids.clear {
                        doc.set(
                            Target::Terrain(at),
                            Some(Value::Terrain(TerrainValue {
                                tile: scene.catalog.raw_tiles[&water.unwrap()],
                                sub: 0,
                                ..old
                            })),
                        )?;
                    }
                    if ids
                        .cliffs
                        .cliff_set
                        .is_some_and(|base| (base..base + 22).contains(&tile))
                    {
                        let t = &scene.palette.templates[&(tile, 0)];
                        origins.insert([
                            at[0] - u16::from(old.sub) % t.width,
                            at[1] - u16::from(old.sub) / t.width,
                        ]);
                    }
                }
                doc.commit("channel");
                if coastal_cliffs::supports(&scene, 7) {
                    coastal_cliffs::plan(&doc, &scene, origins)?.apply(&mut doc)?;
                    doc.commit("coastal cliffs");
                }
            }
            for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
                let before = doc.snapshot().bytes()?;
                let p = bridge_tools::plan(&doc, &scene, rotate([77, 67]), rotate([98, 67]), kind)?;
                for (a, b) in [
                    ([98, 67], [77, 67]),
                    ([80, 67], [94, 67]),
                    ([81, 67], [93, 67]),
                ] {
                    let other = bridge_tools::plan(&doc, &scene, rotate(a), rotate(b), kind)?;
                    ensure!(
                        p.edits == other.edits && p.overlays == other.overlays,
                        "endpoint snap mismatch in {theater}/{kind:?}/{axis}/{wet}"
                    );
                }
                ensure!(
                    before == doc.snapshot().bytes()?,
                    "preview mutated document"
                );
                let name = format!(
                    "{kind:?}-axis{axis}-{}",
                    if wet { "water" } else { "ravine" }
                );
                cases.push(verify(install, &scene, &mut doc, &p, out, &name, true)?);
                doc.undo();
            }
        }
        if water.is_some() {
            let mut doc = document(&scene);
            shore_tools::plan(
                &doc,
                &scene,
                cliff_tools::rectangle(rotate([70, 55]), rotate([104, 80])),
                Some(true),
            )?
            .apply(&mut doc)?;
            doc.commit("river");
            for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
                for end in [76, 90] {
                    let a = rotate([70, 67]);
                    let b = rotate([end, 67]);
                    let draft = bridge_tools::preview_low(&doc, &scene, a, b, kind)?;
                    ensure!(
                        draft.overlays.len() == usize::from(end - 70 + 1) * 3,
                        "open-water preview stopped following the pointer"
                    );
                    ensure!(
                        bridge_tools::plan(&doc, &scene, a, b, kind).is_err(),
                        "unfinished crossing accepted for placement"
                    );
                }
                let p =
                    bridge_tools::plan(&doc, &scene, rotate([70, 67]), rotate([104, 67]), kind)?;
                ensure!(p.edits.is_empty(), "low bridge changed water");
                let reverse =
                    bridge_tools::plan(&doc, &scene, rotate([104, 67]), rotate([70, 67]), kind)?;
                ensure!(
                    p.overlays == reverse.overlays,
                    "reversed low bridge changed"
                );
                let name = format!("{kind:?}-axis{axis}-river");
                cases.push(verify(install, &scene, &mut doc, &p, out, &name, true)?);
                doc.undo();
            }
            // Retail LATs are often non-morphable despite being flat dry land.
            // Put a real one under both landing strips, retaining the river.
            let lat = scene
                .palette
                .templates
                .iter()
                .find_map(|(&(id, variant), t)| {
                    (variant == 0
                        && t.width == 1
                        && t.height == 1
                        && !scene.palette.morphable.contains(&id)
                        && scene
                            .palette
                            .groups
                            .get(&id)
                            .is_some_and(|g| g.name.contains("LAT"))
                        && t.cells[0]
                            .is_some_and(|s| s.slope == 0 && s.height == 0 && s.terrain == 14))
                    .then_some(id)
                });
            if let Some(lat) = lat {
                for (lo, hi) in [([64, 65], [69, 69]), ([105, 65], [110, 69])] {
                    for at in cliff_tools::rectangle(rotate(lo), rotate(hi)) {
                        let old = value(&doc, &scene, at);
                        if i32::from(scene.palette.raw_ids[&old.tile]) == ids.clear {
                            doc.set(
                                Target::Terrain(at),
                                Some(Value::Terrain(TerrainValue {
                                    tile: scene.catalog.raw_tiles[&lat],
                                    sub: 0,
                                    ..old
                                })),
                            )?;
                        }
                    }
                }
                doc.commit("non-morphable LAT banks");
                for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
                    let p = bridge_tools::plan(
                        &doc,
                        &scene,
                        rotate([69, 67]),
                        rotate([105, 67]),
                        kind,
                    )?;
                    ensure!(p.edits.is_empty(), "low bridge changed LAT terrain");
                    let name = format!("{kind:?}-axis{axis}-lat-river");
                    cases.push(verify(install, &scene, &mut doc, &p, out, &name, true)?);
                    doc.undo();
                }
            }
        }
    }
    Ok(serde_json::json!({"theater":theater,"cases":cases,"water_available":water.is_some()}))
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/bridge-audit".into()));
    let filter = args.next().map(|s| s.to_string_lossy().to_uppercase());
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for (theater, _) in THEATERS {
        if filter.as_ref().is_some_and(|f| f != theater) {
            continue;
        }
        println!("Auditing {theater} bridges...");
        reports.push(
            audit(&mut install, theater, &out.join(theater)).with_context(|| theater.to_owned())?,
        );
    }
    ensure!(!reports.is_empty(), "unknown theater filter");
    let report = serde_json::json!({"passed":true,"theaters":reports,"scope":"Installed TMP/SHP assets; both axes and materials; ravines, coastal cliffs and level-zero river shores; endpoint snapping, map reload, undo/redo and CPU previews. No in-game movement or destruction test."});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("Bridge audit passed: {}", out.join("report.json").display());
    Ok(())
}
