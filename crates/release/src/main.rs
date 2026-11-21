use std::env;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn output(command: &mut Command) -> Result<Output> {
    let result = command.output()?;
    ensure!(
        result.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(result)
}

fn variable(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    env::set_current_dir(root)?;
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let target = variable("RA2_RELEASE_TARGET").or_else(|| variable("CARGO_BUILD_TARGET"));
    let version = semver::Version::parse(VERSION)?;
    let tag = variable("RA2_RELEASE_TAG").unwrap_or_else(|| format!("v{version}"));
    let development = tag.strip_prefix("dev-").is_some_and(|short_sha| {
        short_sha.len() == 7 && short_sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    ensure!(
        development
            || (tag == format!("v{version}") && version.pre.is_empty() && version.build.is_empty()),
        "release tag {tag} must match a stable Cargo version, found {version}"
    );

    let mut rustc = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--print", "cfg"]);
    if let Some(target) = &target {
        rustc.args(["--target", target]);
    }
    let cfg = String::from_utf8(output(&mut rustc)?.stdout)?;
    let value = |key: &str| {
        cfg.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.trim_matches('"'))
            .with_context(|| format!("Rust target has no {key}"))
    };
    let os = value("target_os")?;
    let arch = value("target_arch")?;
    ensure!(
        matches!(os, "windows" | "macos" | "linux"),
        "unsupported platform: {os}-{arch}"
    );
    let suffix = if os == "windows" { ".exe" } else { "" };

    let mut command = Command::new(&cargo);
    command.args(["build", "--locked", "--release", "-p", "ra2-map-editor"]);
    if let Some(target) = &target {
        command.args(["--target", target]);
    }
    ensure!(command.status()?.success(), "release build failed");

    let metadata: Value = serde_json::from_slice(
        &output(Command::new(cargo).args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ]))?
        .stdout,
    )?;
    let mut directory = PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .context("Cargo target directory is missing")?,
    );
    if let Some(target) = target {
        directory.push(target);
    }
    let binary = directory
        .join("release")
        .join(format!("ra2-map-editor{suffix}"));
    let platform = format!("{os}-{arch}");
    package(&binary, Path::new("dist"), &platform, suffix, !development)?;
    Ok(())
}

fn package(binary: &Path, dist: &Path, platform: &str, suffix: &str, stable: bool) -> Result<()> {
    fs::create_dir_all(dist)?;
    let staging = tempfile::tempdir_in(dist)?;
    let stem = if stable {
        format!("ra2-map-editor-{platform}")
    } else {
        format!("ra2-map-editor-dev-{platform}")
    };
    let name = format!("{stem}{suffix}");
    let executable = staging.path().join(&name);
    fs::copy(binary, &executable).with_context(|| format!("package {}", binary.display()))?;

    if suffix == ".exe" {
        let zip_name = format!("{stem}.zip");
        let zip_path = staging.path().join(&zip_name);
        let mut zip = ZipWriter::new(File::create(&zip_path)?);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (path, name) in [
            (&executable, "ra2-map-editor.exe"),
            (&PathBuf::from("README.md"), "README.md"),
        ] {
            zip.start_file(name, options)?;
            io::copy(&mut File::open(path)?, &mut zip)?;
        }
        zip.finish()?;
        fs::rename(zip_path, dist.join(zip_name))?;

        if stable {
            let mut sha = Sha256::new();
            io::copy(&mut File::open(&executable)?, &mut sha)?;
            let digest = format!("sha256:{:x}", sha.finalize());
            // Older updaters match the versioned name. Both entries download the same EXE.
            let legacy_name = format!("ra2-map-editor-v{VERSION}-{platform}{suffix}");
            let manifest = json!({
                "schema": 1,
                "releases": [{ "version": VERSION, "assets": [
                    { "url": name, "name": name, "digest": digest },
                    { "url": name, "name": legacy_name, "digest": digest },
                ] }],
            });
            let path = staging.path().join("manifest.json");
            fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
            fs::rename(&executable, dist.join(&name))?;
            fs::rename(path, dist.join("manifest.json"))?;
            println!(
                "Wrote {} and {}",
                dist.join(&name).display(),
                dist.join("manifest.json").display()
            );
            return Ok(());
        }
    }
    fs::rename(executable, dist.join(&name))?;
    println!("Wrote {}", dist.join(name).display());
    Ok(())
}
