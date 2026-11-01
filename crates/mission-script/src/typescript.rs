//! In-process TypeScript transpilation. Type checking belongs to the external IDE.
use anyhow::{Result, bail, ensure};
use boa_engine::{Context, Source};
use std::collections::{BTreeMap, BTreeSet};
use swc_common::{FileName, GLOBALS, Globals, Mark, SourceMap, Spanned, sync::Lrc};
use swc_ecma_ast::{EsVersion, ModuleDecl, ModuleItem, Program, TsModuleRef};
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_transforms_base::{
    fixer::fixer,
    helpers::{HELPERS, Helpers, inject_helpers},
    hygiene::hygiene,
    resolver,
};
use swc_ecma_transforms_module::common_js::{FeatureFlag, common_js};
use swc_ecma_transforms_typescript::strip;

struct Transpiled {
    source: String,
    javascript: String,
    imports: Vec<String>,
}

/// Reuses emitted modules on a background thread; each changed build executes
/// in a fresh Boa realm with no filesystem, network or process bindings.
pub struct Compiler {
    modules: BTreeMap<String, Transpiled>,
    inputs: BTreeMap<String, String>,
    cached_mission: Option<String>,
}

impl Compiler {
    pub fn new() -> Result<Self> {
        Ok(Self {
            modules: BTreeMap::from([("/sdk.ts".into(), transpile("/sdk.ts", super::SDK)?)]),
            inputs: BTreeMap::new(),
            cached_mission: None,
        })
    }

    /// Only syntax is checked here. SDK data and native map constraints are
    /// validated separately; type annotations are checked by the author's IDE.
    pub fn build(&mut self, mut files: BTreeMap<String, String>) -> Result<super::native::Mission> {
        ensure!(files.contains_key("/mission.ts"), "缺少 mission.ts");
        ensure!(files.len() <= 128, "任务模块不能超过 128 个");
        ensure!(
            files.values().map(String::len).sum::<usize>() <= 4 * 1024 * 1024,
            "任务源码超过 4 MiB"
        );
        for name in files.keys() {
            ensure!(
                name.starts_with('/')
                    && name.ends_with(".ts")
                    && !name.contains('\\')
                    && name[1..]
                        .split('/')
                        .all(|part| !matches!(part, "" | "." | "..")),
                "无效的任务模块路径：{name}"
            );
        }
        // The SDK is immutable, even if a caller supplies its on-disk copy.
        files.remove("/sdk.ts");
        if files == self.inputs
            && let Some(text) = &self.cached_mission
        {
            return Ok(serde_json::from_str(text)?);
        }
        self.cached_mission = None;
        self.modules
            .retain(|name, _| name == "/sdk.ts" || files.contains_key(name));

        // Traverse runtime imports after type stripping. Declaration files,
        // type-only imports and unrelated scripts do not need transpilation.
        let mut pending = vec!["/mission.ts".to_owned()];
        let mut visited = BTreeSet::new();
        let mut outputs = BTreeMap::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let source = if name == "/sdk.ts" {
                super::SDK
            } else {
                files
                    .get(&name)
                    .map(String::as_str)
                    .ok_or_else(|| anyhow::anyhow!("找不到任务模块：{name}"))?
            };
            if self
                .modules
                .get(&name)
                .is_none_or(|module| module.source != source)
            {
                self.modules.remove(&name);
                self.modules.insert(name.clone(), transpile(&name, source)?);
            }
            let module = &self.modules[&name];
            for import in &module.imports {
                let resolved = resolve(import, &name)?;
                ensure!(
                    resolved == "/sdk.ts"
                        || (files.contains_key(&resolved) && !resolved.ends_with(".d.ts")),
                    "{name}：找不到可执行任务模块 {import}；类型请使用 import type 导入"
                );
                pending.push(resolved);
            }
            outputs.insert(
                name.strip_suffix(".ts").unwrap().to_owned() + ".js",
                module.javascript.clone(),
            );
        }

        let text = execute(&outputs)?;
        let mission =
            serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("任务结构无效：{e}"))?;
        self.inputs = files;
        self.cached_mission = Some(text);
        Ok(mission)
    }
}

fn resolve(specifier: &str, parent: &str) -> Result<String> {
    ensure!(
        specifier.starts_with("./") || specifier.starts_with("../"),
        "{parent}：只支持项目内相对导入：{specifier}"
    );
    ensure!(
        !specifier.contains('\\'),
        "{parent}：导入路径须使用 /：{specifier}"
    );
    let mut parts: Vec<_> = parent.split('/').collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "." | "" => (),
            ".." => {
                ensure!(parts.len() > 1, "{parent}：导入超出任务目录：{specifier}");
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let path = parts.join("/");
    Ok(if path.ends_with(".ts") {
        path
    } else if let Some(path) = path.strip_suffix(".js") {
        format!("{path}.ts")
    } else {
        format!("{path}.ts")
    })
}

fn transpile(name: &str, source: &str) -> Result<Transpiled> {
    // SourceMap and GLOBALS are local to this call, so changing scripts cannot
    // accumulate old ASTs, source text or hygiene marks in a persistent worker.
    let cm: Lrc<SourceMap> = Default::default();
    let file = cm.new_source_file(FileName::Custom(name.into()).into(), source.to_owned());
    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax::default()),
        EsVersion::EsNext,
        StringInput::from(&*file),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let parsed = parser.parse_module();
    let mut errors = parser.take_errors();
    if let Err(error) = &parsed {
        errors.push(error.clone());
    }
    if !errors.is_empty() {
        bail!(
            errors
                .iter()
                .map(|error| {
                    let loc = cm.lookup_char_pos(error.span().lo);
                    format!(
                        "{name}:{}:{} 语法错误：{}",
                        loc.line,
                        loc.col_display + 1,
                        error.kind().msg()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    GLOBALS.set(&Globals::default(), || {
        HELPERS.set(&Helpers::new(false), || {
            let unresolved = Mark::new();
            let top_level = Mark::new();
            let program = Program::Module(parsed.unwrap())
                .apply(resolver(unresolved, top_level, true))
                .apply(strip(unresolved, top_level));
            let imports = match &program {
                Program::Module(module) => module
                    .body
                    .iter()
                    .filter_map(|item| {
                        let ModuleItem::ModuleDecl(decl) = item else {
                            return None;
                        };
                        let source = match decl {
                            ModuleDecl::Import(import) => Some(&*import.src),
                            ModuleDecl::ExportAll(export) => Some(&*export.src),
                            ModuleDecl::ExportNamed(export) => export.src.as_deref(),
                            ModuleDecl::TsImportEquals(import) => match &import.module_ref {
                                TsModuleRef::TsExternalModuleRef(module) => Some(&module.expr),
                                _ => None,
                            },
                            _ => None,
                        }?;
                        Some(source.value.to_string_lossy().into_owned())
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let program = program
                .apply(common_js(
                    Default::default(),
                    unresolved,
                    Default::default(),
                    FeatureFlag {
                        support_block_scoping: true,
                        support_arrow: true,
                    },
                ))
                .apply(inject_helpers(unresolved))
                .apply(hygiene())
                .apply(fixer(None));
            Ok(Transpiled {
                source: source.into(),
                imports,
                javascript: swc_ecma_codegen::to_code_default(cm, None, &program),
            })
        })
    })
}

fn execute(outputs: &BTreeMap<String, String>) -> Result<String> {
    let mut execution = Context::default();
    execution
        .runtime_limits_mut()
        .set_loop_iteration_limit(100_000);
    execution.runtime_limits_mut().set_recursion_limit(128);
    execution
        .eval(Source::from_bytes(include_str!("compiler.js")))
        .map_err(|e| anyhow::anyhow!("任务执行器：{e}"))?;
    execution.eval(Source::from_bytes("Date = undefined; Math.random = () => { throw new Error('Use explicit build-time constants instead of Math.random'); };"))
        .map_err(|e| anyhow::anyhow!("任务执行器：{e}"))?;
    let value = execution
        .eval(Source::from_bytes(&format!(
            "__yrExecute({})",
            serde_json::to_string(outputs)?
        )))
        .map_err(|e| anyhow::anyhow!("任务执行失败：{e}"))?;
    let text = value
        .as_string()
        .ok_or_else(|| anyhow::anyhow!("mission.ts 必须 default export defineMission(...)"))?
        .to_std_string_escaped();
    ensure!(text.len() <= 4 * 1024 * 1024, "生成的任务超过 4 MiB");
    Ok(text)
}
