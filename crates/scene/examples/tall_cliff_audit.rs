//! Real TMP height checks, saved-map reload and reviewable CPU previews.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{MapDocument, Target, Value},
    new_map::NewMapOptions,
};
use ra2_map_scene::{
    cliff_authoring, cliff_tools, map::load_scene_bytes, terrain_tools, visual::ViewOptions,
};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory required")?);
    let out = PathBuf::from(args.next().context("output directory required")?);
    std::fs::create_dir_all(&out)?;
    let mut install = Install::open(&game)?;
    let scene = load_scene_bytes(
        &mut install,
        "high.map",
        "audit".into(),
        NewMapOptions {
            width: 80,
            height: 80,
            level: 0,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let mut cases = Vec::new();
    for base in [12, 32, cliff_tools::MAX_CLIFF_BASE] {
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        for c in &scene.cells {
            let at = [c.rx, c.ry];
            doc.set(
                Target::Terrain(at),
                Some(Value::Terrain(ra2_map_document::document::TerrainValue {
                    level: base,
                    ..terrain_tools::value(&doc, &scene, at)
                })),
            )?;
        }
        doc.commit("high ground");
        let plateau = cliff_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle([60, 60], [84, 84]),
            base,
        )?;
        plateau.apply(&mut doc)?;
        doc.commit("plateau");
        for (case, start, end) in [
            ("plateau", None, None),
            ("straight", Some([72, 84]), Some([76, 84])),
            ("corner", Some([84, 84]), Some([80, 84])),
        ] {
            let before = doc.snapshot().bytes()?;
            let ramp;
            let p = if let (Some(a), Some(b)) = (start, end) {
                ramp = cliff_authoring::ramp(&doc, &scene, a, b, base)?;
                &ramp
            } else {
                &plateau
            };
            p.apply(&mut doc)?;
            doc.commit(case);
            let bytes = doc.snapshot().bytes()?;
            let name = format!("height-{}-{case}", base + 4);
            std::fs::write(out.join(format!("{name}.map")), &bytes)?;
            let loaded = load_scene_bytes(
                &mut install,
                &name,
                "reload".into(),
                bytes.clone(),
                Instant::now(),
            )?;
            let reopened = MapDocument::new(loaded.source.edit_base.as_ref().unwrap().clone());
            for (&at, &v) in &p.edits {
                ensure!(reopened.terrain(at) == Some(v), "saved height mismatch");
                let c = loaded
                    .cells
                    .iter()
                    .find(|c| [c.rx, c.ry] == at)
                    .context("missing engine cell")?;
                ensure!(
                    c.z == v.level
                        && c.tile_id == scene.palette.raw_ids[&v.tile]
                        && c.sub_tile == v.sub,
                    "engine reload mismatch"
                );
            }
            if case != "plateau" {
                doc.undo();
                ensure!(doc.snapshot().bytes()? == before, "undo mismatch");
                doc.redo();
                ensure!(doc.snapshot().bytes()? == bytes, "redo mismatch");
                doc.undo();
            }
            let mut bounds = egui::Rect::NOTHING;
            for c in &loaded.cells {
                if (56..=90).contains(&c.rx)
                    && (56..=90).contains(&c.ry)
                    && let Some(rect) = loaded.cell_rect(c)
                {
                    bounds = bounds.union(rect);
                }
            }
            let scale = (1000. / bounds.width().max(bounds.height())).min(1.);
            let size = [
                (bounds.width() * scale).ceil() as u32,
                (bounds.height() * scale).ceil() as u32,
            ];
            let rgba = loaded.visual.raster(
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
            cases.push(serde_json::json!({"case":name,"max_height":base+4,"reload":true,"cells":p.edits.len()}));
            println!("{name} passed");
        }
    }
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"cases":cases,
        "scope":"Actual TMP resources, engine terrain reload and CPU previews; no in-game movement validation."}))?,
    )?;
    Ok(())
}
