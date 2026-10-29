//! Isolated resource mutation, cache invalidation and version record audit.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::{assets::Install, profile::Revision};
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions, region::Clipboard};
use ra2_map_io::{editor_io, provenance, publication::Publisher};
use ra2_map_scene::map;
use std::{path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(args.next().context("output directory required")?);
    let exe = args.next().map(PathBuf::from);
    std::fs::create_dir_all(&output)?;
    // Hard links are read-only inputs. Only the new audit INI is modified;
    // the actual installation is never written by this example.
    let resources = output.join(format!("resources-{}", std::process::id()));
    std::fs::create_dir(&resources)?;
    for entry in std::fs::read_dir(&game)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            if matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some(
                    "mix"
                        | "ini"
                        | "pal"
                        | "csf"
                        | "shp"
                        | "vxl"
                        | "hva"
                        | "vpl"
                        | "tmp"
                        | "tem"
                        | "sno"
                        | "urb"
                        | "ubn"
                        | "des"
                        | "lun"
                        | "wav"
                        | "aud"
                        | "bik"
                        | "vqa"
                        | "bag"
                        | "idx"
                        | "pcx"
                        | "fnt"
                        | "pkt"
                        | "wma"
                        | "ogg"
                )
            ) {
                let dest = resources.join(entry.file_name());
                if std::fs::hard_link(&path, &dest).is_err() {
                    std::fs::copy(path, dest)?;
                }
            }
        }
    }
    let original = Revision::scan(&game)?;
    let mut install = Install::open(&resources)?;
    let scene = map::load_scene_bytes(
        &mut install,
        "profile.map",
        "audit".into(),
        NewMapOptions {
            width: 40,
            height: 40,
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
    doc.resource_profile = Some(scene.revision.id());
    let at = doc
        .records()
        .values()
        .find(|r| r.section == "Waypoints")
        .and_then(|r| r.cell())
        .context("cell")?;
    let clipboard = Clipboard::capture(
        &doc,
        &std::collections::BTreeSet::from([at]),
        Default::default(),
    )?;
    let mut publisher = Publisher::default();
    publisher.scene(doc.snapshot().bytes()?, &resources)?;
    publisher.check_profile(&resources, doc.resource_profile.as_deref())?;
    let mutation = resources.join("editor-profile-audit.ini");
    ensure!(
        !mutation.exists(),
        "audit filename is already a resource input"
    );
    std::fs::write(&mutation, b"[EditorProfileAudit]\nValue=1\n")?;
    ensure!(
        publisher
            .check_profile(&resources, doc.resource_profile.as_deref())
            .is_err(),
        "stale resources admitted"
    );
    let revision = publisher.check_profile(&resources, None)?;
    ensure!(
        revision.id() != scene.revision.id(),
        "resource mutation not detected"
    );
    let refreshed = publisher.scene(doc.snapshot().bytes()?, &resources)?;
    ensure!(
        *refreshed.revision == revision,
        "publisher reused old installation"
    );
    doc.resource_profile = Some(revision.id());
    ensure!(
        clipboard.plan(&doc, at).is_err(),
        "cross Mod clipboard admitted"
    );
    let io = editor_io::EditorIo::new(egui::Context::default(), output.join("io"))?;
    let path = output.join("saved.map");
    io.send(editor_io::Job::Save {
        directory: Some(resources.clone()),
        snapshot: doc.snapshot(),
        path: path.clone(),
        expected: editor_io::Expected::Capture,
    })?;
    loop {
        if let editor_io::Done::Save { result, .. } = io
            .results
            .recv_timeout(std::time::Duration::from_secs(30))?
        {
            ensure!(result?.warning.is_none(), "sidecar write warning");
            break;
        }
    }
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(provenance::sidecar(&path))?)?;
    ensure!(
        saved["resource_profile"] == serde_json::to_value(&revision)?,
        "save resource record"
    );
    ensure!(
        saved["map_hash"]
            == format!(
                "{:016x}",
                ra2_map_assets::profile::hash(&std::fs::read(&path)?)
            ),
        "saved map hash"
    );
    let mut process_check = serde_json::Value::Null;
    if let Some(exe) = exe {
        let mut process = editor_io::launch(
            &doc.snapshot().bytes()?,
            &exe,
            &resources,
            &output,
            &Default::default(),
        )?;
        let session: serde_json::Value =
            serde_json::from_slice(&std::fs::read(process.folder.join("editor-session.json"))?)?;
        ensure!(
            session["resource_profile"] == serde_json::to_value(&revision)?,
            "play resource record"
        );
        ensure!(
            session["executable"]["hash"]
                == format!("{:016x}", ra2_map_assets::profile::file_hash(&exe)?),
            "play executable hash"
        );
        let running = process.child.try_wait()?.is_none();
        if running {
            process.child.kill()?;
        }
        process.child.wait()?;
        process_check = serde_json::json!({"started":true,"session":process.folder,"executable_hash_matches":true,"scope":"process launch and provenance only; no game result or original-engine compatibility claim"});
    }
    ensure!(
        Revision::scan(&game)? == original,
        "original installation was modified"
    );
    let report = serde_json::json!({"passed":true,"stale_resources_rejected":true,"publisher_cache_invalidated":true,"cross_mod_clipboard_rejected":true,"save_sidecar_verified":true,"original_install_unchanged":true,"before":scene.revision,"after":revision,"playtest":process_check});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", output.join("report.json").display());
    Ok(())
}
