//! Real-installation framework materials, unchanged map data and reviewable PNGs.
use anyhow::{Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
use ra2_map_scene::{cliff_authoring, cliff_tools, map::load_scene_bytes, visual::ViewOptions};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let directory = PathBuf::from(std::env::args().nth(1).expect("game directory"));
    let output = PathBuf::from(
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "target/framework-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut results = Vec::new();
    for theater in ["TEMPERATE", "SNOW", "URBAN", "NEWURBAN", "DESERT", "LUNAR"] {
        let options = NewMapOptions {
            width: 60,
            height: 60,
            level: 4,
            theater: theater.into(),
            ..Default::default()
        };
        let scene = load_scene_bytes(
            &mut install,
            "framework.map",
            "audit".into(),
            options.bytes()?,
            Instant::now(),
        )?;
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let plateau =
            cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4);
        if let Ok(plan) = plateau {
            plan.apply(&mut doc)?;
            doc.commit("plateau");
            cliff_authoring::ramp(&doc, &scene, [50, 65], [56, 65], 4)?.apply(&mut doc)?;
            doc.commit("ramp");
        }
        let before = doc.snapshot().bytes()?;
        std::fs::write(output.join(format!("{theater}.map")), &before)?;
        let scene = load_scene_bytes(
            &mut install,
            "framework.map",
            "audit".into(),
            before.clone(),
            Instant::now(),
        )?;
        let palette = &scene.palette;
        ensure!(
            palette.framework_images.len() == palette.images.len(),
            "missing framework materials"
        );
        for (key, image) in &palette.images {
            let other = &palette.framework_images[key];
            ensure!(
                image.info.offset == other.info.offset
                    && image.info.width == other.info.width
                    && image.info.height == other.info.height,
                "geometry changed"
            );
            ensure!(
                image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(other.rgba.as_chunks::<4>().0.iter())
                    .all(|(a, b)| a[3] == b[3]),
                "alpha admission changed"
            );
        }
        let rect = egui::Rect::from_center_size(egui::pos2(0., 1470.), egui::vec2(1350., 1100.));
        for framework in [false, true] {
            let rgba = scene.visual.raster(
                rect,
                [1350, 1100],
                ViewOptions {
                    framework,
                    lighting: false,
                    ..Default::default()
                },
            )?;
            image::save_buffer(
                output.join(format!(
                    "{theater}-{}.png",
                    if framework { "framework" } else { "normal" }
                )),
                &rgba,
                1350,
                1100,
                image::ColorType::Rgba8,
            )?;
        }
        ensure!(doc.snapshot().bytes()? == before, "view modified map");
        results.push(serde_json::json!({"theater":theater,"subtiles":palette.images.len(),"native_templates":palette.framework_native.len(),"alpha_and_geometry_unchanged":true,"map_unchanged":true}));
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}
