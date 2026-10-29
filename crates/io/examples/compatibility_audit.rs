//! Repeatable compatibility evidence; external readers remain separate claims.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
use ra2_map_io::{editor_io, publication::fit};
use ra2_map_scene::map;
use std::{path::PathBuf, time::Instant};
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p5-compatibility".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut cases = Vec::new();
    for theater in ["TEMPERATE", "SNOW", "URBAN", "NEWURBAN", "DESERT", "LUNAR"] {
        let name = format!("compat-{theater}.map");
        let bytes = NewMapOptions {
            width: 40,
            height: 40,
            theater: theater.into(),
            ..Default::default()
        }
        .bytes()?;
        std::fs::write(output.join(&name), &bytes)?;
        let case = (|| -> Result<_> {
            let scene = map::load_scene_bytes(
                &mut install,
                &name,
                "compatibility".into(),
                bytes,
                Instant::now(),
            )?;
            ensure!(!scene.images.is_empty(), "no terrain decoded");
            let mut doc = MapDocument::new(
                scene
                    .source
                    .edit_base
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!(e.clone()))?
                    .clone(),
            );
            let at = doc
                .records()
                .values()
                .find(|r| r.section == "Waypoints")
                .and_then(|r| r.cell())
                .context("start")?;
            doc.add_record("Units", at, "Neutral", "MTNK")?;
            doc.commit("compatibility edit");
            let bytes = doc.snapshot().bytes()?;
            editor_io::validate(&bytes)?;
            let scene = map::load_scene_bytes(
                &mut install,
                &name,
                "reopen".into(),
                bytes.clone(),
                Instant::now(),
            )?;
            let rect = scene.visual.bounds.context("visual")?;
            let size = fit(rect, 512);
            let rgba = scene.visual.raster(rect, size, Default::default())?;
            let bytes = ra2_map_document::preview::embed(&bytes, size[0], size[1], &rgba)?;
            let map = editor_io::validate(&bytes)?;
            ensure!(
                map.header.theater.eq_ignore_ascii_case(theater),
                "saved theater"
            );
            std::fs::write(output.join(&name), bytes)?;
            image::save_buffer(
                output.join(format!("{theater}.png")),
                &rgba,
                size[0],
                size[1],
                image::ColorType::Rgba8,
            )?;
            Ok(
                serde_json::json!({"case":theater,"passed":true,"authored_real_resources":true,"edit_publish_reopen":true,"terrain_cells":scene.cells.len(),"map":name}),
            )
        })();
        cases.push(case.unwrap_or_else(
            |e| serde_json::json!({"case":theater,"passed":false,"error":format!("{e:#}")}),
        ));
    }
    for name in [
        "all01umd.map",
        "sov01umd.map",
        "all01t.map",
        "sov01t.map",
        "XMP03T4.MAP",
    ] {
        let case = (|| -> Result<_> {
            let scene = map::load_scene(&mut install, name)?;
            let bytes = scene.source.bytes();
            let mut doc = MapDocument::new(
                scene
                    .source
                    .edit_base
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!(e.clone()))?
                    .clone(),
            );
            ensure!(
                doc.snapshot().bytes()? == bytes,
                "unmodified retail bytes changed"
            );
            doc.set_ini(
                "Basic",
                "EditorCompatibilityNote",
                Some(b"preserved".to_vec()),
            )?;
            doc.commit("retail edit");
            let modified = doc.snapshot().bytes()?;
            editor_io::validate(&modified)?;
            ensure!(
                doc.undo() && doc.snapshot().bytes()? == bytes,
                "retail undo"
            );
            std::fs::write(output.join(name), modified)?;
            Ok(
                serde_json::json!({"case":name,"status":"passed","passed":true,"source":scene.source.origin,"read_only_byte_exact":true,"edit_parse_undo":true,"terrain_cells":scene.cells.len()}),
            )
        })();
        cases.push(case.unwrap_or_else(|e|serde_json::json!({"case":name,"status":"unavailable_or_failed","passed":false,"error":format!("{e:#}")})));
    }
    let mut bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    bytes.extend_from_slice(b"\n; legacy comment \xd6\xd0\xce\xc4\n[PrivateExtension]\nOpaque=\x81\xfe\nDup=first\nDup=second\n[PrivateExtension]\nOther=vendor-private\n");
    let (source, _) = ra2_map_document::source::SourceMap::parse(
        "legacy.map".into(),
        "fixture".into(),
        bytes.clone(),
    )?;
    let mut doc = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
    ensure!(doc.snapshot().bytes()? == bytes, "legacy initial roundtrip");
    doc.waypoint(20, [24, 40])?;
    doc.commit("legacy edit");
    let modified = doc.snapshot().bytes()?;
    ensure!(
        modified
            .windows(b"Opaque=\x81\xfe".len())
            .any(|s| s == b"Opaque=\x81\xfe"),
        "opaque bytes changed"
    );
    ensure!(
        modified
            .windows(b"Dup=first\nDup=second".len())
            .any(|s| s == b"Dup=first\nDup=second"),
        "duplicates changed"
    );
    editor_io::validate(&modified)?;
    std::fs::write(output.join("legacy-extension.map"), modified)?;
    ensure!(
        doc.undo() && doc.snapshot().bytes()? == bytes,
        "legacy undo"
    );
    cases.push(serde_json::json!({"case":"legacy_encoding_duplicates_extensions","passed":true,"edit_parse_undo":true}));
    let production_passed = cases.iter().take(6).all(|c| c["passed"] == true);
    let report = serde_json::json!({"schema_version":1,"production_six_theaters_passed":production_passed,"complete_matrix_passed":false,"profile":install.revision,"cases":cases,
        "external_checks":[{"reader":"FA2 GUI","status":"requires_external_run","scope":"open edited maps"},{"reader":"FA2 bundled XCC/lzo","status":"separate_probe","scope":"packed codecs only"},{"reader":"WAE GUI","status":"not_available_in_workspace"},{"reader":"original gamemd.exe","status":"not_verified","scope":"retail-size and stock-field maps only; VERA extensions excluded"}]});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(production_passed, "one or more theaters failed");
    println!("{}", output.join("report.json").display());
    Ok(())
}
