//! Actual-theater closed plateau generation, native terrain reload and PNGs.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::MapDocument,
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{cliff_tools as tools, map::load_scene_bytes, visual::ViewOptions};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/auto-cliff-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for (theater, _) in THEATERS {
        let scene = load_scene_bytes(
            &mut install,
            "plateau.map",
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
        let mut doc = MapDocument::new(
            scene
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        let original = doc.snapshot().bytes()?;
        if scene
            .palette
            .tile_ids
            .and_then(|ids| ids.cliffs.cliff_set)
            .is_none()
        {
            ensure!(
                tools::plan(&doc, &scene, tools::rectangle([60, 60], [70, 70]), 4).is_err(),
                "unsupported theater was accepted"
            );
            reports.push(serde_json::json!({"theater":theater,"unsupported_cliffs_rejected":true}));
            continue;
        }
        let mut rectangles = 0;
        for width in 1..10 {
            for height in 1..10 {
                let p = tools::plan(
                    &doc,
                    &scene,
                    tools::rectangle([60, 60], [60 + width, 60 + height]),
                    4,
                )?;
                p.apply(&mut doc)?;
                doc.cancel();
                rectangles += 1;
            }
        }
        ensure!(
            doc.snapshot().bytes()? == original,
            "rectangle planning mutated baseline"
        );
        for r in 2..=10 {
            let diamond =
                tools::enclose(&[[72 - r, 72], [72, 72 - r], [72 + r, 72], [72, 72 + r]])?;
            tools::plan(&doc, &scene, diamond, 4)
                .with_context(|| format!("{theater}: diagonal outline r={r}"))?;
        }
        let shapes = [
            ("rectangle", tools::rectangle([60, 60], [73, 71])),
            (
                "L",
                tools::enclose(&[[88, 60], [106, 60], [106, 66], [95, 66], [95, 78], [88, 78]])?,
            ),
            (
                "U",
                tools::enclose(&[
                    [60, 88],
                    [82, 88],
                    [82, 108],
                    [75, 108],
                    [75, 95],
                    [67, 95],
                    [67, 108],
                    [60, 108],
                ])?,
            ),
            (
                "diamond",
                tools::enclose(&[[98, 102], [106, 94], [114, 102], [106, 110]])?,
            ),
        ];
        let mut expected = BTreeMap::new();
        let mut details = Vec::new();
        for (name, region) in shapes {
            let p = tools::plan(&doc, &scene, region, 4)
                .with_context(|| format!("{theater} {name}"))?;
            p.apply(&mut doc)?;
            for stamp in &p.cliffs {
                let t = &scene.palette.templates[&(stamp.tile, 0)];
                for (sub, s) in t.cells.iter().enumerate() {
                    if let Some(s) = s {
                        let at = [
                            stamp.origin[0] + sub as u16 % t.width,
                            stamp.origin[1] + sub as u16 / t.width,
                        ];
                        let v = doc.terrain(at).context("cliff subtile missing")?;
                        ensure!(
                            v.tile == scene.catalog.raw_tiles[&stamp.tile]
                                && v.sub == sub as u8
                                && v.level == stamp.base + s.height,
                            "incomplete cliff template"
                        );
                    }
                }
            }
            expected.extend(p.edits);
            details.push(serde_json::json!({"shape":name,"interior_cells":p.interior.len(),"cliff_stamps":p.cliffs.len(),"complete_templates":true}));
        }
        doc.commit("automatic plateaus");
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(output.join(format!("{theater}.map")), &bytes)?;
        let reloaded = load_scene_bytes(
            &mut install,
            "reloaded.map",
            "audit".into(),
            bytes.clone(),
            Instant::now(),
        )?;
        let cells: BTreeMap<_, _> = reloaded.cells.iter().map(|c| ([c.rx, c.ry], c)).collect();
        let mut bounds = egui::Rect::NOTHING;
        for (at, v) in &expected {
            let cell = cells.get(at).context("engine cell missing")?;
            let tile = scene
                .catalog
                .raw_tiles
                .iter()
                .find_map(|(&tile, &raw)| (raw == v.tile).then_some(tile))
                .context("tile translation")?;
            ensure!(
                (cell.tile_id, cell.sub_tile, cell.z) == (tile, v.sub, v.level),
                "{theater}: engine changed {at:?}"
            );
            if let Some(image) = reloaded.image(cell) {
                bounds = bounds.union(egui::Rect::from_min_size(
                    egui::pos2(
                        cell.screen_x + image.offset_x as f32,
                        cell.screen_y + image.offset_y as f32,
                    ),
                    egui::vec2(image.width as f32, image.height as f32),
                ));
            }
        }
        doc.undo();
        ensure!(doc.snapshot().bytes()? == original, "undo failed");
        doc.redo();
        ensure!(doc.snapshot().bytes()? == bytes, "redo failed");
        let bounds = bounds.expand(90.);
        let size = ra2_map_io::publication::fit(bounds, 1800);
        let pixels = reloaded.visual.raster(
            bounds,
            size,
            ViewOptions {
                lighting: false,
                ..Default::default()
            },
        )?;
        image::save_buffer(
            output.join(format!("{theater}.png")),
            &pixels,
            size[0],
            size[1],
            image::ColorType::Rgba8,
        )?;
        reports.push(
            serde_json::json!({"theater":theater,"rectangles_checked":rectangles,"diagonal_sizes_checked":9,"shapes":details,
            "engine_reload_cells":expected.len(),"engine_reload_matches":true,"undo_redo":true}),
        );
    }
    let report = serde_json::json!({"passed":true,"theaters":reports,"scope":"Standard land CliffSet; atomic local authoring. Rectangles, concave L/U and diagonal outlines, complete TMPs, actual engine terrain reload. No native-client pixel parity claim."});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
