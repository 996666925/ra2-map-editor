//! Presentation-only localization. Chinese source messages act as gettext-style
//! keys; identifiers and user-authored map/script text must never be translated.
use std::{cell::Cell, collections::BTreeMap, sync::OnceLock};

use eframe::egui;
use serde::{Deserialize, Serialize};

macro_rules! tr {
    ($source:literal $(,)?) => { $crate::i18n::text($source) };
    ($source:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::format($source, &[$((stringify!($name), ($value).to_string())),+])
    };
}
pub(crate) use tr;

const STORAGE_KEY: &str = "editor-language";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Language {
    #[serde(rename = "en")]
    English,
    #[default]
    #[serde(rename = "zh-CN", other)]
    Chinese,
}

pub(crate) struct LanguageInfo {
    pub language: Language,
    pub native_name: &'static str,
    pub code: &'static str,
    catalog: Option<&'static str>,
    messages: OnceLock<BTreeMap<String, String>>,
}

/// Use native names so users can recover from selecting an unfamiliar language.
pub(crate) static LANGUAGES: [LanguageInfo; 2] = [
    LanguageInfo {
        language: Language::Chinese,
        native_name: "简体中文",
        code: "zh-CN",
        catalog: None,
        messages: OnceLock::new(),
    },
    LanguageInfo {
        language: Language::English,
        native_name: "English",
        code: "en",
        catalog: Some(include_str!("../locales/en.json")),
        messages: OnceLock::new(),
    },
];

impl Language {
    fn info(self) -> &'static LanguageInfo {
        LANGUAGES
            .iter()
            .find(|info| info.language == self)
            .expect("registered language")
    }
}

impl LanguageInfo {
    fn lookup(&self, source: &str) -> Option<&str> {
        let catalog = self.catalog?;
        self.messages
            .get_or_init(|| serde_json::from_str(catalog).expect("validated language catalog"))
            .get(source)
            .map(String::as_str)
    }
}

thread_local! {
    // Scoped to the UI/logic callback, never a process-wide language setting.
    // Separate windows and parallel UI tests therefore cannot affect each other.
    static CURRENT: Cell<Language> = const { Cell::new(Language::Chinese) };
}

pub(crate) struct LanguageScope(Language);
impl Drop for LanguageScope {
    fn drop(&mut self) {
        CURRENT.set(self.0);
    }
}

pub(crate) fn enter(language: Language) -> LanguageScope {
    LanguageScope(CURRENT.replace(language))
}

pub(crate) fn selected_language() -> Language {
    CURRENT.get()
}

pub(crate) fn selected(ctx: &egui::Context) -> Language {
    ctx.data(|data| data.get_temp(egui::Id::unique(STORAGE_KEY)))
        .unwrap_or_default()
}

pub(crate) fn select(ctx: &egui::Context, language: Language) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(STORAGE_KEY), language));
    let _language = enter(language);
    ctx.send_viewport_cmd(egui::ViewportCommand::Title(
        text("尤里的复仇地图编辑器").to_owned(),
    ));
    ctx.request_repaint();
}

pub(crate) fn restore(ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
    select(
        ctx,
        storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default(),
    );
}

pub(crate) fn save(ctx: &egui::Context, storage: &mut dyn eframe::Storage) {
    eframe::set_value(storage, STORAGE_KEY, &selected(ctx));
}

/// Borrow source text when untranslated. Only call for editor-owned messages.
pub(crate) fn text(source: &str) -> &str {
    CURRENT.get().info().lookup(source).unwrap_or(source)
}

/// Values have already been formatted at the call site. Named slots can be
/// reordered by translators; inserted values are never interpreted or translated.
pub(crate) fn format(source: &str, values: &[(&str, String)]) -> String {
    let template = text(source);
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                result.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                result.push('}');
            }
            '{' => {
                let mut slot = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    slot.push(c);
                }
                if let Some((_, value)) = values.iter().find(|(key, _)| *key == slot) {
                    result.push_str(value);
                } else {
                    result.push('{');
                    result.push_str(&slot);
                    result.push('}');
                }
            }
            _ => result.push(c),
        }
    }
    result
}

pub(crate) fn language_menu(ui: &mut egui::Ui) {
    ui.push_id("editor-language-menu", |ui| {
        ui.menu_button(text("语言 / Language"), |ui| {
            let mut language = selected(ui.ctx());
            for info in &LANGUAGES {
                if ui
                    .selectable_value(&mut language, info.language, info.native_name)
                    .on_hover_text(info.code)
                    .clicked()
                {
                    select(ui.ctx(), language);
                    ui.close();
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::Storage as _;
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct Storage(BTreeMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn language_defaults_persists_and_recovers_from_unknown_or_broken_settings() {
        let ctx = egui::Context::default();
        assert_eq!(selected(&ctx), Language::Chinese);
        let mut storage = Storage::default();
        select(&ctx, Language::English);
        save(&ctx, &mut storage);
        let reopened = egui::Context::default();
        restore(&reopened, Some(&storage));
        assert_eq!(selected(&reopened), Language::English);
        // Persist stable language codes, independent of enum order or labels.
        assert!(storage.0[STORAGE_KEY].contains("en"));
        for invalid in ["\"future-language\"", "broken settings"] {
            storage.set_string(STORAGE_KEY, invalid.to_owned());
            restore(&reopened, Some(&storage));
            assert_eq!(selected(&reopened), Language::Chinese);
        }
        restore(&reopened, None);
        assert_eq!(selected(&reopened), Language::Chinese);
    }

    #[test]
    fn formatting_preserves_values_and_scopes_are_isolated() {
        assert_eq!(text("设置"), "设置");
        {
            let _english = enter(Language::English);
            assert_eq!(text("设置"), "Settings");
            assert_eq!(text("untranslated extension"), "untranslated extension");
            assert_eq!(tr!("航点 {start}", start = 12), "Waypoint 12");
            let value = "地图 {{x}} {start}";
            assert_eq!(tr!("日志：{arg0}", arg0 = value), format!("Log: {value}"));
            assert_eq!(format("{{{value}}}", &[("value", "a{b}".into())]), "{a{b}}");
            std::thread::spawn(|| assert_eq!(text("设置"), "设置"))
                .join()
                .unwrap();
            {
                let _chinese = enter(Language::Chinese);
                assert_eq!(tr!("航点 {start}", start = 12), "航点 12");
            }
            assert_eq!(text("设置"), "Settings");
        }
        assert_eq!(text("设置"), "设置");
    }

    fn slots(template: &str) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    continue;
                }
                let slot: String = chars.by_ref().take_while(|c| *c != '}').collect();
                assert!(!slot.is_empty(), "use named slots: {template}");
                result.insert(slot);
            }
        }
        result
    }

    #[test]
    fn all_catalogs_have_valid_slots_and_cover_schema_labels() {
        use ra2_map_document::mission::{self, Kind, schema};
        for info in &LANGUAGES {
            assert_eq!(serde_json::to_value(info.language).unwrap(), info.code);
            let Some(json) = info.catalog else { continue };
            let messages: BTreeMap<String, String> = serde_json::from_str(json).unwrap();
            for (source, translation) in &messages {
                assert!(!translation.is_empty(), "empty {}: {source}", info.code);
                assert_eq!(slots(source), slots(translation), "{}: {source}", info.code);
            }
            let require = |source: &str| {
                if !source.is_ascii() {
                    assert!(
                        messages.contains_key(source),
                        "{} missing: {source}",
                        info.code
                    );
                }
            };
            for kind in Kind::ALL {
                require(kind.label());
            }
            for severity in [
                mission::Severity::Error,
                mission::Severity::Warning,
                mission::Severity::Capability,
            ] {
                require(severity.label());
            }
            for opcode in schema::EVENTS
                .iter()
                .chain(schema::ACTIONS)
                .chain(schema::scripts::OPCODES)
            {
                require(opcode.name);
                require(opcode.hint);
                for p in opcode.parameters {
                    require(p.label);
                    if let schema::ParameterKind::Integer { unit, .. } = p.kind {
                        require(unit);
                    }
                }
            }
            for field in &ra2_map_document::crate_settings::FIELDS {
                require(field.label);
                require(field.hint);
            }
            for (_, label) in ra2_map_document::new_map::THEATERS {
                require(label);
            }
            for (_, label) in ra2_map_document::ai_triggers::CONDITIONS {
                require(label);
            }
            for section in ["Structures", "Infantry", "Units", "Aircraft"] {
                for field in ra2_map_document::object_properties::fields(section) {
                    require(field.name);
                }
            }
            for mode in ra2_map_scene::terrain_tools::TerrainMode::ALL {
                require(mode.name());
            }
        }
    }
}
