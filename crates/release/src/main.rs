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
    let rust_platform = format!("{os}-{arch}");
    let platform = variable("RA2_RELEASE_PLATFORM").unwrap_or_else(|| rust_platform.clone());
    ensure!(
        platform == rust_platform,
        "release platform {platform} does not match Rust target platform {rust_platform}"
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

    // Every platform gets an editor-only ZIP. Keep the raw executable
    // separately for existing launchers and update manifests.
    let zip_name = format!("{stem}.zip");
    let zip_path = staging.path().join(&zip_name);
    write_package_zip(&zip_path, &executable, suffix)?;

    let mut manifest_path = None;
    if stable && suffix == ".exe" {
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
        manifest_path = Some(path);
    }

    fs::rename(&executable, dist.join(&name))?;
    fs::rename(&zip_path, dist.join(&zip_name))?;
    if let Some(path) = manifest_path {
        fs::rename(path, dist.join("manifest.json"))?;
    }
    println!(
        "Wrote {} and {}",
        dist.join(&name).display(),
        dist.join(&zip_name).display()
    );
    Ok(())
}

fn write_package_zip(path: &Path, executable: &Path, suffix: &str) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(path)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let executable_name = if suffix == ".exe" {
        "ra2-map-editor.exe"
    } else {
        "ra2-map-editor"
    };
    zip.start_file(
        executable_name,
        file_options(options, &fs::metadata(executable)?),
    )?;
    io::copy(&mut File::open(executable)?, &mut zip)?;
    zip.start_file("README.md", options)?;
    let readme = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("README.md");
    io::copy(&mut File::open(readme)?, &mut zip)?;
    zip.finish()?;
    Ok(())
}

fn file_options(options: SimpleFileOptions, metadata: &fs::Metadata) -> SimpleFileOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        options.unix_permissions(metadata.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        options
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    const BINARY: &[u8] = b"editor binary";

    fn assert_editor_package(platform: &str, suffix: &str, stable: bool) {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join(format!("editor{suffix}"));
        fs::write(&binary, BINARY).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let dist = root.path().join("dist");

        package(&binary, &dist, platform, suffix, stable).unwrap();

        let label = if stable { "" } else { "dev-" };
        let stem = format!("ra2-map-editor-{label}{platform}");
        let raw_name = format!("{stem}{suffix}");
        let zip_name = format!("{stem}.zip");
        assert_eq!(fs::read(dist.join(&raw_name)).unwrap(), BINARY);
        assert_eq!(fs::read(&binary).unwrap(), BINARY);
        let mut expected_files = vec![raw_name.clone(), zip_name.clone()];
        if stable && suffix == ".exe" {
            expected_files.push("manifest.json".to_owned());
            let manifest: Value =
                serde_json::from_slice(&fs::read(dist.join("manifest.json")).unwrap()).unwrap();
            let digest = format!("sha256:{:x}", Sha256::digest(BINARY));
            let legacy_name = format!("ra2-map-editor-v{VERSION}-{platform}{suffix}");
            assert_eq!(
                manifest,
                json!({
                    "schema": 1,
                    "releases": [{ "version": VERSION, "assets": [
                        { "url": raw_name, "name": raw_name, "digest": digest },
                        { "url": raw_name, "name": legacy_name, "digest": digest },
                    ] }],
                })
            );
        }
        expected_files.sort();
        let mut actual_files: Vec<_> = fs::read_dir(&dist)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        actual_files.sort();
        assert_eq!(actual_files, expected_files);

        let file = File::open(dist.join(zip_name)).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let executable_name = format!("ra2-map-editor{suffix}");
        let mut entries: Vec<_> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        entries.sort();
        assert_eq!(entries, ["README.md".to_owned(), executable_name.clone()]);
        let mut executable = archive.by_name(&executable_name).unwrap();
        assert!(!executable.is_dir());
        let mut bytes = Vec::new();
        executable.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, BINARY);
        #[cfg(unix)]
        assert_eq!(executable.unix_mode().unwrap() & 0o777, 0o755);
        drop(executable);
        let mut readme = archive.by_name("README.md").unwrap();
        let mut bytes = Vec::new();
        readme.read_to_end(&mut bytes).unwrap();
        let readme_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
        assert_eq!(bytes, fs::read(readme_path).unwrap());
    }

    #[test]
    fn packages_stable_windows_editor_and_update_manifest() {
        assert_editor_package("windows-x86_64", ".exe", true);
    }

    #[test]
    fn packages_development_windows_editor_without_manifest() {
        assert_editor_package("windows-x86_64", ".exe", false);
    }

    #[test]
    fn packages_stable_linux_editor() {
        assert_editor_package("linux-x86_64", "", true);
    }

    #[test]
    fn packages_development_linux_editor() {
        assert_editor_package("linux-x86_64", "", false);
    }

    #[test]
    fn packages_stable_macos_aarch64_editor() {
        assert_editor_package("macos-aarch64", "", true);
    }

    #[test]
    fn packages_development_macos_aarch64_editor() {
        assert_editor_package("macos-aarch64", "", false);
    }

    #[test]
    fn packages_stable_macos_x86_64_editor() {
        assert_editor_package("macos-x86_64", "", true);
    }

    #[test]
    fn packages_development_macos_x86_64_editor() {
        assert_editor_package("macos-x86_64", "", false);
    }

    #[test]
    fn ignores_existing_toolchain_and_unrelated_dist_files() {
        let root = tempfile::tempdir().unwrap();
        let binary = root.path().join("editor.exe");
        fs::write(&binary, BINARY).unwrap();
        let dist = root.path().join("dist");
        let old_toolchain = dist.join("mission-toolchain");
        fs::create_dir_all(&old_toolchain).unwrap();
        fs::write(old_toolchain.join("toolchain.json"), b"not valid JSON").unwrap();
        fs::write(dist.join("unrelated.txt"), b"preserve").unwrap();

        package(&binary, &dist, "windows-x86_64", ".exe", false).unwrap();

        assert_eq!(
            fs::read(old_toolchain.join("toolchain.json")).unwrap(),
            b"not valid JSON"
        );
        assert_eq!(fs::read(dist.join("unrelated.txt")).unwrap(), b"preserve");
        let file = File::open(dist.join("ra2-map-editor-dev-windows-x86_64.zip")).unwrap();
        let archive = ZipArchive::new(file).unwrap();
        let mut entries: Vec<_> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        entries.sort();
        assert_eq!(entries, ["README.md", "ra2-map-editor.exe"]);
        assert_eq!(fs::read_dir(&dist).unwrap().count(), 4);
    }

    #[test]
    fn missing_binary_does_not_publish_partial_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let dist = root.path().join("dist");
        let error = package(
            &root.path().join("missing.exe"),
            &dist,
            "windows-x86_64",
            ".exe",
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("package"));
        assert_eq!(fs::read_dir(dist).unwrap().count(), 0);
    }
}
