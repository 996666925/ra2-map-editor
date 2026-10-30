//! Real-resource radar framing, player starts and embedded-preview checks.
use anyhow::{Context, Result, ensure};
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
use ra2_map_io::{
    editor_io,
    publication::{self, Publisher},
};
use ra2_map_scene::radar::{self, SymbolKind};
use std::{collections::HashSet, path::PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(args.next().context("output directory required")?);
    std::fs::create_dir_all(&output)?;
    let mut publisher = Publisher::default();
    let mut reports = Vec::new();
    for (index, path) in args.map(PathBuf::from).enumerate() {
        let scene = publisher.scene(std::fs::read(&path)?, &directory)?;
        let doc = MapDocument::new(
            scene
                .source
                .edit_base
                .as_ref()
                .map_err(|error| anyhow::anyhow!(error.clone()))?
                .clone(),
        );
        let radar = radar::terrain(&scene)?;
        let flat = publisher.scene(
            NewMapOptions {
                width: scene.source.header.width as u16,
                height: scene.source.header.height as u16,
                level: 0,
                ..Default::default()
            }
            .bytes()?,
            &directory,
        )?;
        let flat_radar = radar::terrain(&flat)?;
        ensure!(
            radar.bounds == flat_radar.bounds,
            "height changes cropped map framing: {}",
            path.display()
        );
        ensure!(
            radar.image.size == flat_radar.image.size,
            "height changes changed map aspect ratio"
        );
        let colours: HashSet<_> = scene
            .cells
            .iter()
            .flat_map(|cell| [cell.radar_left, cell.radar_right])
            .collect();
        ensure!(
            radar.image.pixels.iter().all(|pixel| {
                pixel.a() == 255 && colours.contains(&[pixel.r(), pixel.g(), pixel.b()])
            }),
            "radar contains uncovered pixels"
        );
        let starts: Vec<_> = radar::symbols(&scene, &doc)
            .into_iter()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Start(_)))
            .collect();
        ensure!(
            starts
                .iter()
                .all(|start| radar.bounds.contains(start.position)),
            "player start cropped away"
        );
        let bytes = publication::embed_scene_preview(&scene)?;
        editor_io::validate(&bytes)?;
        let file = format!("radar-{index}");
        let png = output.join(format!("{file}.png"));
        let size = publication::preview_png(&bytes, &png)?;
        let decoded = image::open(&png)?.to_rgba8();
        ensure!(
            decoded == publication::render_preview(&scene)?,
            "PreviewPack pixels differ from navigator"
        );
        std::fs::write(output.join(format!("{file}.map")), bytes)?;
        reports.push(serde_json::json!({"input":path,"passed":true,"size":size,
            "bounds":[radar.bounds.left(),radar.bounds.top(),radar.bounds.right(),radar.bounds.bottom()],
            "starts":starts.len(),"height_changes_preserve_full_map":true,"preview":png}));
    }
    ensure!(!reports.is_empty(), "at least one input map required");
    let report = serde_json::json!({"passed":true,"cases":reports});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
