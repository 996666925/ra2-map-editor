//! Session-local watches for successfully imported Wasm sources.
use eframe::egui;
use ra2_map_document::document::MapDocument;
use ra2_map_io::wasm_mission::{self, ModuleKind};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use vera20k_core::map::ext_module_pack as pack;

const SCAN_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub(super) struct Source {
    pub kind: ModuleKind,
    pub path: PathBuf,
    pub bytes: Arc<[u8]>,
}

struct WatchedSource {
    acknowledged: Source,
    binding: ModuleBinding,
    candidate: Option<Arc<[u8]>>,
    pending: Option<Arc<[u8]>>,
    invalid: Option<Arc<[u8]>>,
}

struct Probe {
    source: Source,
    candidate: Option<Arc<[u8]>>,
    verified: Option<Arc<[u8]>>,
    invalid: Option<Arc<[u8]>>,
}

#[derive(PartialEq, Eq)]
struct ModuleBinding {
    size: Option<Vec<u8>>,
    packed: BTreeMap<String, Vec<u8>>,
}

impl ModuleBinding {
    fn capture(doc: &MapDocument, kind: ModuleKind) -> Self {
        let (size, section) = match kind {
            ModuleKind::Rules => (pack::RULES_SIZE, pack::RULES_PACK),
            ModuleKind::Ui => (pack::UI_SIZE, pack::UI_PACK),
        };
        Self {
            size: doc.ini_value(pack::SECTION, size),
            packed: doc.ini_fields(section),
        }
    }
}

enum Reading {
    Available(Arc<[u8]>),
    Invalid(Arc<[u8]>),
    Unavailable,
}

type Readings = Vec<Reading>;

#[derive(Default)]
pub(super) struct SourceWatch {
    document_id: Option<u64>,
    revision: Option<u64>,
    sources: Vec<WatchedSource>,
    worker: Option<mpsc::Receiver<Readings>>,
    next_scan: Option<Instant>,
}

impl SourceWatch {
    pub fn sync_document(&mut self, doc: Option<&MapDocument>) {
        let document_id = doc.map(|doc| doc.id);
        if self.document_id != document_id {
            *self = Self {
                document_id,
                ..Default::default()
            };
        }
        let Some(doc) = doc else { return };
        if self.revision == Some(doc.edit_revision()) {
            return;
        }
        self.revision = Some(doc.edit_revision());
        let count = self.sources.len();
        // Compare only the module's INI fields: undo/replacement invalidates its source,
        // while unrelated edits need neither a map parse nor module decompression.
        self.sources.retain(|source| {
            ModuleBinding::capture(doc, source.acknowledged.kind) == source.binding
        });
        if self.sources.len() != count {
            self.invalidate_scan();
        }
    }

    pub fn imported(&mut self, doc: &MapDocument, kind: ModuleKind, source: Option<Source>) {
        self.sync_document(Some(doc));
        self.sources.retain(|entry| entry.acknowledged.kind != kind);
        if let Some(source) = source {
            self.sources.push(WatchedSource {
                acknowledged: source,
                binding: ModuleBinding::capture(doc, kind),
                candidate: None,
                pending: None,
                invalid: None,
            });
        }
        // Discard any in-flight readings of the previous source/path.
        self.invalidate_scan();
    }

    fn invalidate_scan(&mut self) {
        self.worker = None;
        self.next_scan = None;
    }

    pub fn pending(&self) -> Option<Source> {
        self.sources.iter().find_map(|source| {
            source.pending.as_ref().map(|bytes| Source {
                bytes: Arc::clone(bytes),
                ..source.acknowledged.clone()
            })
        })
    }

    pub fn dismiss(&mut self, kind: ModuleKind) {
        if let Some(source) = self
            .sources
            .iter_mut()
            .find(|source| source.acknowledged.kind == kind)
            && let Some(bytes) = source.pending.take()
        {
            source.acknowledged.bytes = bytes;
            source.candidate = None;
            self.invalidate_scan();
        }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(readings) => {
                    self.worker = None;
                    self.observe(readings);
                }
                Err(mpsc::TryRecvError::Disconnected) => self.worker = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.sources.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.worker.is_none() && self.next_scan.is_none_or(|next| now >= next) {
            let probes = self.probes();
            let (send, receive) = mpsc::channel();
            self.worker = Some(receive);
            self.next_scan = Some(now + SCAN_INTERVAL);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = send.send(read_sources(probes));
                ctx.request_repaint();
            });
        }
        ctx.request_repaint_after(SCAN_INTERVAL);
    }

    fn probes(&self) -> Vec<Probe> {
        self.sources
            .iter()
            .map(|source| Probe {
                source: source.acknowledged.clone(),
                candidate: source.candidate.clone(),
                verified: source.pending.clone(),
                invalid: source.invalid.clone(),
            })
            .collect()
    }

    fn observe(&mut self, readings: Readings) {
        for (source, bytes) in self.sources.iter_mut().zip(readings) {
            match bytes {
                Reading::Available(bytes) if bytes != source.acknowledged.bytes => {
                    source.invalid = None;
                    if source.candidate.as_ref() == Some(&bytes) {
                        source.pending = Some(bytes);
                    } else {
                        source.candidate = Some(bytes);
                        source.pending = None;
                    }
                }
                reading => {
                    match reading {
                        Reading::Available(_) => source.invalid = None,
                        Reading::Invalid(bytes) => source.invalid = Some(bytes),
                        Reading::Unavailable => {}
                    }
                    source.candidate = None;
                    source.pending = None;
                }
            }
        }
    }
}

fn read_sources(probes: Vec<Probe>) -> Readings {
    probes
        .into_iter()
        .map(
            |Probe {
                 source,
                 candidate,
                 verified,
                 invalid,
             }| {
                let Ok(bytes) = wasm_mission::read_module(&source.path) else {
                    return Reading::Unavailable;
                };
                let bytes: Arc<[u8]> = bytes.into();
                if invalid.as_ref() == Some(&bytes) {
                    return Reading::Invalid(bytes);
                }
                // Wait for two equal samples, and validate each stable version only once.
                if bytes != source.bytes
                    && candidate.as_ref() == Some(&bytes)
                    && verified.as_ref() != Some(&bytes)
                    && wasm_mission::verify_kind(source.kind, &bytes).is_err()
                {
                    return Reading::Invalid(bytes);
                }
                Reading::Available(bytes)
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(version: u8) -> Vec<u8> {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend([0, 2, 1, version]); // A one-character custom-section name.
        bytes
    }

    fn bind(
        watch: &mut SourceWatch,
        doc: &mut MapDocument,
        dir: &Path,
        kind: ModuleKind,
    ) -> PathBuf {
        let bytes = module(b'a');
        wasm_mission::apply_kind(kind, doc, Some(&bytes)).unwrap();
        let path = dir.join(kind.file_name());
        std::fs::write(&path, &bytes).unwrap();
        watch.imported(
            doc,
            kind,
            Some(Source {
                kind,
                path: path.clone(),
                bytes: bytes.into(),
            }),
        );
        path
    }

    use std::path::Path;

    fn scan(watch: &mut SourceWatch) {
        let readings = read_sources(watch.probes());
        watch.observe(readings);
    }

    #[test]
    fn only_stable_content_changes_prompt_and_dismissal_is_per_version() {
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let dir = tempfile::tempdir().unwrap();
            let mut doc = ra2_map_document::test_support::document();
            let mut watch = SourceWatch::default();
            let path = bind(&mut watch, &mut doc, dir.path(), kind);
            // A rewrite of identical bytes must not prompt, regardless of its mtime.
            std::fs::write(&path, module(b'a')).unwrap();
            scan(&mut watch);
            scan(&mut watch);
            assert!(watch.pending().is_none());
            std::fs::write(&path, module(b'b')).unwrap();
            scan(&mut watch);
            assert!(watch.pending().is_none());
            scan(&mut watch);
            assert_eq!(watch.pending().unwrap().bytes.as_ref(), module(b'b'));
            watch.dismiss(kind);
            scan(&mut watch);
            scan(&mut watch);
            assert!(watch.pending().is_none());
            std::fs::write(&path, module(b'c')).unwrap();
            scan(&mut watch);
            scan(&mut watch);
            assert_eq!(watch.pending().unwrap().bytes.as_ref(), module(b'c'));
            // Declining never changes the embedded module.
            assert_eq!(
                wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                    .unwrap()
                    .unwrap(),
                module(b'a')
            );
        }
    }

    #[test]
    fn incomplete_invalid_and_temporarily_missing_outputs_wait_for_a_valid_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        let path = bind(&mut watch, &mut doc, dir.path(), ModuleKind::Ui);
        std::fs::remove_file(&path).unwrap();
        scan(&mut watch);
        for bytes in [b"\0as".as_slice(), b"not wasm"] {
            std::fs::write(&path, bytes).unwrap();
            scan(&mut watch);
            scan(&mut watch);
            assert!(watch.pending().is_none());
        }
        std::fs::remove_file(&path).unwrap();
        let temp = dir.path().join("ui.wasm.tmp");
        std::fs::write(&temp, module(b'b')).unwrap();
        std::fs::rename(&temp, &path).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert_eq!(watch.pending().unwrap().bytes.as_ref(), module(b'b'));
        std::fs::remove_file(&path).unwrap();
        scan(&mut watch);
        assert!(watch.pending().is_none());
        assert_eq!(watch.sources.len(), 1);
    }

    #[test]
    fn simultaneous_changes_and_removal_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        let rules = bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        let ui = bind(&mut watch, &mut doc, dir.path(), ModuleKind::Ui);
        std::fs::write(rules, module(b'b')).unwrap();
        std::fs::write(ui, module(b'c')).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert_eq!(watch.pending().unwrap().kind, ModuleKind::Rules);
        watch.dismiss(ModuleKind::Rules);
        assert_eq!(watch.pending().unwrap().kind, ModuleKind::Ui);
        wasm_mission::apply_kind(ModuleKind::Ui, &mut doc, None).unwrap();
        watch.sync_document(Some(&doc));
        assert!(watch.pending().is_none());
        assert_eq!(watch.sources.len(), 1);
        assert_eq!(watch.sources[0].acknowledged.kind, ModuleKind::Rules);
    }

    #[test]
    fn undo_import_or_switching_or_closing_maps_discards_watches_and_late_readings() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        let (send, receive) = mpsc::channel();
        watch.worker = Some(receive);
        assert!(doc.undo());
        watch.sync_document(Some(&doc));
        assert!(watch.sources.is_empty());
        assert!(
            send.send(vec![Reading::Available(module(b'b').into())])
                .is_err()
        );
        assert!(doc.redo());
        watch.sync_document(Some(&doc));
        assert!(watch.sources.is_empty());
        bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        let other = ra2_map_document::test_support::document();
        watch.sync_document(Some(&other));
        assert!(watch.sources.is_empty());
        bind(&mut watch, &mut doc, dir.path(), ModuleKind::Ui);
        watch.sync_document(None);
        assert!(watch.sources.is_empty());
        assert!(watch.document_id.is_none());
    }

    #[test]
    fn rebinding_identical_bytes_discards_old_paths_and_inflight_scans() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        let old = bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        let (send, receive) = mpsc::channel();
        watch.worker = Some(receive);
        let path = dir.path().join("other.wasm");
        std::fs::write(&path, module(b'a')).unwrap();
        watch.imported(
            &doc,
            ModuleKind::Rules,
            Some(Source {
                kind: ModuleKind::Rules,
                path: path.clone(),
                bytes: module(b'a').into(),
            }),
        );
        assert!(
            send.send(vec![Reading::Available(module(b'b').into())])
                .is_err()
        );
        std::fs::write(old, module(b'b')).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert!(watch.pending().is_none());
        std::fs::write(&path, module(b'c')).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert_eq!(watch.pending().unwrap().path, path);
    }

    #[test]
    fn undo_overwriting_an_existing_module_drops_the_new_files_binding() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        let path = dir.path().join("replacement.wasm");
        let bytes = module(b'b');
        std::fs::write(&path, &bytes).unwrap();
        wasm_mission::apply_kind(ModuleKind::Rules, &mut doc, Some(&bytes)).unwrap();
        watch.imported(
            &doc,
            ModuleKind::Rules,
            Some(Source {
                kind: ModuleKind::Rules,
                path: path.clone(),
                bytes: bytes.into(),
            }),
        );
        assert_eq!(watch.sources.len(), 1);
        assert!(doc.undo());
        watch.sync_document(Some(&doc));
        assert!(watch.sources.is_empty());
        std::fs::write(path, module(b'c')).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert!(watch.pending().is_none());
        assert_eq!(
            wasm_mission::extract(&doc.snapshot().bytes().unwrap())
                .unwrap()
                .unwrap(),
            module(b'a')
        );
    }

    #[test]
    fn unchanged_invalid_bytes_keep_their_failed_validation_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = ra2_map_document::test_support::document();
        let mut watch = SourceWatch::default();
        let path = bind(&mut watch, &mut doc, dir.path(), ModuleKind::Rules);
        std::fs::write(&path, b"invalid").unwrap();
        scan(&mut watch);
        scan(&mut watch);
        let invalid = watch.sources[0].invalid.clone().unwrap();
        assert!(watch.sources[0].candidate.is_none());
        for _ in 0..3 {
            // Even a candidate that would otherwise trigger validation is bypassed.
            let mut probes = watch.probes();
            probes[0].candidate = Some(invalid.clone());
            assert!(
                matches!(&read_sources(probes)[0], Reading::Invalid(bytes) if *bytes == invalid)
            );
            scan(&mut watch);
            assert_eq!(watch.sources[0].invalid.as_ref(), Some(&invalid));
            assert!(watch.sources[0].candidate.is_none());
        }
        std::fs::write(path, module(b'b')).unwrap();
        scan(&mut watch);
        scan(&mut watch);
        assert!(watch.sources[0].invalid.is_none());
        assert_eq!(watch.pending().unwrap().bytes.as_ref(), module(b'b'));
    }

    fn finish_import(
        state: &mut super::super::WasmMissionUi,
        editor: &mut crate::editing::Editor,
        scene: &ra2_map_scene::map::MapScene,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.worker.is_some() && Instant::now() < deadline {
            state.poll(editor, Some(scene));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(state.worker.is_none());
    }

    #[test]
    fn failed_or_stale_imports_preserve_the_last_successful_binding() {
        use super::super::WasmMissionUi;
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut state = WasmMissionUi::default();
        let missing = root.join("missing.wasm");
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(missing),
        );
        finish_import(&mut state, &mut editor, &scene);
        assert!(state.sources.sources.is_empty());
        let path = root.join("rules.wasm");
        std::fs::write(&path, module(b'a')).unwrap();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(path.clone()),
        );
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources.len(), 1);
        let bad = root.join("invalid.wasm");
        std::fs::write(&bad, b"invalid").unwrap();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(bad),
        );
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources[0].acknowledged.path, path);
        let new_path = root.join("other.wasm");
        std::fs::write(&new_path, module(b'b')).unwrap();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(new_path.clone()),
        );
        let doc = editor.doc.as_mut().unwrap();
        doc.set_ini("Basic", "Name", Some(b"Keep concurrent edit".to_vec()))
            .unwrap();
        doc.commit("concurrent edit");
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources[0].acknowledged.path, path);
        assert_eq!(
            editor.doc.as_ref().unwrap().ini_text("Basic", "Name"),
            "Keep concurrent edit"
        );
        // An unchanged successful import still rebinds to the newly selected path.
        std::fs::write(&new_path, module(b'a')).unwrap();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(new_path.clone()),
        );
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources[0].acknowledged.path, new_path);
        // Successful removal also releases the binding.
        state.begin(&ctx, editor.doc.as_ref().unwrap(), ModuleKind::Rules, None);
        finish_import(&mut state, &mut editor, &scene);
        assert!(state.sources.sources.is_empty());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn confirmed_bytes_are_used_even_if_the_file_changes_again_and_imports_are_serialized() {
        use super::super::{ModuleUpdate, WasmMissionUi};
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut state = WasmMissionUi::default();
        let path = bind(
            &mut state.sources,
            editor.doc.as_mut().unwrap(),
            &root,
            ModuleKind::Rules,
        );
        std::fs::write(&path, module(b'b')).unwrap();
        scan(&mut state.sources);
        scan(&mut state.sources);
        let confirmed = state.sources.pending().unwrap();
        std::fs::write(&path, module(b'c')).unwrap();
        state.begin_update(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            ModuleUpdate::Reload(confirmed),
        );
        // A second operation must not replace the in-flight worker.
        state.begin(&ctx, editor.doc.as_ref().unwrap(), ModuleKind::Rules, None);
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(
            wasm_mission::extract(&editor.doc.as_ref().unwrap().snapshot().bytes().unwrap())
                .unwrap()
                .unwrap(),
            module(b'b')
        );
        scan(&mut state.sources);
        scan(&mut state.sources);
        assert_eq!(
            state.sources.pending().unwrap().bytes.as_ref(),
            module(b'c')
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_reimport_remains_pending_and_switching_maps_cannot_apply_it() {
        use super::super::{ModuleUpdate, WasmMissionUi};
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut state = WasmMissionUi::default();
        let path = bind(
            &mut state.sources,
            editor.doc.as_mut().unwrap(),
            &root,
            ModuleKind::Rules,
        );
        std::fs::write(&path, module(b'b')).unwrap();
        scan(&mut state.sources);
        scan(&mut state.sources);
        let confirmed = state.sources.pending().unwrap();
        state.begin_update(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            ModuleUpdate::Reload(confirmed.clone()),
        );
        let doc = editor.doc.as_mut().unwrap();
        doc.set_ini("Basic", "Name", Some(b"Keep".to_vec()))
            .unwrap();
        doc.commit("concurrent edit");
        finish_import(&mut state, &mut editor, &scene);
        assert!(!state.reload_error.is_empty());
        assert_eq!(
            state.sources.pending().unwrap().bytes.as_ref(),
            module(b'b')
        );
        state.begin_update(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            ModuleUpdate::Reload(confirmed),
        );
        editor.doc = Some(ra2_map_document::test_support::document());
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        state.sources.sync_document(editor.doc.as_ref());
        assert!(state.sources.sources.is_empty());
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closed_panel_prompts_for_rules_source_changes_and_reimports_only_after_confirmation() {
        check_closed_panel_prompt(ModuleKind::Rules, ModuleKind::Ui);
    }

    #[test]
    fn closed_panel_prompts_for_ui_source_changes_and_reimports_only_after_confirmation() {
        check_closed_panel_prompt(ModuleKind::Ui, ModuleKind::Rules);
    }

    #[test]
    fn completed_build_waits_for_an_existing_import_and_watches_its_own_output() {
        use super::super::WasmMissionUi;
        use ra2_map_io::mission_project::BuiltModule;
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut state = WasmMissionUi::default();
        let rules = root.join("rules.wasm");
        let ui = root.join("ui.wasm");
        std::fs::write(&rules, module(b'a')).unwrap();
        std::fs::write(&ui, module(b'b')).unwrap();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(rules.clone()),
        );
        let (send, receive) = mpsc::channel();
        state.build_worker = Some(receive);
        state.embed_after_build = Some(editor.doc.as_ref().unwrap().id);
        send.send(Ok(vec![BuiltModule {
            kind: ModuleKind::Ui,
            path: ui.clone(),
        }]))
        .unwrap();
        state.poll_build(&ctx, &editor, Some(&scene));
        assert!(state.build_worker.is_some());
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources[0].acknowledged.path, rules);
        state.poll_build(&ctx, &editor, Some(&scene));
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources.len(), 2);
        assert_eq!(state.sources.sources[1].acknowledged.path, ui);
        let bytes = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        assert_eq!(
            wasm_mission::extract_kind(ModuleKind::Rules, &bytes)
                .unwrap()
                .unwrap(),
            module(b'a')
        );
        assert_eq!(
            wasm_mission::extract_kind(ModuleKind::Ui, &bytes)
                .unwrap()
                .unwrap(),
            module(b'b')
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn check_closed_panel_prompt(kind: ModuleKind, other: ModuleKind) {
        use super::super::WasmMissionUi;
        use egui_kittest::kittest::Queryable as _;
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        wasm_mission::apply_kind(other, editor.doc.as_mut().unwrap(), Some(&module(b'z'))).unwrap();
        let mut initial = WasmMissionUi::default();
        let path = root.join(kind.file_name());
        std::fs::write(&path, module(b'a')).unwrap();
        initial.begin(&ctx, editor.doc.as_ref().unwrap(), kind, Some(path.clone()));
        finish_import(&mut initial, &mut editor, &scene);
        assert_eq!(initial.sources.sources.len(), 1);
        assert!(!initial.open);
        let state = std::cell::RefCell::new(initial);
        let mut harness = crate::test_support::harness()
            .with_size([800., 600.])
            .with_accessibility_check(false)
            .build_ui(|ui| state.borrow_mut().show(ui.ctx(), &mut editor, Some(&scene)));
        harness.run();
        assert!(harness.query_by_label("Extension System").is_none());
        std::fs::write(&path, module(b'b')).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.borrow().sources.pending().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            harness.run();
        }
        harness.get_by_label("Extension file changed");
        assert!(state.borrow().worker.is_none());
        harness.get_by_label("Not now").click();
        harness.run();
        assert!(state.borrow().sources.pending().is_none());
        assert!(state.borrow().worker.is_none());
        assert!(harness.query_by_label("Reimport").is_none());
        std::fs::write(&path, module(b'c')).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.borrow().sources.pending().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            harness.run();
        }
        harness.get_by_label("Reimport").click();
        harness.run();
        let deadline = Instant::now() + Duration::from_secs(5);
        while state.borrow().worker.is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
            harness.run();
        }
        assert!(state.borrow().worker.is_none());
        assert!(state.borrow().sources.pending().is_none());
        assert!(harness.query_by_label("Reimport").is_none());
        drop(harness);
        let doc = editor.doc.as_mut().unwrap();
        let bytes = doc.snapshot().bytes().unwrap();
        assert_eq!(
            wasm_mission::extract_kind(kind, &bytes).unwrap().unwrap(),
            module(b'c')
        );
        assert_eq!(
            wasm_mission::extract_kind(other, &bytes).unwrap().unwrap(),
            module(b'z')
        );
        assert!(doc.undo());
        let bytes = doc.snapshot().bytes().unwrap();
        assert_eq!(
            wasm_mission::extract_kind(kind, &bytes).unwrap().unwrap(),
            module(b'a')
        );
        assert_eq!(
            wasm_mission::extract_kind(other, &bytes).unwrap().unwrap(),
            module(b'z')
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_build_binds_both_outputs_and_one_undo_stops_both_watches() {
        use super::super::WasmMissionUi;
        use ra2_map_io::mission_project::BuiltModule;
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut modules = Vec::new();
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            let path = root.join(kind.file_name());
            std::fs::write(&path, module(b'a')).unwrap();
            modules.push(BuiltModule { kind, path });
        }
        let (send, receive) = mpsc::channel();
        let mut state = WasmMissionUi {
            build_worker: Some(receive),
            embed_after_build: Some(editor.doc.as_ref().unwrap().id),
            ..Default::default()
        };
        send.send(Ok(modules)).unwrap();
        state.poll_build(&ctx, &editor, Some(&scene));
        finish_import(&mut state, &mut editor, &scene);
        assert_eq!(state.sources.sources.len(), 2);
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            std::fs::write(root.join(kind.file_name()), module(b'b')).unwrap();
        }
        scan(&mut state.sources);
        scan(&mut state.sources);
        assert_eq!(state.sources.pending().unwrap().kind, ModuleKind::Rules);
        state.sources.dismiss(ModuleKind::Rules);
        assert_eq!(state.sources.pending().unwrap().kind, ModuleKind::Ui);
        let doc = editor.doc.as_mut().unwrap();
        assert!(doc.undo());
        state.sources.sync_document(Some(doc));
        assert!(state.sources.sources.is_empty());
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            assert!(
                wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                    .unwrap()
                    .is_none()
            );
        }
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }
}
