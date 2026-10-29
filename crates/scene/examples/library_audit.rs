//! Run against an installed game; writes coverage and round-trip evidence.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{MapDocument, Target, Value},
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::map::load_scene_bytes;
use std::{collections::BTreeMap, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(args.next().unwrap_or_else(|| "target/library-audit".into()));
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for (theater, _) in THEATERS {
        let started = Instant::now();
        let bytes = NewMapOptions {
            theater: theater.into(),
            width: 40,
            height: 40,
            players: 30,
            ..Default::default()
        }
        .bytes()?;
        let scene = load_scene_bytes(
            &mut install,
            "audit.map",
            "audit".into(),
            bytes,
            Instant::now(),
        )?;
        ensure!(
            !scene.images.is_empty() && !scene.brushes.is_empty(),
            "{theater}: terrain not loaded"
        );
        let tank = scene
            .sprites
            .objects
            .get(&("Units".into(), "HTNK".into()))
            .context("no HTNK map appearance")?;
        ensure!(tank.facings.len() == 32, "{theater}: VXL facings missing");
        let mut hashes = std::collections::HashSet::new();
        for facing in [0, 64, 128, 192] {
            let image = tank.image(facing);
            ensure!(
                image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0),
                "empty facing"
            );
            hashes.insert(image.rgba.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            }));
        }
        ensure!(
            hashes.len() == 4,
            "{theater}: tank directions reuse one thumbnail"
        );
        let mut categories = BTreeMap::new();
        for (section, ids) in &scene.catalog.object_types {
            let available = ids
                .iter()
                .filter(|id| {
                    scene
                        .sprites
                        .get(section, id)
                        .is_some_and(|p| p.image.is_some())
                })
                .count();
            categories.insert(
                section,
                serde_json::json!({"total": ids.len(), "previews": available}),
            );
        }
        let missing: BTreeMap<_, _> = scene
            .sprites
            .entries
            .iter()
            .filter(|(_, p)| p.image.is_none())
            .map(|((section, id), p)| (format!("{section}:{id}"), &p.detail))
            .collect();
        let mut doc = MapDocument::new(
            scene
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        let tree = scene.catalog.object_types["Terrain"]
            .iter()
            .find(|id| {
                scene
                    .sprites
                    .get("Terrain", id)
                    .is_some_and(|p| p.image.is_some())
            })
            .context("no terrain object preview")?;
        doc.add_record("Terrain", [30, 30], "Neutral", tree)?;
        let smudge = scene.catalog.object_types["Smudge"]
            .iter()
            .find(|id| {
                scene
                    .sprites
                    .get("Smudge", id)
                    .is_some_and(|p| p.image.is_some())
            })
            .context("no smudge")?;
        doc.add_record("Smudge", [31, 30], "Neutral", smudge)?;
        doc.add_record("Structures", [30, 31], "Neutral", "GAPOWR")?;
        doc.add_record("Infantry", [31, 31], "Neutral", "E1")?;
        doc.set(Target::Overlay([32, 30]), Some(Value::Overlay(0, 0)))?;
        doc.commit("library objects");
        let saved = doc.snapshot().bytes()?;
        std::fs::write(output.join(format!("{theater}.map")), &saved)?;
        let reopened = load_scene_bytes(
            &mut install,
            "reopened.map",
            "audit".into(),
            saved.clone(),
            Instant::now(),
        )?;
        let doc = MapDocument::new(
            reopened
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        ensure!(
            doc.snapshot().bytes()? == saved && doc.overlay([32, 30]) == (0, 0),
            "{theater}: save roundtrip failed"
        );
        ensure!(
            doc.records()
                .values()
                .filter(|r| r.section == "Terrain" || r.section == "Smudge")
                .count()
                == 2,
            "{theater}: environment records lost"
        );
        reports.push(serde_json::json!({"theater": theater, "categories": categories, "terrain_subtiles": scene.brushes.len(), "variant_subtiles": scene.brushes.iter().filter(|b| b.key.variant > 0).count(), "overlays": scene.catalog.overlays.len(), "overlay_frames": scene.sprites.overlay_frames.len(), "missing_previews": missing, "save_reopen": true, "map_objects": scene.sprites.objects.len(), "voxel_facings": tank.facings.len(), "distinct_cardinal_facings": hashes.len(), "elapsed_ms": started.elapsed().as_millis()}));
        println!(
            "{theater}: {} terrain subtiles, {} sprite entries; save/reopen passed",
            scene.brushes.len(),
            scene.sprites.entries.len()
        );
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"passed": true, "theaters": reports}))?,
    )?;
    Ok(())
}
