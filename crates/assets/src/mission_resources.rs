//! Cached named choices; loaded on the installation worker, never in UI frames.
use std::collections::BTreeMap;
use vera20k_core::{
    assets::{asset_manager::AssetManager, csf_file::CsfFile},
    rules::ini_parser::IniFile,
};
#[derive(Default)]
pub struct MissionResources {
    pub choices: BTreeMap<String, Vec<(String, String)>>,
    pub errors: Vec<String>,
}
pub(crate) fn load(manager: &AssetManager) -> MissionResources {
    let mut out = MissionResources::default();
    if let Some(bytes) = manager.get_ref("artmd.ini")
        && let Ok(art) = IniFile::from_bytes(bytes)
        && let Some(movies) = art.section("Movies")
    {
        out.choices.insert(
            "Movie".into(),
            movies
                .raw_entries()
                .map(|(_, id)| (id.to_owned(), id.to_owned()))
                .collect(),
        );
    }
    for (name, file, section) in [
        ("Sound", "soundmd.ini", "SoundList"),
        ("Eva", "evamd.ini", "DialogList"),
        ("Theme", "thememd.ini", "Themes"),
    ] {
        match manager
            .get_ref(file)
            .ok_or_else(|| format!("缺少 {file}"))
            .and_then(|bytes| IniFile::from_bytes(bytes).map_err(|e| e.to_string()))
        {
            Ok(ini) => {
                if let Some(s) = ini.section(section) {
                    out.choices.insert(
                        name.into(),
                        s.raw_entries()
                            .map(|(_, v)| (v.to_owned(), v.to_owned()))
                            .collect(),
                    );
                }
            }
            Err(e) => out.errors.push(e),
        }
    }
    out.choices.insert(
        "Theme".into(),
        vera20k_core::rules::theme_ini::ThemeCatalog::select(manager)
            .entries()
            .iter()
            .map(|t| (t.key.clone(), t.key.clone()))
            .collect(),
    );
    match CsfFile::load_active(|name| manager.get_ref(name)) {
        Ok((table, _)) => {
            let mut entries: Vec<_> = table
                .entries()
                .map(|(label, text)| (label.to_owned(), format!("{label} · {text}")))
                .collect();
            entries.sort();
            out.choices.insert("Csf".into(), entries);
        }
        Err(e) => out.errors.push(e.to_string()),
    }
    out
}
