//! Stable, display-free batch validation and publication. No egui context/device.
use anyhow::{Context, Result, bail, ensure};
use ra2_map_assets::{
    assets::Install,
    profile::{Profile, Revision},
};
use ra2_map_document::{
    document::MapDocument,
    mission::{self, Severity},
};
use ra2_map_scene::{map, visual::ViewOptions};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub const HELP: &str = "Headless commands (no window/GPU required):
  ra2-map-editor validate [OPTIONS] MAP...
  ra2-map-editor export   [OPTIONS] MAP...    # full-map PNG
  ra2-map-editor publish  [OPTIONS] MAP...    # .map with PreviewPack + version sidecar
Options:
  --game-dir PATH       Resource installation (or RA2_DIR)
  --profile FILE        Version 1 Mod profile; pinned VERA20k schema
  --preset FILE         Version 1 JSON preset; explicit flags override it
  --report FILE         Version 1 JSON batch report (also printed to stdout)
  --output DIR          Required for export/publish
  --max-edge N          PNG edge limit, 0 = native size (default 4096)
  --strict              Warnings also fail validation
  --no-dependencies     Skip known mission/media dependencies
  --overwrite           Replace existing output artifacts
  --                    Remaining arguments are map paths/names
Exit codes: 0 passed, 2 per-map validation/load/output failures,
            1 setup/arguments/report IO failure.
Input maps are never overwritten; output names must be unique.";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preset {
    pub version: u32,
    pub game_dir: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub max_edge: u32,
    pub strict: bool,
    pub dependencies: bool,
}
impl Default for Preset {
    fn default() -> Self {
        Self {
            version: 1,
            game_dir: None,
            profile: None,
            output: None,
            max_edge: 4096,
            strict: false,
            dependencies: true,
        }
    }
}
struct Args {
    command: String,
    preset: Preset,
    report: Option<PathBuf>,
    overwrite: bool,
    maps: Vec<String>,
    preset_file: Option<PathBuf>,
}
fn parse(args: Vec<OsString>) -> Result<Args> {
    let command = args[0]
        .to_str()
        .context("command is not Unicode")?
        .to_owned();
    let mut preset = Preset::default();
    let mut preset_file = None;
    let mut scan = args.iter().skip(1);
    while let Some(arg) = scan.next() {
        if arg == "--" {
            break;
        }
        if arg == "--preset" {
            ensure!(preset_file.is_none(), "only one --preset is allowed");
            preset_file = Some(PathBuf::from(
                scan.next().context("--preset requires a value")?,
            ));
        }
    }
    if let Some(path) = &preset_file {
        ensure!(std::fs::metadata(path)?.len() < 65536, "preset too large");
        preset = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(preset.version == 1, "unsupported preset version");
        let parent = path.parent().context("preset directory")?;
        for value in [
            &mut preset.game_dir,
            &mut preset.profile,
            &mut preset.output,
        ]
        .into_iter()
        .flatten()
        {
            if value.is_relative() {
                *value = parent.join(&*value);
            }
        }
    }
    let mut out = Args {
        command,
        preset,
        report: None,
        overwrite: false,
        maps: Vec::new(),
        preset_file,
    };
    let mut iter = args.into_iter().skip(1);
    while let Some(arg) = iter.next() {
        let text = arg.to_str().context("argument is not Unicode")?;
        match text {
            "--" => {
                out.maps.extend(
                    iter.map(|s| {
                        s.into_string()
                            .map_err(|_| anyhow::anyhow!("map is not Unicode"))
                    })
                    .collect::<Result<Vec<_>>>()?,
                );
                break;
            }
            "--strict" => out.preset.strict = true,
            "--no-dependencies" => out.preset.dependencies = false,
            "--overwrite" => out.overwrite = true,
            "--game-dir" | "--profile" | "--output" | "--report" | "--max-edge" | "--preset" => {
                let value = iter
                    .next()
                    .with_context(|| format!("{text} requires a value"))?;
                match text {
                    "--game-dir" => out.preset.game_dir = Some(value.into()),
                    "--profile" => out.preset.profile = Some(value.into()),
                    "--output" => out.preset.output = Some(value.into()),
                    "--report" => out.report = Some(value.into()),
                    "--max-edge" => {
                        out.preset.max_edge = value.to_str().context("max edge")?.parse()?
                    }
                    _ => {}
                }
            }
            _ if text.starts_with('-') => bail!("unknown headless option: {text}"),
            _ => out.maps.push(text.into()),
        }
    }
    ensure!(!out.maps.is_empty(), "at least one map is required");
    ensure!(out.preset.max_edge <= 16384, "max edge must be 0..16384");
    if let Some(path) = &out.preset.profile {
        let p = Profile::read(path)?;
        ensure!(
            out.preset.game_dir.as_ref().is_none_or(
                |d| std::fs::canonicalize(d).ok() == std::fs::canonicalize(&p.game_dir).ok()
            ),
            "--profile and --game-dir conflict"
        );
        out.preset.game_dir = Some(p.game_dir);
    }
    if out.preset.game_dir.is_none() {
        out.preset.game_dir = std::env::var_os("RA2_DIR").map(PathBuf::from);
    }
    ensure!(
        out.preset.game_dir.is_some(),
        "--game-dir, --profile or RA2_DIR is required"
    );
    ensure!(
        out.command == "validate" || out.preset.output.is_some(),
        "--output is required"
    );
    Ok(out)
}

fn output_path(args: &Args, name: &str) -> Result<PathBuf> {
    Ok(args
        .preset
        .output
        .as_ref()
        .context("output directory")?
        .join(Path::new(name).file_name().context("output name")?)
        .with_extension(if args.command == "export" {
            "png"
        } else {
            "map"
        }))
}
// Resolve aliases even when the destination or its parents do not exist yet.
fn path_identity(path: &Path) -> Result<String> {
    let absolute = std::path::absolute(path)?;
    let mut normalized = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(part.as_os_str()),
        }
    }
    let mut ancestor = normalized.as_path();
    let mut tail = Vec::new();
    while !ancestor.exists() {
        tail.push(ancestor.file_name().context("destination file name")?);
        ancestor = ancestor.parent().context("destination parent")?;
    }
    let mut resolved = std::fs::canonicalize(ancestor)?;
    for part in tail.into_iter().rev() {
        resolved.push(part);
    }
    let key = resolved.to_string_lossy().into_owned();
    Ok(if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    })
}
fn check_paths(args: &Args) -> Result<()> {
    let mut inputs = Vec::new();
    for name in &args.maps {
        inputs.push(path_identity(Path::new(name))?);
        if Path::new(name).components().count() == 1 {
            inputs.push(path_identity(
                &args.preset.game_dir.as_ref().unwrap().join(name),
            )?);
        }
    }
    for path in [&args.preset_file, &args.preset.profile]
        .into_iter()
        .flatten()
    {
        inputs.push(path_identity(path)?);
    }
    let mut writes = std::collections::BTreeSet::new();
    let mut check = |path: &Path| -> Result<()> {
        let id = path_identity(path)?;
        ensure!(
            !inputs.contains(&id),
            "destination cannot overwrite input/profile/preset: {}",
            path.display()
        );
        ensure!(
            writes.insert(id),
            "destinations collide: {}",
            path.display()
        );
        Ok(())
    };
    if args.command != "validate" {
        for name in &args.maps {
            let path = output_path(args, name)?;
            check(&path)?;
            check(&crate::provenance::sidecar(&path))?;
            if args.command == "export" {
                check(&path.with_extension("png.tmp"))?;
            }
        }
    }
    if let Some(report) = &args.report {
        check(report)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct Issue {
    severity: &'static str,
    code: String,
    location: String,
    message: String,
}
#[derive(Serialize)]
struct MapReport {
    input: String,
    passed: bool,
    issues: Vec<Issue>,
    dependencies: Option<crate::campaign::Report>,
    output: Option<PathBuf>,
    elapsed_ms: f64,
}
#[derive(Serialize)]
struct BatchReport {
    schema_version: u32,
    command: String,
    editor_version: &'static str,
    profile: Option<Revision>,
    preset: Preset,
    passed: bool,
    error: Option<String>,
    maps: Vec<MapReport>,
}
fn run_map(args: &Args, install: &mut Install, name: &str) -> Result<MapReport> {
    let started = std::time::Instant::now();
    let scene = map::load_scene(install, name)?;
    let base = scene
        .source
        .edit_base
        .as_ref()
        .map_err(|e| anyhow::anyhow!(e.clone()))?;
    let mut doc = MapDocument::new(base.clone());
    doc.resource_profile = Some(scene.revision.id());
    let mut issues: Vec<_> = mission::diagnostics(&doc, Some(&scene.mission_definitions))
        .into_iter()
        .map(|d| Issue {
            severity: match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Capability => "capability",
            },
            code: "MISSION".into(),
            location: format!("{:?}/{}/{}", d.issue.kind, d.issue.id, d.field),
            message: d.issue.message,
        })
        .collect();
    for d in &scene.diagnostics {
        issues.push(Issue {
            severity: match d.code {
                "EDITOR_PREVIEW" => "capability",
                "MISSING_TILE"
                | "INVALID_TILE_ID"
                | "NO_RENDERABLE_TERRAIN"
                | "UNSUPPORTED_THEATER" => "error",
                _ => "warning",
            },
            code: d.code.into(),
            location: format!("{:?}", d.cell),
            message: d.message.clone(),
        });
    }
    let file = Path::new(name)
        .file_name()
        .context("map file name")?
        .to_string_lossy();
    let dependencies = if args.preset.dependencies {
        Some(crate::campaign::audit_map(
            scene.source.bytes(),
            &file,
            install,
        )?)
    } else {
        None
    };
    let mut passed = !issues
        .iter()
        .any(|d| d.severity == "error" || (args.preset.strict && d.severity == "warning"))
        && dependencies
            .as_ref()
            .is_none_or(|r| r.passed && (!args.preset.strict || r.warnings.is_empty()));
    let mut output = None;
    if passed && args.command != "validate" {
        ensure!(!scene.images.is_empty(), "no renderable terrain");
        let path = output_path(args, name)?;
        ensure!(
            args.overwrite || (!path.exists() && !crate::provenance::sidecar(&path).exists()),
            "output already exists: {}",
            path.display()
        );
        ensure!(
            std::fs::canonicalize(name)
                .ok()
                .is_none_or(|input| std::fs::canonicalize(&path).ok().as_ref() != Some(&input)),
            "output cannot overwrite input map"
        );
        if args.command == "export" {
            let rect = scene.visual.bounds.context("empty scene")?;
            let size = if args.preset.max_edge == 0 {
                [rect.width().ceil() as u32, rect.height().ceil() as u32]
            } else {
                crate::publication::fit(rect, args.preset.max_edge)
            };
            ensure!(
                u64::from(size[0]) * u64::from(size[1]) <= 64 * 1024 * 1024,
                "PNG exceeds 64 Mi pixel budget; set --max-edge"
            );
            let rgba = scene.visual.raster(rect, size, ViewOptions::default())?;
            let temp = path.with_extension("png.tmp");
            image::RgbaImage::from_raw(size[0], size[1], rgba)
                .context("raster size")?
                .save_with_format(&temp, image::ImageFormat::Png)?;
            std::fs::rename(temp, &path)?;
            crate::provenance::write(
                &crate::provenance::sidecar(&path),
                scene.source.bytes(),
                &scene.revision,
                None,
            )?;
        } else {
            let bytes = crate::publication::embed_scene_preview(&scene)?;
            crate::editor_io::validate(&bytes)?;
            crate::editor_io::replace_map(&path, &bytes, crate::editor_io::Stamp::file(&path)?)?;
            crate::provenance::write(
                &crate::provenance::sidecar(&path),
                &bytes,
                &scene.revision,
                None,
            )?;
        }
        output = Some(path);
    }
    if !passed {
        issues.push(Issue {
            severity: "error",
            code: "VALIDATION_FAILED".into(),
            location: String::new(),
            message: "Inspect diagnostics and unresolved dependencies; no output was generated."
                .into(),
        });
    }
    // Keep the branch explicit for consumers: capability notes alone never fail.
    passed &= !issues.iter().any(|i| i.severity == "error");
    Ok(MapReport {
        input: name.into(),
        passed,
        issues,
        dependencies,
        output,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.,
    })
}

/// None means normal GUI arguments. A headless invocation never initializes UI.
pub fn run(arguments: impl IntoIterator<Item = OsString>) -> Result<Option<i32>> {
    let raw: Vec<_> = arguments.into_iter().collect();
    if raw
        .first()
        .is_some_and(|a| a == "mission-init" || a == "mission-build")
    {
        return crate::mission_script::run_cli(&raw).map(Some);
    }
    if !raw
        .first()
        .is_some_and(|a| a == "validate" || a == "export" || a == "publish")
    {
        return Ok(None);
    }
    if raw
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--help" || a == "-h")
    {
        println!("{HELP}");
        return Ok(Some(0));
    }
    let args = parse(raw)?;
    check_paths(&args)?;
    let mut report = BatchReport {
        schema_version: 1,
        command: args.command.clone(),
        editor_version: env!("CARGO_PKG_VERSION"),
        profile: None,
        preset: args.preset.clone(),
        passed: false,
        error: None,
        maps: Vec::new(),
    };
    let result = (|| -> Result<()> {
        if let Some(output) = &args.preset.output {
            std::fs::create_dir_all(output)?;
        }
        let mut names = std::collections::BTreeSet::new();
        for name in &args.maps {
            let stem = Path::new(name)
                .file_stem()
                .context("map name")?
                .to_string_lossy()
                .to_lowercase();
            ensure!(
                args.command == "validate" || names.insert(stem),
                "duplicate output map name"
            );
        }
        let mut install = Install::open(args.preset.game_dir.as_ref().unwrap())?;
        report.profile = Some((*install.revision).clone());
        for name in &args.maps {
            report.maps.push(match run_map(&args, &mut install, name) {
                Ok(map) => map,
                Err(e) => MapReport {
                    input: name.clone(),
                    passed: false,
                    issues: vec![Issue {
                        severity: "error",
                        code: "LOAD_OR_IO".into(),
                        location: String::new(),
                        message: format!("{e:#}"),
                    }],
                    dependencies: None,
                    output: None,
                    elapsed_ms: 0.,
                },
            });
        }
        report.passed = report.maps.iter().all(|m| m.passed);
        Ok(())
    })();
    if let Err(e) = result {
        report.error = Some(format!("{e:#}"));
    }
    let text = serde_json::to_string_pretty(&report)?;
    if let Some(path) = &args.report {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &text)?;
    }
    println!("{text}");
    Ok(Some(if report.error.is_some() {
        1
    } else if report.passed {
        0
    } else {
        2
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_protect_sources_reports_presets_and_output_aliases() {
        let parse_str =
            |s: &str| parse(s.split_whitespace().map(OsString::from).collect()).unwrap();
        for command in [
            "validate --game-dir . --report source.map source.map",
            "publish --game-dir . --overwrite --output . source.map",
            "export --game-dir . --output out --report out/source.png source.map",
            "export --game-dir . --output out --report out/source.png.yr-editor.json source.map",
            "publish --game-dir . --output out a/source.map b/source.map",
        ] {
            assert!(check_paths(&parse_str(command)).is_err(), "{command}");
        }
        assert!(
            check_paths(&parse_str(
                "export --game-dir . --output out --report report.json source.map"
            ))
            .is_ok()
        );
    }
    #[test]
    fn parser_rejects_unknown_flags_and_requires_input_and_output() {
        let parse_str = |s: &str| parse(s.split_whitespace().map(OsString::from).collect());
        assert!(parse_str("validate --game-dir . --mistake map.map").is_err());
        assert!(parse_str("export --game-dir . map.map").is_err());
        assert!(parse_str("validate --game-dir .").is_err());
        let a = parse_str("validate --game-dir . --strict -- --map.map").unwrap();
        assert!(a.preset.strict);
        assert_eq!(a.maps, ["--map.map"]);
    }
}
