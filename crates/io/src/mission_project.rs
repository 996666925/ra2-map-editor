//! Scaffolds engine SDK projects and builds their configured Wasm module.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::wasm_mission::{self, ModuleKind};
use anyhow::{Context, Result, ensure};

mod compiler;
use compiler::compile_module;

macro_rules! template {
    ($file:literal) => {
        include_bytes!(concat!("../../create-vera-app/template/", $file))
    };
}

/// 一个扩展工程可以包含同步逻辑、本机界面，或同时包含两种模块。
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    #[default]
    Extension,
    Rules,
    Ui,
}

impl ProjectKind {
    pub fn modules(self) -> &'static [ModuleKind] {
        match self {
            Self::Extension => &[ModuleKind::Rules, ModuleKind::Ui],
            Self::Rules => &[ModuleKind::Rules],
            Self::Ui => &[ModuleKind::Ui],
        }
    }
}

impl From<ModuleKind> for ProjectKind {
    fn from(kind: ModuleKind) -> Self {
        match kind {
            ModuleKind::Rules => Self::Rules,
            ModuleKind::Ui => Self::Ui,
        }
    }
}

/// 创建同时包含 rules、ui 和共享源码的 AssemblyScript 扩展工程。
/// 已有目录及用户文件保持原样。
pub fn create_project(target: &Path) -> Result<()> {
    let files: &[(&str, &[u8])] = &[
        (".gitignore", template!("_gitignore")),
        ("README.md", template!("README.md")),
        ("package.json", template!("extension/_package.json")),
        ("tsconfig.json", template!("extension/tsconfig.json")),
        ("rules/vera.json", template!("extension/rules/vera.json")),
        (
            "rules/src/main.ts",
            template!("extension/rules/src/main.ts"),
        ),
        ("ui/vera.json", template!("extension/ui/vera.json")),
        ("ui/src/main.ts", template!("extension/ui/src/main.ts")),
        (
            "shared/protocol.ts",
            template!("extension/shared/protocol.ts"),
        ),
    ];
    // The root is created exclusively; cleanup can never remove an existing project.
    fs::create_dir(target).with_context(|| target.display().to_string())?;
    let result = (|| {
        for (name, bytes) in files {
            let path = target.join(name);
            fs::create_dir_all(path.parent().context("Missing template directory")?)?;
            fs::File::create_new(path)?.write_all(bytes)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(target);
    }
    result
}

#[derive(Debug, PartialEq, Eq)]
struct ProjectConfig {
    config_bytes: Vec<u8>,
    kind: ModuleKind,
}

impl ProjectConfig {
    fn read(project: &Path) -> Result<Self> {
        let config_bytes = fs::read(project.join("vera.json")).context("Missing vera.json")?;
        let config: serde_json::Value =
            serde_json::from_slice(&config_bytes).context("Invalid vera.json")?;
        let kind = match config["kind"].as_str() {
            Some("rules") => ModuleKind::Rules,
            Some("ui") => ModuleKind::Ui,
            _ => anyhow::bail!("vera.json kind must be rules or ui"),
        };
        Ok(Self { config_bytes, kind })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ExtensionConfig {
    modules: Vec<(PathBuf, ProjectConfig)>,
    build_configs: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl ExtensionConfig {
    fn read(project: &Path) -> Result<Self> {
        let package_path = project.join("package.json");
        let extension = match fs::read(&package_path) {
            Ok(bytes) => {
                let package: serde_json::Value =
                    serde_json::from_slice(&bytes).context("Invalid package.json")?;
                ensure!(package.is_object(), "package.json must be a JSON object");
                if let Some(config) = package.get("vera") {
                    ensure!(
                        config.is_object() && config["kind"] == "extension",
                        "package.json vera must be an object with kind extension"
                    );
                    true
                } else {
                    false
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error).context("Read package.json"),
        };
        let modules = if extension {
            ensure!(
                !project.join("vera.json").try_exists()?,
                "An extension root must not contain vera.json"
            );
            let mut modules = Vec::new();
            for kind in [ModuleKind::Rules, ModuleKind::Ui] {
                let path = project.join(kind.as_str());
                let resolved = path
                    .canonicalize()
                    .context("Open extension module directory")?;
                ensure!(
                    resolved.starts_with(project),
                    "Extension module directory must stay inside the project"
                );
                ensure!(
                    !resolved.join("package.json").try_exists()?,
                    "Extension modules must use the root package.json"
                );
                let config = ProjectConfig::read(&resolved)?;
                ensure!(
                    config.kind == kind,
                    "Extension module kind does not match its directory"
                );
                modules.push((resolved, config));
            }
            modules
        } else {
            vec![(project.to_owned(), ProjectConfig::read(project)?)]
        };
        let mut build_configs = Vec::new();
        for directory in
            std::iter::once(project).chain(modules.iter().map(|(path, _)| path.as_path()))
        {
            for name in ["package.json", "tsconfig.json", "asconfig.json"] {
                let path = directory.join(name);
                let bytes = match fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error).with_context(|| path.display().to_string()),
                };
                build_configs.push((path, bytes));
            }
        }
        Ok(Self {
            modules,
            build_configs,
        })
    }

    fn kind(&self) -> ProjectKind {
        if self.modules.len() == 2 {
            ProjectKind::Extension
        } else {
            self.modules[0].1.kind.into()
        }
    }
}

/// 读取联合或单模块项目类型。
pub fn project_type(project: &Path) -> Result<ProjectKind> {
    let project = project
        .canonicalize()
        .context("Open the extension project directory")?;
    Ok(ExtensionConfig::read(&project)?.kind())
}

/// 从引擎 SDK 配置读取并验证模块类型。
pub fn project_kind(project: &Path) -> Result<ModuleKind> {
    Ok(ProjectConfig::read(project)?.kind)
}

/// 构建结果保留启动时捕获的模块类型，供调用方选择正确的地图区块。
#[derive(Debug)]
pub struct BuiltModule {
    pub kind: ModuleKind,
    pub path: PathBuf,
}

/// 使用系统 Node.js 和项目中安装的构建器编译模块。
pub fn build_project(project: &Path) -> Result<BuiltModule> {
    build_project_using(project, compile_module)
}

/// 在同一构建中校验全部模块，全部成功后才更新最终产物。
pub fn build_modules(project: &Path) -> Result<Vec<BuiltModule>> {
    build_modules_using(project, compile_module)
}

fn build_project_using(
    project: &Path,
    compile: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<BuiltModule> {
    let mut compile = Some(compile);
    ensure!(
        project_type(project)? != ProjectKind::Extension,
        "Use build_modules for a combined extension project"
    );
    Ok(build_modules_using(project, |project, wasm| {
        compile.take().context("Unexpected second module")?(project, wasm)
    })?
    .remove(0))
}

fn build_modules_using(
    project: &Path,
    mut compile: impl FnMut(&Path, &Path) -> Result<()>,
) -> Result<Vec<BuiltModule>> {
    let project = project
        .canonicalize()
        .context("Open the extension project directory")?;
    let config = ExtensionConfig::read(&project)?;
    let directory = project.join("dist");
    fs::create_dir_all(&directory)?;
    let workspace = tempfile::tempdir_in(&directory)?;
    let mut modules = Vec::new();
    for (path, config) in &config.modules {
        let wasm = workspace.path().join(config.kind.file_name());
        compile(path, &wasm)?;
        wasm_mission::verify_kind(config.kind, &wasm_mission::read_module(&wasm)?)?;
        modules.push(BuiltModule {
            kind: config.kind,
            path: directory.join(config.kind.file_name()),
        });
    }
    ensure!(
        ExtensionConfig::read(&project)? == config,
        "Extension project configuration changed during build; build again"
    );
    // Keep previous outputs for rollback if publishing a later file fails.
    let mut previous = Vec::new();
    for module in &modules {
        let backup = workspace
            .path()
            .join(format!("{}.previous", module.kind.as_str()));
        let existed = match fs::symlink_metadata(&module.path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file(),
                    "Extension output must be a regular file: {}",
                    module.path.display()
                );
                fs::copy(&module.path, &backup).context("Back up extension output")?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error).context("Read previous extension output"),
        };
        previous.push(existed.then_some(backup));
    }
    for (index, module) in modules.iter().enumerate() {
        if let Err(error) = fs::rename(workspace.path().join(module.kind.file_name()), &module.path)
        {
            let mut failures = Vec::new();
            for (module, backup) in modules[..index].iter().zip(&previous).rev() {
                let restored = if let Some(backup) = backup {
                    fs::rename(backup, &module.path)
                } else {
                    fs::remove_file(&module.path)
                };
                if let Err(error) = restored {
                    failures.push(format!("{}: {error}", module.path.display()));
                }
            }
            if !failures.is_empty() {
                let recovery = workspace.keep();
                return Err(error).context(format!(
                    "Publish failed; rollback failed: {}. Backups retained in {}",
                    failures.join("; "),
                    recovery.display()
                ));
            }
            return Err(error).context("Publish extension output");
        }
    }
    Ok(modules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(kind: ModuleKind) -> &'static [u8] {
        // Real ABI-specific host imports, with one () -> i32 function type.
        match kind {
            ModuleKind::Rules => b"\0asm\x01\0\0\0\x01\x05\x01\x60\0\x01\x7f\x02\x16\x01\x0cvera:sim@1.0\x05frame\0\0",
            ModuleKind::Ui => b"\0asm\x01\0\0\0\x01\x05\x01\x60\0\x01\x7f\x02\x20\x01\x0fvera:client@1.0\x0cclient_frame\0\0",
        }
    }

    // 单模块布局仅用于验证已有工程和子模块的编译，不再提供创建入口。
    fn single_module_fixture(project: &Path, kind: ModuleKind) {
        fs::create_dir_all(project).unwrap();
        fs::write(project.join("vera.json"), serde_json::to_vec(&serde_json::json!({
            "kind": kind.as_str(), "entry": "src/main.ts", "out": format!("dist/{}", kind.file_name())
        })).unwrap()).unwrap();
    }

    fn write_kind(project: &Path, kind: &str) {
        let path = project.join("vera.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["kind"] = kind.into();
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }

    #[test]
    fn project_kind_uses_only_the_engine_module_config() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        assert!(project_kind(&project).is_err());
        single_module_fixture(&project, ModuleKind::Rules);
        assert_eq!(project_kind(&project).unwrap(), ModuleKind::Rules);
        write_kind(&project, "typo");
        assert!(
            project_kind(&project)
                .unwrap_err()
                .to_string()
                .contains("kind")
        );
        write_kind(&project, "ui");
        assert_eq!(project_kind(&project).unwrap(), ModuleKind::Ui);
        assert_eq!(project_type(&project).unwrap(), ProjectKind::Ui);
    }

    #[test]
    fn project_type_rejects_invalid_package_markers_without_falling_back() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        single_module_fixture(&project, ModuleKind::Rules);
        for value in [
            "[]",
            "null",
            "{",
            r#"{"vera":null}"#,
            r#"{"vera":true}"#,
            r#"{"vera":"extension"}"#,
            r#"{"vera":{"kind":"rules"}}"#,
            r#"{"vera":{}}"#,
        ] {
            fs::write(project.join("package.json"), value).unwrap();
            assert!(project_type(&project).is_err(), "{value}");
            assert!(
                build_modules_using(&project, |_, _| panic!("must validate before compiling"))
                    .is_err()
            );
        }
        fs::write(project.join("package.json"), b"{}").unwrap();
        assert_eq!(project_type(&project).unwrap(), ProjectKind::Rules);
    }

    #[test]
    fn build_captures_kind_and_promotes_only_validated_matching_artifact() {
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("项目 with spaces");
            single_module_fixture(&project, kind);
            let other = if kind == ModuleKind::Rules {
                ModuleKind::Ui
            } else {
                ModuleKind::Rules
            };
            fs::create_dir(project.join("dist")).unwrap();
            fs::write(
                project.join("dist").join(other.file_name()),
                b"keep other module",
            )
            .unwrap();
            fs::write(
                project.join("dist").join(kind.file_name()),
                b"previous artifact",
            )
            .unwrap();
            let result = build_project_using(&project, |_, wasm| {
                assert_eq!(wasm.file_name().unwrap(), kind.file_name());
                fs::write(wasm, module(kind))?;
                Ok(())
            })
            .unwrap();
            assert_eq!(result.kind, kind);
            assert_eq!(
                result.path,
                project
                    .canonicalize()
                    .unwrap()
                    .join("dist")
                    .join(kind.file_name())
            );
            assert_eq!(fs::read(result.path).unwrap(), module(kind));
            assert_eq!(
                fs::read(project.join("dist").join(other.file_name())).unwrap(),
                b"keep other module"
            );
            assert_eq!(fs::read_dir(project.join("dist")).unwrap().count(), 2);
        }
    }

    #[test]
    fn failed_or_concurrently_reconfigured_build_preserves_both_artifacts() {
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            for failure in [
                "kind",
                "package",
                "entry",
                "invalid",
                "wrong-abi",
                "missing",
                "compiler",
            ] {
                let root = tempfile::tempdir().unwrap();
                let project = root.path().join("project");
                single_module_fixture(&project, kind);
                fs::create_dir(project.join("dist")).unwrap();
                for name in ["rules.wasm", "ui.wasm"] {
                    fs::write(project.join("dist").join(name), name.as_bytes()).unwrap();
                }
                let result = build_project_using(&project, |project, wasm| {
                    fs::write(wasm, module(kind))?;
                    match failure {
                        "kind" => write_kind(
                            project,
                            if kind == ModuleKind::Rules {
                                "ui"
                            } else {
                                "rules"
                            },
                        ),
                        "package" => fs::write(
                            project.join("package.json"),
                            br#"{"vera":{"kind":"extension"}}"#,
                        )?,
                        "entry" => {
                            let mut config: serde_json::Value =
                                serde_json::from_slice(&fs::read(project.join("vera.json"))?)?;
                            config["entry"] = "src/changed.ts".into();
                            fs::write(project.join("vera.json"), serde_json::to_vec(&config)?)?;
                        }
                        "invalid" => fs::write(wasm, b"invalid Wasm")?,
                        "wrong-abi" => fs::write(
                            wasm,
                            module(if kind == ModuleKind::Rules {
                                ModuleKind::Ui
                            } else {
                                ModuleKind::Rules
                            }),
                        )?,
                        "missing" => fs::remove_file(wasm)?,
                        "compiler" => anyhow::bail!("compiler failed"),
                        _ => unreachable!(),
                    }
                    Ok(())
                });
                assert!(result.is_err(), "{kind:?}: {failure}");
                for name in ["rules.wasm", "ui.wasm"] {
                    assert_eq!(
                        fs::read(project.join("dist").join(name)).unwrap(),
                        name.as_bytes()
                    );
                }
                assert_eq!(fs::read_dir(project.join("dist")).unwrap().count(), 2);
            }
        }
    }

    #[test]
    fn combined_project_has_one_package_two_modules_and_shared_sources() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("联合项目 with spaces");
        create_project(&project).unwrap();
        assert_eq!(project_type(&project).unwrap(), ProjectKind::Extension);
        assert!(!project.join("vera.json").exists());
        assert!(project.join("shared/protocol.ts").is_file());
        let package: serde_json::Value =
            serde_json::from_slice(&fs::read(project.join("package.json")).unwrap()).unwrap();
        assert_eq!(package["scripts"]["build"], "ra2-mission-project .");
        assert_eq!(package["vera"], serde_json::json!({"kind": "extension"}));
        for directory in [project.clone(), project.join("rules"), project.join("ui")] {
            assert!(!directory.join("vera-mission.json").exists());
        }
        assert_eq!(package["devDependencies"]["assemblyscript"], "0.28.20");
        assert_eq!(
            package["devDependencies"]["@ra2-map-editor/mission-wasm-build"],
            "0.1.3"
        );
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let path = project.join(kind.as_str());
            assert_eq!(project_kind(&path).unwrap(), kind);
            assert!(path.join("src/main.ts").is_file());
            assert!(!path.join("package.json").exists());
            assert!(!path.join("vendor").exists());
        }
        fs::write(project.join("shared/protocol.ts"), "user source").unwrap();
        assert!(create_project(&project).is_err());
        assert_eq!(
            fs::read_to_string(project.join("shared/protocol.ts")).unwrap(),
            "user source"
        );
    }

    #[test]
    fn combined_build_publishes_both_validated_artifacts() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("联合项目 with spaces");
        create_project(&project).unwrap();
        let mut compiled = Vec::new();
        let built = build_modules_using(&project, |path, output| {
            let kind = project_kind(path)?;
            compiled.push(kind);
            fs::write(output, module(kind))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(compiled, [ModuleKind::Rules, ModuleKind::Ui]);
        assert_eq!(built.len(), 2);
        for built in built {
            assert_eq!(fs::read(built.path).unwrap(), module(built.kind));
        }
        assert_eq!(fs::read_dir(project.join("dist")).unwrap().count(), 2);
    }

    #[test]
    fn combined_build_failures_leave_both_previous_outputs_untouched() {
        for failure in [
            "compiler",
            "invalid",
            "wrong-abi",
            "root-config",
            "rules-config",
            "ui-config",
            "package.json",
            "tsconfig.json",
            "rules/asconfig.json",
            "ui/tsconfig.json",
        ] {
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("extension");
            create_project(&project).unwrap();
            fs::create_dir(project.join("dist")).unwrap();
            for kind in [ModuleKind::Rules, ModuleKind::Ui] {
                fs::write(project.join("dist").join(kind.file_name()), kind.as_str()).unwrap();
            }
            let result = build_modules_using(&project, |path, output| {
                let kind = project_kind(path)?;
                fs::write(output, module(kind))?;
                if kind == ModuleKind::Ui {
                    match failure {
                        "compiler" => anyhow::bail!("second compiler failed"),
                        "invalid" => fs::write(output, b"invalid")?,
                        "wrong-abi" => fs::write(output, module(ModuleKind::Rules))?,
                        "root-config" => fs::write(
                            project.join("package.json"),
                            br#"{"vera":{"kind":"invalid"}}"#,
                        )?,
                        "rules-config" => fs::write(
                            project.join("rules/vera.json"),
                            br#"{"kind":"rules","entry":"changed.ts"}"#,
                        )?,
                        "ui-config" => fs::write(
                            project.join("ui/vera.json"),
                            br#"{"kind":"ui","entry":"changed.ts"}"#,
                        )?,
                        name @ ("package.json"
                        | "tsconfig.json"
                        | "rules/asconfig.json"
                        | "ui/tsconfig.json") => fs::write(project.join(name), b"{}")?,
                        _ => unreachable!(),
                    }
                }
                Ok(())
            });
            assert!(result.is_err(), "{failure}");
            for kind in [ModuleKind::Rules, ModuleKind::Ui] {
                assert_eq!(
                    fs::read(project.join("dist").join(kind.file_name())).unwrap(),
                    kind.as_str().as_bytes(),
                    "{failure}"
                );
            }
            assert_eq!(fs::read_dir(project.join("dist")).unwrap().count(), 2);
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_second_publication_restores_both_previous_outputs() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("extension");
        create_project(&project).unwrap();
        fs::create_dir(project.join("dist")).unwrap();
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            fs::write(project.join("dist").join(kind.file_name()), kind.as_str()).unwrap();
        }
        // Allow reading the old UI output for backup, but deny its replacement.
        let _locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(project.join("dist/ui.wasm"))
            .unwrap();
        let error = build_modules_using(&project, |path, output| {
            fs::write(output, module(project_kind(path)?))?;
            Ok(())
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("Publish extension output"),
            "{error:#}"
        );
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            assert_eq!(
                fs::read(project.join("dist").join(kind.file_name())).unwrap(),
                kind.as_str().as_bytes()
            );
        }
        assert_eq!(fs::read_dir(project.join("dist")).unwrap().count(), 2);
    }

    #[test]
    fn combined_project_rejects_missing_or_mismatched_modules_before_compiling() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("extension");
        create_project(&project).unwrap();
        write_kind(&project.join("ui"), "rules");
        assert!(project_type(&project).is_err());
        assert!(
            build_modules_using(&project, |_, _| panic!("must validate before compiling")).is_err()
        );
        fs::remove_file(project.join("ui/vera.json")).unwrap();
        assert!(project_type(&project).is_err());
    }
}
