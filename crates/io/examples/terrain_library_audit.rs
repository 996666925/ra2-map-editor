//! Six-theater template placement/reload audit and complete-cliff preview sheets.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::{assets::Install, terrain_library::TerrainCategory};
use ra2_map_document::{
    document::MapDocument,
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{map::load_scene_bytes, terrain_tools as tools, visual::ViewOptions};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/terrain-library-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for (theater, _) in THEATERS {
        let scene = load_scene_bytes(
            &mut install,
            "templates.map",
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
        let mut seen = BTreeSet::new();
        let templates: Vec<_> = scene
            .brushes
            .iter()
            .filter(|b| b.key.variant == 0 && seen.insert(b.key.tile_id))
            .collect();
        let mut categories = BTreeMap::<String, usize>::new();
        let mut cliff_ids = Vec::new();
        for brush in &templates {
            let group = scene
                .palette
                .groups
                .get(&brush.key.tile_id)
                .context("template group missing")?;
            *categories.entry(group.category.name().into()).or_default() += 1;
            ensure!(
                scene.palette.previews.contains_key(&(brush.key.tile_id, 0)),
                "{theater}: incomplete preview {}",
                brush.label
            );
            let planned = tools::template_placement(&doc, &scene, brush, [80, 80], Some(0))
                .with_context(|| format!("{theater}: {}", brush.label))?;
            tools::stamp_at_height(&mut doc, &scene, brush, [80, 80], true, Some(0))?;
            for (at, v) in &planned {
                ensure!(
                    doc.terrain(*at) == Some(*v),
                    "{theater}: preflight/stamp mismatch"
                );
            }
            doc.cancel();
            if group.category == TerrainCategory::Cliff {
                cliff_ids.push(brush.key.tile_id);
            }
        }
        // Exercise several distinct orientations/shapes plus other terrain families.
        let mut selected = Vec::new();
        for (category, limit) in [
            (TerrainCategory::Cliff, 8),
            (TerrainCategory::Ramp, 2),
            (TerrainCategory::Road, 2),
            (TerrainCategory::Shore, 2),
            (TerrainCategory::Water, 1),
            (TerrainCategory::Bridge, 1),
        ] {
            let candidates: Vec<_> = templates
                .iter()
                .copied()
                .filter(|b| scene.palette.groups[&b.key.tile_id].category == category)
                .collect();
            for index in 0..limit.min(candidates.len()) {
                selected.push(candidates[index * candidates.len() / limit.min(candidates.len())]);
            }
        }
        let mut expected = BTreeMap::new();
        for (i, brush) in selected.iter().enumerate() {
            let at = [48 + (i % 4) as u16 * 18, 48 + (i / 4) as u16 * 18];
            let planned = tools::template_placement(&doc, &scene, brush, at, Some(4))?;
            expected.extend(planned);
            tools::stamp_at_height(&mut doc, &scene, brush, at, true, Some(4))?;
        }
        doc.commit("terrain templates");
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(output.join(format!("{theater}.map")), &bytes)?;
        let reloaded = load_scene_bytes(
            &mut install,
            "reload.map",
            "audit".into(),
            bytes.clone(),
            Instant::now(),
        )?;
        for (at, v) in &expected {
            let cell = reloaded
                .cells
                .iter()
                .find(|c| [c.rx, c.ry] == *at)
                .context("rendered cell missing")?;
            let tile = scene
                .catalog
                .raw_tiles
                .iter()
                .find_map(|(&tile, &raw)| (raw == v.tile).then_some(tile))
                .context("raw translation missing")?;
            ensure!(
                (cell.tile_id, cell.sub_tile, cell.z) == (tile, v.sub, v.level),
                "{theater}: engine reload changed {at:?}"
            );
        }
        doc.undo();
        ensure!(doc.state_id == 0, "{theater}: undo failed");
        doc.redo();
        ensure!(doc.snapshot().bytes()? == bytes, "{theater}: redo failed");

        // A contact sheet makes every cliff orientation reviewable locally.
        let columns = 8u32;
        let mut sheet = image::RgbaImage::from_pixel(
            columns * 180,
            cliff_ids.len().div_ceil(columns as usize).max(1) as u32 * 140,
            image::Rgba([23, 29, 35, 255]),
        );
        for (i, tile) in cliff_ids.iter().enumerate() {
            let preview = &scene.palette.previews[&(*tile, 0)];
            let source = image::RgbaImage::from_raw(
                preview.info.width,
                preview.info.height,
                preview.rgba.clone(),
            )
            .context("invalid preview size")?;
            let scale = (170. / f64::from(preview.info.width))
                .min(130. / f64::from(preview.info.height))
                .min(1.);
            let thumb = image::imageops::resize(
                &source,
                (f64::from(source.width()) * scale).round().max(1.) as u32,
                (f64::from(source.height()) * scale).round().max(1.) as u32,
                image::imageops::FilterType::Nearest,
            );
            image::imageops::overlay(
                &mut sheet,
                &thumb,
                ((i % 8) as u32 * 180 + (180 - thumb.width()) / 2).into(),
                ((i / 8) as u32 * 140 + (140 - thumb.height()) / 2).into(),
            );
        }
        sheet.save(output.join(format!("{theater}-cliffs.png")))?;
        let size = [1600, 1000];
        let pixels = reloaded.visual.raster(
            reloaded.bounds,
            size,
            ViewOptions {
                lighting: false,
                ..Default::default()
            },
        )?;
        image::save_buffer(
            output.join(format!("{theater}-map.png")),
            &pixels,
            size[0],
            size[1],
            image::ColorType::Rgba8,
        )?;
        reports.push(serde_json::json!({"theater":theater,"all_pristine_templates_checked":templates.len(),
            "categories":categories,"cliff_sheet_tile_ids":cliff_ids,"engine_reload_templates":selected.len(),
            "engine_reload_cells":expected.len(),"engine_reload_matches":true,"undo_redo":true}));
    }
    let report = serde_json::json!({"passed":true,"theaters":reports,
        "scope":"All readable pristine templates: explicit-base preflight/stamp. Representative templates: save, engine reload and undo/redo. Static previews; no native-client pixel comparison."});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
