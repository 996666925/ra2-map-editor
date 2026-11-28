//! Editor transactions and CLI for embedded simulation and client Wasm modules.
use crate::editor_io::{Stamp, replace_map_with_resources};
use anyhow::{Context, Result, anyhow, ensure};
use ra2_map_assets::map_resources::MapResources;
use ra2_map_document::document::{MapDocument, PreparedReplacement, Snapshot};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::Path,
};
use vera20k_core::{
    map::ext_module_pack as pack,
    rules::ini_parser::IniFile,
    util::{ini_writer, version},
};
use vera20k_wasm_ext::{ClientModule, Limits, SimModule};

// One shared version record survives until the last embedded module is removed.
const VERSION_ADDED: &str = "EditorWasmMinVersion";
const VERSION_LEGACY: &str = "EditorRulesMinVersion";
const VERSION_PREVIOUS: &str = "EditorPreviousMinVersion";
const MIN_VERSION: &str = "VeraMinVersion";

/// 引擎支持的地图扩展类型；逻辑与本机界面分别使用独立 ABI。
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    #[default]
    Rules,
    Ui,
}

impl ModuleKind {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Rules => "rules.wasm",
            Self::Ui => "ui.wasm",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Ui => "ui",
        }
    }

    fn section(self) -> &'static str {
        match self {
            Self::Rules => pack::RULES_PACK,
            Self::Ui => pack::UI_PACK,
        }
    }

    fn size_key(self) -> &'static str {
        match self {
            Self::Rules => pack::RULES_SIZE,
            Self::Ui => pack::UI_SIZE,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Rules => Self::Ui,
            Self::Ui => Self::Rules,
        }
    }

    fn extract(self, ini: &IniFile) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Rules => pack::rules_module(ini),
            Self::Ui => pack::ui_module(ini),
        }
        .map_err(anyhow::Error::msg)
    }
}

pub const HELP: &str = "Wasm extensions:\n  wasm-build <extension-project>\n  wasm-check [--kind rules|ui] <module.wasm | map>\n  wasm-embed [--kind rules|ui] <map> <module.wasm> <out-map>\n  wasm-extract [--kind rules|ui] <map> <out.wasm>\n  wasm-remove [--kind rules|ui] <map> <out-map>\n\nModule kind defaults to rules; wasm-build uses the project configuration.";

pub fn read_module(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((pack::MAX_MODULE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= pack::MAX_MODULE_BYTES,
        "Wasm module exceeds 8 MiB"
    );
    Ok(bytes)
}

pub fn verify(module: &[u8]) -> Result<()> {
    verify_kind(ModuleKind::Rules, module)
}

/// 按模块类型使用引擎实际沙箱校验 ABI、内存与地图模块限制。
pub fn verify_kind(kind: ModuleKind, module: &[u8]) -> Result<()> {
    ensure!(
        module.len() <= pack::MAX_MODULE_BYTES,
        "{} exceeds 8 MiB",
        kind.file_name()
    );
    match kind {
        ModuleKind::Rules => SimModule::load(module, Limits::MAP).map(|_| ()),
        ModuleKind::Ui => ClientModule::load(module, Limits::MAP).map(|_| ()),
    }
    .map_err(|error| anyhow!("invalid {}: {error}", kind.file_name()))
}

pub fn extract(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    extract_kind(ModuleKind::Rules, bytes)
}

pub fn extract_kind(kind: ModuleKind, bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    kind.extract(&IniFile::from_bytes(bytes)?)
}

pub fn embed(bytes: &[u8], module: &[u8]) -> Result<Vec<u8>> {
    embed_kind(ModuleKind::Rules, bytes, module)
}

pub fn embed_kind(kind: ModuleKind, bytes: &[u8], module: &[u8]) -> Result<Vec<u8>> {
    verify_kind(kind, module)?;
    let ini = IniFile::from_bytes(bytes)?;
    // Api is shared: never rewrite an unsupported API underneath another module.
    if ini.section(kind.other().section()).is_some() {
        let api = ini
            .section_or_empty(pack::SECTION)
            .read_string("Api", "", 32);
        pack::check_api(&api)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("embedded {} API", kind.other().file_name()))?;
    }
    let current = ini
        .section_or_empty("Basic")
        .read_string(MIN_VERSION, "", 32);
    let raise_minimum = current.is_empty()
        || (current != version::GAME_VERSION
            && version::version_serves(version::GAME_VERSION, &current));
    if !raise_minimum && kind.extract(&ini).ok().flatten().as_deref() == Some(module) {
        return Ok(bytes.to_vec());
    }
    let api = format!("{}.{}", pack::API.0, pack::API.1);
    let size = module.len().to_string();
    let packed = pack::pack_lines(module);
    let values: Vec<_> = packed
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let bytes = ini_writer::replace_ini_section(bytes, kind.section(), &values);
    let mut bytes = ini_writer::set_ini_values(
        &bytes,
        pack::SECTION,
        &[("Api", &api), (kind.size_key(), &size)],
    );
    if raise_minimum {
        let meta = ini.section_or_empty(pack::SECTION);
        let mut added = meta.read_string(VERSION_ADDED, "", 32);
        if added.is_empty() {
            added = meta.read_string(VERSION_LEGACY, "", 32);
        }
        let saved_previous = meta.read_string(VERSION_PREVIOUS, "", 32);
        // An engine upgrade may raise our own old minimum again. Preserve the
        // original baseline, but honor a minimum the user changed since then.
        let previous = if !current.is_empty() && added == current && !saved_previous.is_empty() {
            saved_previous.as_str()
        } else if current.is_empty() {
            "none"
        } else {
            &current
        };
        bytes = ini_writer::remove_ini_keys(&bytes, pack::SECTION, &[VERSION_LEGACY]);
        bytes = ini_writer::set_ini_values(
            &bytes,
            pack::SECTION,
            &[
                (VERSION_ADDED, version::GAME_VERSION),
                (VERSION_PREVIOUS, previous),
            ],
        );
        bytes =
            ini_writer::set_ini_values(&bytes, "Basic", &[(MIN_VERSION, version::GAME_VERSION)]);
    }
    ensure!(
        extract_kind(kind, &bytes)?.as_deref() == Some(module),
        "embedded {} failed readback",
        kind.file_name()
    );
    Ok(bytes)
}

pub fn remove(bytes: &[u8]) -> Result<Vec<u8>> {
    remove_kind(ModuleKind::Rules, bytes)
}

pub fn remove_kind(kind: ModuleKind, bytes: &[u8]) -> Result<Vec<u8>> {
    let ini = IniFile::from_bytes(bytes)?;
    let meta = ini.section_or_empty(pack::SECTION);
    let mut added = meta.read_string(VERSION_ADDED, "", 32);
    if added.is_empty() {
        added = meta.read_string(VERSION_LEGACY, "", 32);
    }
    let previous = meta.read_string(VERSION_PREVIOUS, "", 32);
    let current = ini
        .section_or_empty("Basic")
        .read_string(MIN_VERSION, "", 32);
    let other_present = ini.section(kind.other().section()).is_some();
    let mut bytes =
        ini_writer::remove_ini_sections(bytes, |name| name.eq_ignore_ascii_case(kind.section()));
    let mut keys = vec![kind.size_key()];
    if !other_present {
        keys.extend(["Api", VERSION_ADDED, VERSION_LEGACY, VERSION_PREVIOUS]);
    }
    bytes = ini_writer::remove_ini_keys(&bytes, pack::SECTION, &keys);
    if !other_present && !added.is_empty() && added == current {
        bytes = ini_writer::remove_ini_keys(&bytes, "Basic", &[MIN_VERSION]);
        if !previous.is_empty() && previous != "none" {
            bytes = ini_writer::set_ini_values(&bytes, "Basic", &[(MIN_VERSION, &previous)]);
        }
    }
    Ok(bytes)
}

pub(crate) fn verify_map(ini: &IniFile) -> Result<()> {
    for kind in [ModuleKind::Rules, ModuleKind::Ui] {
        if let Some(module) = kind.extract(ini)? {
            verify_kind(kind, &module)?;
        }
    }
    Ok(())
}

pub fn prepare(
    snapshot: Snapshot,
    revision: u64,
    module: Option<&[u8]>,
) -> Result<PreparedReplacement> {
    prepare_kind(ModuleKind::Rules, snapshot, revision, module)
}

pub fn prepare_kind(
    kind: ModuleKind,
    snapshot: Snapshot,
    revision: u64,
    module: Option<&[u8]>,
) -> Result<PreparedReplacement> {
    prepare_modules(snapshot, revision, &[(kind, module)])
}

/// 将多个模块准备成同一地图事务；任一校验失败均不修改地图。
pub fn prepare_modules(
    snapshot: Snapshot,
    revision: u64,
    modules: &[(ModuleKind, Option<&[u8]>)],
) -> Result<PreparedReplacement> {
    ensure!(!modules.is_empty(), "No extension modules to update");
    let mut bytes = snapshot.bytes()?;
    for (index, (kind, module)) in modules.iter().enumerate() {
        ensure!(
            !modules[..index]
                .iter()
                .any(|(previous, _)| previous == kind),
            "Duplicate extension module kind"
        );
        bytes = match module {
            Some(module) => embed_kind(*kind, &bytes, module)?,
            None => remove_kind(*kind, &bytes)?,
        };
    }
    PreparedReplacement::prepare(snapshot, revision, bytes)
}

pub fn apply(doc: &mut MapDocument, module: Option<&[u8]>) -> Result<bool> {
    apply_kind(ModuleKind::Rules, doc, module)
}

pub fn apply_kind(kind: ModuleKind, doc: &mut MapDocument, module: Option<&[u8]>) -> Result<bool> {
    let prepared = prepare_kind(kind, doc.snapshot(), doc.edit_revision(), module)?;
    doc.replace_prepared(prepared, "Wasm extension")
}

fn write_map(input: &Path, output: &Path, bytes: &[u8], expected: Option<Stamp>) -> Result<()> {
    let resources = MapResources::read(input)?;
    replace_map_with_resources(output, bytes, expected, resources.as_ref(), true)?;
    Ok(())
}

fn cli_arguments(args: &[OsString]) -> Result<(Option<ModuleKind>, Vec<OsString>)> {
    let mut kind = None;
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            positional.extend(args.cloned());
            break;
        }
        let value = if arg == "--kind" {
            Some(
                args.next()
                    .and_then(|value| value.to_str())
                    .context("--kind requires rules or ui")?,
            )
        } else {
            arg.to_str().and_then(|arg| arg.strip_prefix("--kind="))
        };
        if let Some(value) = value {
            ensure!(kind.is_none(), "--kind must be specified only once");
            kind = Some(match value {
                "rules" => ModuleKind::Rules,
                "ui" => ModuleKind::Ui,
                _ => anyhow::bail!("--kind requires rules or ui"),
            });
        } else {
            ensure!(
                !arg.to_str().is_some_and(|arg| arg.starts_with("--")),
                "unknown option: {}",
                arg.to_string_lossy()
            );
            positional.push(arg.clone());
        }
    }
    Ok((kind, positional))
}

pub(crate) fn run_cli(args: &[OsString]) -> Result<Option<i32>> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(None);
    };
    if !matches!(
        command,
        "wasm-build" | "wasm-check" | "wasm-embed" | "wasm-extract" | "wasm-remove"
    ) {
        return Ok(None);
    }
    if args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("{HELP}");
        return Ok(Some(0));
    }
    let (explicit_kind, args) = cli_arguments(&args[1..])?;
    let kind = explicit_kind.unwrap_or_default();
    match (command, args.as_slice()) {
        ("wasm-build", [project]) => {
            ensure!(
                explicit_kind.is_none(),
                "wasm-build uses the project configuration; do not pass --kind"
            );
            for built in crate::mission_project::build_modules(Path::new(project))? {
                println!("{}", built.path.display());
            }
        }
        ("wasm-check", [input]) => {
            let bytes = crate::campaign::read_bounded(Path::new(input))?;
            let module = if bytes.starts_with(b"\0asm") {
                bytes
            } else {
                extract_kind(kind, &bytes)?
                    .with_context(|| format!("map has no {}", kind.file_name()))?
            };
            verify_kind(kind, &module)?;
            println!(
                "{}",
                serde_json::json!({"ok": true, "bytes": module.len(), "kind": kind.as_str()})
            );
        }
        ("wasm-embed", [map, module, out]) => {
            let map = Path::new(map);
            let out = Path::new(out);
            // Capture before reading: an external save must never be overwritten.
            let expected = Stamp::file(out)?;
            let input = crate::campaign::read_bounded(map)?;
            let bytes = embed_kind(kind, &input, &read_module(Path::new(module))?)?;
            write_map(map, out, &bytes, expected)?;
        }
        ("wasm-extract", [map, out]) => {
            let input = crate::campaign::read_bounded(Path::new(map))?;
            let module = extract_kind(kind, &input)?
                .with_context(|| format!("map has no {}", kind.file_name()))?;
            fs::File::create_new(out)?.write_all(&module)?;
        }
        ("wasm-remove", [map, out]) => {
            let map = Path::new(map);
            let out = Path::new(out);
            let expected = Stamp::file(out)?;
            let input = crate::campaign::read_bounded(map)?;
            write_map(map, out, &remove_kind(kind, &input)?, expected)?;
        }
        _ => anyhow::bail!("{HELP}"),
    }
    Ok(Some(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    const EMPTY: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn embedding_checks_module_and_preserves_other_fields() {
        let base = b"[Basic]\nVeraMinVersion=0.4.1\nName=Test\n[Other]\nKey=Keep\n";
        assert!(embed(base, b"not wasm").is_err());
        let embedded = embed(base, EMPTY).unwrap();
        assert_eq!(extract(&embedded).unwrap().as_deref(), Some(EMPTY));
        assert_eq!(embed(&embedded, EMPTY).unwrap(), embedded);
        let removed = remove(&embedded).unwrap();
        assert!(extract(&removed).unwrap().is_none());
        assert!(
            removed
                .windows(b"VeraMinVersion=0.4.1".len())
                .any(|v| v == b"VeraMinVersion=0.4.1")
        );
        assert!(removed.windows(b"Key=Keep".len()).any(|v| v == b"Key=Keep"));
    }

    #[test]
    fn reimport_raises_a_low_minimum_but_preserves_a_higher_one() {
        let original = embed(b"[Basic]\nName=Test\n", EMPTY).unwrap();
        let lowered = ini_writer::set_ini_values(&original, "Basic", &[(MIN_VERSION, "0.4.1")]);
        let changed = embed(&lowered, EMPTY).unwrap();
        let minimum = |bytes: &[u8]| {
            IniFile::from_bytes(bytes)
                .unwrap()
                .section_or_empty("Basic")
                .read_string(MIN_VERSION, "", 32)
        };
        assert_eq!(minimum(&changed), version::GAME_VERSION);
        assert_eq!(minimum(&remove(&changed).unwrap()), "0.4.1");
        let future = embed(b"[Basic]\nVeraMinVersion=999.0\n", EMPTY).unwrap();
        assert_eq!(minimum(&remove(&future).unwrap()), "999.0");
    }

    #[test]
    fn map_embedding_and_removal_are_undoable() {
        let mut doc = ra2_map_document::test_support::document();
        let original = doc.snapshot().bytes().unwrap();
        assert!(apply(&mut doc, Some(EMPTY)).unwrap());
        let embedded = doc.snapshot().bytes().unwrap();
        assert!(!apply(&mut doc, Some(EMPTY)).unwrap());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), original);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), embedded);
        assert!(apply(&mut doc, None).unwrap());
        assert!(extract(&doc.snapshot().bytes().unwrap()).unwrap().is_none());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), embedded);
    }

    #[test]
    fn cli_map_write_preserves_concurrent_changes_and_resource_sidecars() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input.map");
        let bytes = ra2_map_document::new_map::NewMapOptions::default()
            .bytes()
            .unwrap();
        fs::write(&input, &bytes).unwrap();
        let input_resources = MapResources {
            ares: ra2_map_assets::map_resources::AresMode::Off,
            ..Default::default()
        };
        input_resources.write(&input).unwrap();
        let changed = ini_writer::set_ini_values(&bytes, "Basic", &[("Name", "External save")]);
        let resources = MapResources {
            ares: ra2_map_assets::map_resources::AresMode::On,
            ..Default::default()
        };
        // Cover both an existing destination and a file created after the command
        // began, as well as an in-place update of the input map.
        for (name, existing) in [
            ("existing.map", true),
            ("new.map", false),
            ("input.map", true),
        ] {
            let output = root.path().join(name);
            if existing {
                fs::write(&output, &bytes).unwrap();
            }
            let expected = Stamp::file(&output).unwrap();
            fs::write(&output, &changed).unwrap();
            resources.write(&output).unwrap();
            assert!(write_map(&input, &output, &bytes, expected).is_err());
            assert_eq!(fs::read(&output).unwrap(), changed);
            assert_eq!(
                MapResources::read(&output).unwrap(),
                Some(resources.clone())
            );
        }
    }

    // A minimal binary with one () -> i32 host import; no WAT compiler dependency.
    fn importing(kind: ModuleKind) -> Vec<u8> {
        let (namespace, name) = match kind {
            ModuleKind::Rules => ("vera:sim@1.0", "frame"),
            ModuleKind::Ui => ("vera:client@1.0", "client_frame"),
        };
        let mut wasm = EMPTY.to_vec();
        wasm.extend_from_slice(&[1, 5, 1, 0x60, 0, 1, 0x7f]); // type: () -> i32
        let mut imports = vec![1, namespace.len() as u8];
        imports.extend_from_slice(namespace.as_bytes());
        imports.push(name.len() as u8);
        imports.extend_from_slice(name.as_bytes());
        imports.extend_from_slice(&[0, 0]); // function import, type 0
        wasm.extend_from_slice(&[2, imports.len() as u8]);
        wasm.extend(imports);
        wasm
    }

    fn ini_value(bytes: &[u8], section: &str, key: &str) -> String {
        IniFile::from_bytes(bytes)
            .unwrap()
            .section_or_empty(section)
            .read_string(key, "", 64)
    }

    #[test]
    fn kinds_use_distinct_real_engine_abis_and_map_limits() {
        assert_eq!(ModuleKind::default(), ModuleKind::Rules);
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let module = importing(kind);
            verify_kind(kind, &module).unwrap();
            assert!(verify_kind(kind.other(), &module).is_err());
            let mut excessive_memory = EMPTY.to_vec();
            // Memory minimum 257 pages exceeds Limits::MAP (256 pages).
            excessive_memory.extend_from_slice(&[5, 4, 1, 0, 0x81, 2]);
            assert!(verify_kind(kind, &excessive_memory).is_err());
        }
    }

    #[test]
    fn both_module_orders_preserve_packs_and_restore_shared_version_metadata() {
        for first in [ModuleKind::Rules, ModuleKind::Ui] {
            for removed_first in [ModuleKind::Rules, ModuleKind::Ui] {
                for minimum in ["", "0.4.1", version::GAME_VERSION, "999.0"] {
                    let base = ini_writer::set_ini_values(
                        b"[Basic]\nName=Keep\n[Other]\nKey=Keep\n",
                        "Basic",
                        &[(MIN_VERSION, minimum)],
                    );
                    let bytes = embed_kind(first, &base, &importing(first)).unwrap();
                    let both =
                        embed_kind(first.other(), &bytes, &importing(first.other())).unwrap();
                    for kind in [ModuleKind::Rules, ModuleKind::Ui] {
                        assert_eq!(extract_kind(kind, &both).unwrap(), Some(importing(kind)));
                        assert_eq!(embed_kind(kind, &both, &importing(kind)).unwrap(), both);
                    }
                    verify_map(&IniFile::from_bytes(&both).unwrap()).unwrap();
                    let remaining = remove_kind(removed_first, &both).unwrap();
                    assert!(extract_kind(removed_first, &remaining).unwrap().is_none());
                    assert_eq!(
                        extract_kind(removed_first.other(), &remaining).unwrap(),
                        Some(importing(removed_first.other()))
                    );
                    assert_eq!(ini_value(&remaining, pack::SECTION, "Api"), "1.0");
                    assert_eq!(
                        ini_value(&remaining, "Basic", MIN_VERSION),
                        ini_value(&both, "Basic", MIN_VERSION)
                    );
                    let removed = remove_kind(removed_first.other(), &remaining).unwrap();
                    assert_eq!(ini_value(&removed, "Basic", MIN_VERSION), minimum);
                    for key in [
                        "Api",
                        pack::RULES_SIZE,
                        pack::UI_SIZE,
                        VERSION_ADDED,
                        VERSION_LEGACY,
                        VERSION_PREVIOUS,
                    ] {
                        assert_eq!(ini_value(&removed, pack::SECTION, key), "", "{key}");
                    }
                    assert_eq!(ini_value(&removed, "Basic", "Name"), "Keep");
                    assert_eq!(ini_value(&removed, "Other", "Key"), "Keep");
                }
            }
        }
    }

    #[test]
    fn legacy_version_record_survives_removing_either_module_first() {
        for first in [ModuleKind::Rules, ModuleKind::Ui] {
            let rules = embed(
                b"[Basic]\nVeraMinVersion=0.4.1\n",
                &importing(ModuleKind::Rules),
            )
            .unwrap();
            let old = ini_writer::remove_ini_keys(&rules, pack::SECTION, &[VERSION_ADDED]);
            let old = ini_writer::set_ini_values(
                &old,
                pack::SECTION,
                &[(VERSION_LEGACY, version::GAME_VERSION)],
            );
            let both = embed_kind(ModuleKind::Ui, &old, &importing(ModuleKind::Ui)).unwrap();
            let remaining = remove_kind(first, &both).unwrap();
            assert_eq!(
                ini_value(&remaining, pack::SECTION, VERSION_LEGACY),
                version::GAME_VERSION
            );
            assert_eq!(
                ini_value(&remaining, pack::SECTION, VERSION_PREVIOUS),
                "0.4.1"
            );
            let removed = remove_kind(first.other(), &remaining).unwrap();
            assert_eq!(ini_value(&removed, "Basic", MIN_VERSION), "0.4.1");
            assert_eq!(ini_value(&removed, pack::SECTION, VERSION_LEGACY), "");
        }
    }

    #[test]
    fn missing_module_removal_keeps_other_module_and_user_minimum_changes() {
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let embedded = embed_kind(kind, b"[Basic]\n", &importing(kind)).unwrap();
            assert_eq!(remove_kind(kind.other(), &embedded).unwrap(), embedded);
            let changed = ini_writer::set_ini_values(&embedded, "Basic", &[(MIN_VERSION, "999.0")]);
            assert_eq!(
                ini_value(&remove_kind(kind, &changed).unwrap(), "Basic", MIN_VERSION),
                "999.0"
            );
        }
    }

    #[test]
    fn map_verification_rejects_wrong_client_abi_and_shared_api_is_not_downgraded() {
        let both = embed_kind(ModuleKind::Ui, &embed(b"[Basic]\n", EMPTY).unwrap(), EMPTY).unwrap();
        let invalid = importing(ModuleKind::Rules);
        let packed = pack::pack_lines(&invalid);
        let values: Vec<_> = packed
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        let corrupt = ini_writer::replace_ini_section(&both, pack::UI_PACK, &values);
        let corrupt = ini_writer::set_ini_values(
            &corrupt,
            pack::SECTION,
            &[(pack::UI_SIZE, &invalid.len().to_string())],
        );
        assert!(
            verify_map(&IniFile::from_bytes(&corrupt).unwrap())
                .unwrap_err()
                .to_string()
                .contains("ui.wasm")
        );
        assert_eq!(extract(&corrupt).unwrap().as_deref(), Some(EMPTY));
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let future = ini_writer::set_ini_values(&both, pack::SECTION, &[("Api", "2.0")]);
            assert!(embed_kind(kind, &future, EMPTY).is_err());
            let repaired = remove_kind(kind, &future).unwrap();
            assert_eq!(ini_value(&repaired, pack::SECTION, "Api"), "2.0");
            assert!(
                IniFile::from_bytes(&repaired)
                    .unwrap()
                    .section(kind.other().section())
                    .is_some()
            );
        }
    }

    #[test]
    fn both_modules_are_independently_undoable_and_stale_workers_cannot_overwrite_them() {
        let mut doc = ra2_map_document::test_support::document();
        let original = doc.snapshot().bytes().unwrap();
        let stale = prepare_kind(
            ModuleKind::Ui,
            doc.snapshot(),
            doc.edit_revision(),
            Some(&importing(ModuleKind::Ui)),
        )
        .unwrap();
        apply(&mut doc, Some(&importing(ModuleKind::Rules))).unwrap();
        let rules = doc.snapshot().bytes().unwrap();
        assert!(doc.replace_prepared(stale, "stale UI worker").is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), rules);
        assert!(
            apply_kind(
                ModuleKind::Ui,
                &mut doc,
                Some(&importing(ModuleKind::Rules))
            )
            .is_err()
        );
        assert_eq!(doc.snapshot().bytes().unwrap(), rules);
        apply_kind(ModuleKind::Ui, &mut doc, Some(&importing(ModuleKind::Ui))).unwrap();
        let both = doc.snapshot().bytes().unwrap();
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), rules);
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), original);
        assert!(doc.redo());
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), both);
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            apply_kind(kind, &mut doc, None).unwrap();
            assert_eq!(
                extract_kind(kind.other(), &doc.snapshot().bytes().unwrap()).unwrap(),
                Some(importing(kind.other()))
            );
            assert!(doc.undo());
            assert_eq!(doc.snapshot().bytes().unwrap(), both);
        }
    }

    #[test]
    fn cli_supports_ui_and_rules_without_overwriting_other_modules_or_extractions() {
        let root = tempfile::tempdir().unwrap();
        let map = root.path().join("双模块 map.map");
        fs::write(
            &map,
            ra2_map_document::new_map::NewMapOptions::default()
                .bytes()
                .unwrap(),
        )
        .unwrap();
        let run = |command: &str, arguments: Vec<OsString>| {
            let mut args = vec![command.into()];
            args.extend(arguments);
            run_cli(&args)
        };
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let module = root.path().join(kind.file_name());
            fs::write(&module, importing(kind)).unwrap();
            let mut args = vec![
                map.clone().into_os_string(),
                module.clone().into_os_string(),
                map.clone().into_os_string(),
            ];
            // Default rules remains compatible; options may follow paths.
            if kind == ModuleKind::Ui {
                args.extend(["--kind".into(), "ui".into()]);
            }
            assert_eq!(run("wasm-embed", args).unwrap(), Some(0));
            for input in [&map, &module] {
                assert_eq!(
                    run(
                        "wasm-check",
                        vec![
                            "--kind".into(),
                            kind.as_str().into(),
                            input.as_os_str().into()
                        ]
                    )
                    .unwrap(),
                    Some(0)
                );
            }
            assert!(
                run(
                    "wasm-check",
                    vec![
                        "--kind".into(),
                        kind.other().as_str().into(),
                        module.into_os_string()
                    ]
                )
                .is_err()
            );
            let extracted = root.path().join(format!("extracted-{}", kind.file_name()));
            let args = vec![
                map.clone().into_os_string(),
                extracted.clone().into_os_string(),
                format!("--kind={}", kind.as_str()).into(),
            ];
            assert_eq!(run("wasm-extract", args.clone()).unwrap(), Some(0));
            assert_eq!(fs::read(&extracted).unwrap(), importing(kind));
            assert!(run("wasm-extract", args).is_err());
            assert_eq!(fs::read(&extracted).unwrap(), importing(kind));
        }
        let both = fs::read(&map).unwrap();
        for first in [ModuleKind::Rules, ModuleKind::Ui] {
            fs::write(&map, &both).unwrap();
            for kind in [first, first.other()] {
                let args = vec![
                    "--kind".into(),
                    kind.as_str().into(),
                    map.clone().into_os_string(),
                    map.clone().into_os_string(),
                ];
                assert_eq!(run("wasm-remove", args).unwrap(), Some(0));
                assert!(
                    extract_kind(kind, &fs::read(&map).unwrap())
                        .unwrap()
                        .is_none()
                );
                if kind == first {
                    assert_eq!(
                        extract_kind(kind.other(), &fs::read(&map).unwrap()).unwrap(),
                        Some(importing(kind.other()))
                    );
                }
            }
        }
        fs::write(&map, &both).unwrap();
        let wrong = root.path().join("rules.wasm");
        assert!(
            run(
                "wasm-embed",
                vec![
                    "--kind".into(),
                    "ui".into(),
                    map.clone().into_os_string(),
                    wrong.into_os_string(),
                    map.clone().into_os_string()
                ]
            )
            .is_err()
        );
        assert_eq!(fs::read(&map).unwrap(), both);
    }

    #[test]
    fn cli_rejects_invalid_duplicate_and_missing_kind_options() {
        for args in [
            vec!["--kind"],
            vec!["--kind", "typo"],
            vec!["--kind", "ui", "--kind", "rules"],
            vec!["--unknown", "rules"],
        ] {
            let args: Vec<_> = args.into_iter().map(OsString::from).collect();
            assert!(cli_arguments(&args).is_err());
        }
        let args = ["--", "--kind", "ui"].map(OsString::from);
        assert_eq!(
            cli_arguments(&args).unwrap(),
            (None, vec!["--kind".into(), "ui".into()])
        );
        let args = ["wasm-build", "--kind", "ui", "project"].map(OsString::from);
        assert!(
            run_cli(&args)
                .unwrap_err()
                .to_string()
                .contains("project configuration")
        );
    }

    #[test]
    fn upgrading_an_editor_managed_minimum_retains_the_original_baseline() {
        for marker in [VERSION_LEGACY, VERSION_ADDED] {
            for first in [ModuleKind::Rules, ModuleKind::Ui] {
                for baseline in ["none", "0.0.0"] {
                    let bytes = embed_kind(first, b"[Basic]\n", &importing(first)).unwrap();
                    let bytes = ini_writer::remove_ini_keys(
                        &bytes,
                        pack::SECTION,
                        &[VERSION_ADDED, VERSION_LEGACY],
                    );
                    let bytes =
                        ini_writer::set_ini_values(&bytes, "Basic", &[(MIN_VERSION, "0.0.1")]);
                    let bytes = ini_writer::set_ini_values(
                        &bytes,
                        pack::SECTION,
                        &[(marker, "0.0.1"), (VERSION_PREVIOUS, baseline)],
                    );
                    let both =
                        embed_kind(first.other(), &bytes, &importing(first.other())).unwrap();
                    assert_eq!(
                        ini_value(&both, "Basic", MIN_VERSION),
                        version::GAME_VERSION
                    );
                    assert_eq!(ini_value(&both, pack::SECTION, VERSION_PREVIOUS), baseline);
                    let remaining = remove_kind(first, &both).unwrap();
                    let removed = remove_kind(first.other(), &remaining).unwrap();
                    assert_eq!(
                        ini_value(&removed, "Basic", MIN_VERSION),
                        if baseline == "none" { "" } else { baseline }
                    );
                }
            }
        }
    }
}
