//! Invoke the project's npm build package through the system Node.js runtime.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, ensure};

const BUILDER: &str = "@ra2-map-editor/mission-wasm-build";

pub(super) fn compile_module(project: &Path, wasm: &Path) -> Result<()> {
    compile_using(project, wasm, |command| command.output())
}

fn node_command() -> Command {
    let mut command = Command::new("node");
    command.env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn check_node_version(version: &str) -> Result<()> {
    let parts: Vec<_> = version
        .trim()
        .strip_prefix('v')
        .unwrap_or_default()
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<_, _>>()
        .unwrap_or_default();
    ensure!(
        parts.len() == 3 && (parts[0], parts[1]) >= (22, 15),
        "Node.js 22.15 or newer is required (found {}). Install a supported version and restart the editor.",
        version.trim()
    );
    Ok(())
}

/// A joint child resolves its dependencies from the extension root, never from
/// the editor checkout, a global npm installation, or an unrelated ancestor.
fn dependency_root(project: &Path) -> Result<PathBuf> {
    if matches!(
        project.file_name().and_then(|name| name.to_str()),
        Some("rules" | "ui")
    ) && let Some(parent) = project.parent()
    {
        let manifest = parent.join("package.json");
        match fs::read(&manifest) {
            Ok(bytes) => {
                let package: serde_json::Value = serde_json::from_slice(&bytes)
                    .with_context(|| format!("Invalid {}", manifest.display()))?;
                ensure!(package.is_object(), "package.json must be a JSON object");
                if let Some(marker) = package.get("vera") {
                    ensure!(
                        marker.is_object() && marker["kind"] == "extension",
                        "package.json vera must be an object with kind extension"
                    );
                    return Ok(parent.to_owned());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Read extension package.json"),
        }
    }
    Ok(project.to_owned())
}

fn builder_script(root: &Path) -> Result<PathBuf> {
    let package = root.join("node_modules").join(BUILDER);
    let install = format!("Run npm install in {}", node_path(root).display());
    let bytes = fs::read(package.join("package.json"))
        .with_context(|| format!("Project build dependencies are missing. {install}"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("Invalid {BUILDER} package.json. {install}"))?;
    ensure!(
        manifest["name"] == BUILDER,
        "Invalid build package. {install}"
    );
    let entry = manifest["bin"]["ra2-mission-project"]
        .as_str()
        .context("The installed build package has no ra2-mission-project entry")?;
    let package = package
        .canonicalize()
        .context("Open project build package")?;
    let script = package
        .join(entry)
        .canonicalize()
        .with_context(|| format!("The project build command is missing. {install}"))?;
    ensure!(
        script.starts_with(&package) && script.is_file(),
        "Invalid project build command. {install}"
    );
    Ok(script)
}

// Node's module resolver rejects Windows verbatim prefixes even when Rust can
// open the path. Normalize only process arguments; keep Rust filesystem paths
// canonical, and preserve non-Unicode path components.
fn node_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        if let Some(Component::Prefix(prefix)) = components.next() {
            let mut result = match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut result = PathBuf::from(r"\\");
                    result.push(server);
                    result.push(share);
                    result
                }
                _ => return path.to_owned(),
            };
            result.extend(components);
            return result;
        }
    }
    path.to_owned()
}

fn compile_using(
    project: &Path,
    wasm: &Path,
    mut run: impl FnMut(&mut Command) -> io::Result<Output>,
) -> Result<()> {
    let mut version_command = node_command();
    version_command.arg("--version");
    let version = run(&mut version_command).context(
        "Cannot start Node.js. Install Node.js 22.15 or newer, ensure node is on PATH, and restart the editor",
    )?;
    ensure!(
        version.status.success(),
        "Cannot check Node.js version: {}",
        String::from_utf8_lossy(&version.stderr).trim()
    );
    check_node_version(&String::from_utf8_lossy(&version.stdout))?;

    let root = dependency_root(project)?;
    let script = builder_script(&root)?;
    let mut command = node_command();
    command
        .current_dir(node_path(&root))
        .arg(node_path(&script))
        .arg(node_path(project))
        .arg(node_path(wasm));
    let output = run(&mut command).context("Start the project extension compiler")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!(
            "Extension build failed ({}):\n{}",
            output.status,
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn output(code: u32, stdout: &str, stderr: &str) -> Output {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        #[cfg(windows)]
        let status = std::process::ExitStatus::from_raw(code);
        #[cfg(unix)]
        let status = std::process::ExitStatus::from_raw((code as i32) << 8);
        Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn install_builder(root: &Path, entry: &str) -> PathBuf {
        let package = root.join("node_modules").join(BUILDER);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": BUILDER, "bin": { "ra2-mission-project": entry }
            }))
            .unwrap(),
        )
        .unwrap();
        let script = package.join(entry);
        fs::write(&script, "// test build command").unwrap();
        script.canonicalize().unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn node_paths_support_canonical_disks_unc_and_unicode() {
        for (input, expected) in [
            (
                r"\\?\C:\扩展 project\compile.mjs",
                r"C:\扩展 project\compile.mjs",
            ),
            (
                r"\\?\UNC\server\share\project\out.wasm",
                r"\\server\share\project\out.wasm",
            ),
            (r"C:\ordinary path\out.wasm", r"C:\ordinary path\out.wasm"),
        ] {
            assert_eq!(node_path(Path::new(input)), Path::new(expected));
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires system Node.js; run explicitly alongside the editor build smoke"]
    fn canonical_windows_paths_start_the_real_project_build_command() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("扩展 project with spaces");
        fs::create_dir(&root).unwrap();
        install_builder(&root, "compile.mjs");
        let script = root.join("node_modules").join(BUILDER).join("compile.mjs");
        fs::write(
            script,
            r#"
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
assert(!process.argv[1].startsWith('\\\\?\\'));
assert(!process.argv[2].startsWith('\\\\?\\'));
assert(!process.argv[3].startsWith('\\\\?\\'));
writeFileSync(process.argv[3], process.argv[2]);
"#,
        )
        .unwrap();
        let root = root.canonicalize().unwrap();
        let output = root.join("result.wasm");
        compile_module(&root, &output).unwrap();
        assert_eq!(
            fs::read_to_string(output).unwrap(),
            node_path(&root).to_str().unwrap()
        );
    }

    #[test]
    fn node_versions_follow_the_project_minimum() {
        for version in ["v22.15.0", "v22.15.1\n", "v22.99.0", "v23.0.0", "v24.0.0"] {
            assert!(check_node_version(version).is_ok(), "{version}");
        }
        for version in [
            "v20.19.0",
            "v22.14.9",
            "v1.99.0",
            "22.15.0",
            "v22.15",
            "v22.15.0-rc.1",
            "",
            "invalid",
        ] {
            assert!(check_node_version(version).is_err(), "{version}");
        }
    }

    #[test]
    fn only_project_local_builder_is_used_with_literal_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("扩展 project ; & spaces");
        fs::create_dir(&root).unwrap();
        let script = install_builder(&root, "compile.mjs");
        let wasm = root.join("dist/custom output.wasm");
        let mut calls = 0;
        compile_using(&root, &wasm, |command| {
            calls += 1;
            assert_eq!(command.get_program(), "node");
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == "NODE_OPTIONS" && value.is_none())
            );
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == "NODE_PATH" && value.is_none())
            );
            if calls == 1 {
                assert_eq!(
                    command.get_args().collect::<Vec<_>>(),
                    [OsStr::new("--version")]
                );
                return Ok(output(0, "v22.15.0", ""));
            }
            assert_eq!(command.get_current_dir(), Some(node_path(&root).as_path()));
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                [
                    node_path(&script).as_os_str(),
                    node_path(&root).as_os_str(),
                    node_path(&wasm).as_os_str()
                ]
            );
            Ok(output(0, "built", ""))
        })
        .unwrap();
        assert_eq!(calls, 2);
    }

    #[test]
    fn joint_children_use_root_dependencies_and_standalone_modules_do_not_use_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("package.json"),
            r#"{"vera":{"kind":"extension"}}"#,
        )
        .unwrap();
        for name in ["rules", "ui"] {
            let child = temp.path().join(name);
            assert_eq!(dependency_root(&child).unwrap(), temp.path());
        }
        fs::write(
            temp.path().join("package.json"),
            r#"{"name":"ordinary-parent"}"#,
        )
        .unwrap();
        for name in ["rules", "ui", "standalone"] {
            let child = temp.path().join(name);
            assert_eq!(dependency_root(&child).unwrap(), child);
        }
        let child = temp.path().join("rules");
        install_builder(temp.path(), "compile.mjs");
        assert!(builder_script(&dependency_root(&child).unwrap()).is_err());
    }

    #[test]
    fn both_joint_children_run_the_root_build_package() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(
            root.join("package.json"),
            r#"{"vera":{"kind":"extension"}}"#,
        )
        .unwrap();
        let script = install_builder(root, "compile.mjs");
        for name in ["rules", "ui"] {
            let child = root.join(name);
            let wasm = root.join(format!("dist/{name}.wasm"));
            let mut calls = 0;
            compile_using(&child, &wasm, |command| {
                calls += 1;
                if calls == 1 {
                    return Ok(output(0, "v24.0.0", ""));
                }
                assert_eq!(command.get_current_dir(), Some(node_path(root).as_path()));
                assert_eq!(
                    command.get_args().collect::<Vec<_>>(),
                    [
                        node_path(&script).as_os_str(),
                        node_path(&child).as_os_str(),
                        node_path(&wasm).as_os_str()
                    ]
                );
                Ok(output(0, "", ""))
            })
            .unwrap();
            assert_eq!(calls, 2);
        }
    }

    #[test]
    fn missing_dependencies_show_install_directory_and_do_not_fall_back_to_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let mut calls = 0;
        let error = compile_using(temp.path(), &temp.path().join("out.wasm"), |_| {
            calls += 1;
            Ok(output(0, "v24.0.0", ""))
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        let error = format!("{error:#}");
        assert!(error.contains("npm install"));
        assert!(error.contains(&temp.path().display().to_string()));
    }

    #[test]
    fn missing_or_unsupported_node_reports_installation_without_running_compiler() {
        let temp = tempfile::tempdir().unwrap();
        let error = compile_using(temp.path(), &temp.path().join("out.wasm"), |_| {
            Err(io::Error::new(io::ErrorKind::NotFound, "node missing"))
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("Install Node.js 22.15"));
        let error = compile_using(temp.path(), &temp.path().join("out.wasm"), |_| {
            Ok(output(0, "v20.0.0", ""))
        })
        .unwrap_err();
        assert!(error.to_string().contains("Node.js 22.15"));
    }

    #[test]
    fn invalid_parent_marker_and_invalid_package_entry_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("package.json"), r#"{"vera":null}"#).unwrap();
        assert!(dependency_root(&temp.path().join("rules")).is_err());
        let package = temp.path().join("node_modules").join(BUILDER);
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), r#"{"name":"wrong-package"}"#).unwrap();
        assert!(builder_script(temp.path()).is_err());
        fs::write(
            package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": BUILDER, "bin": { "ra2-mission-project": "../outside.mjs" }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(package.parent().unwrap().join("outside.mjs"), "").unwrap();
        assert!(builder_script(temp.path()).is_err());
    }

    #[test]
    fn compiler_failures_keep_stderr_or_fall_back_to_stdout() {
        let temp = tempfile::tempdir().unwrap();
        install_builder(temp.path(), "compile.mjs");
        for (stdout, stderr, expected) in [
            ("", "compile error", "compile error"),
            ("stdout failure", "", "stdout failure"),
        ] {
            let mut calls = 0;
            let error = compile_using(temp.path(), &temp.path().join("out.wasm"), |_| {
                calls += 1;
                Ok(if calls == 1 {
                    output(0, "v24.0.0", "")
                } else {
                    output(1, stdout, stderr)
                })
            })
            .unwrap_err();
            assert!(error.to_string().contains(expected));
        }
    }
}
