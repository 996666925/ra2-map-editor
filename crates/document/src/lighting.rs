//! Editable keys shared with the pinned engine's scenario lighting reader.
use crate::document::{MapDocument, Snapshot};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use vera20k_core::{
    map::lighting::{self, CellRelightProfile},
    rules::ini_parser::IniFile,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    #[default]
    Normal,
    Ion,
    Dominator,
}
impl Profile {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Ion, Self::Dominator];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "普通",
            Self::Ion => "闪电风暴",
            Self::Dominator => "心灵控制器",
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Normal => "",
            Self::Ion => "Ion",
            Self::Dominator => "Dominator",
        }
    }
    pub fn units(self, ini: &IniFile) -> CellRelightProfile {
        let p = lighting::parse_lighting_profiles(ini);
        match self {
            Self::Normal => p.normal.into(),
            Self::Ion => p.ion.into(),
            Self::Dominator => CellRelightProfile {
                units: p.dominator,
                top_level_units: vera20k_core::sim::scenario_session::NUKE_LEVEL_UNITS,
            },
        }
    }
}
pub const COMPONENTS: [&str; 6] = ["Ambient", "Red", "Green", "Blue", "Ground", "Level"];
pub fn keys() -> Vec<String> {
    Profile::ALL
        .into_iter()
        .flat_map(|p| COMPONENTS.map(|k| format!("{}{k}", p.prefix())))
        .chain([
            "DominatorAmbientChangeRate".into(),
            "NukeAmbientChangeRate".into(),
        ])
        .collect()
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub values: BTreeMap<String, String>,
}
impl Draft {
    pub fn read(doc: &MapDocument) -> Self {
        Self {
            values: keys()
                .into_iter()
                .map(|key| {
                    let v = doc.ini_text("Lighting", &key);
                    (key, v)
                })
                .collect(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        for (key, value) in &self.values {
            ensure!(keys().contains(key), "未知光照字段 {key}");
            if value.trim().is_empty() {
                continue;
            }
            let n = value.trim_end_matches('%').parse::<f64>()?
                / if value.ends_with('%') { 100. } else { 1. };
            ensure!(
                n.is_finite() && (-16.0..=16.0).contains(&n),
                "{key} 须为 -16～16 的有限值"
            );
            if key.ends_with("ChangeRate") {
                ensure!(n > 0., "过渡间隔必须大于 0");
            }
        }
        Ok(())
    }
    pub fn apply(&self, doc: &mut MapDocument) -> Result<()> {
        self.validate()?;
        doc.atomic(|doc| {
            for (key, value) in &self.values {
                if *value == doc.ini_text("Lighting", key) {
                    continue;
                }
                doc.set_ini(
                    "Lighting",
                    key,
                    (!value.trim().is_empty()).then(|| value.as_bytes().to_vec()),
                )?;
            }
            Ok(())
        })
    }
    pub fn snapshot(&self, doc: &MapDocument) -> Result<Snapshot> {
        let mut scratch = doc.fork();
        self.apply(&mut scratch)?;
        let mut snapshot = scratch.snapshot();
        snapshot.document_id = doc.id;
        snapshot.state_id = doc.state_id;
        Ok(snapshot)
    }
    pub fn effective(&self, doc: &MapDocument, profile: Profile) -> Result<CellRelightProfile> {
        let _ = doc;
        self.validate()?;
        let text = format!(
            "[Lighting]\n{}",
            self.values
                .iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .map(|(k, v)| format!("{k}={v}\n"))
                .collect::<String>()
        );
        Ok(profile.units(&IniFile::from_str(&text)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Baseline, new_map::NewMapOptions};
    #[test]
    fn previews_are_read_only_and_native_profiles_quantize_apply_and_undo() {
        let bytes = NewMapOptions::default().bytes().unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
        let mut d = Draft::read(&doc);
        d.values.insert("IonAmbient".into(), "0.61".into());
        d.values.insert("DominatorLevel".into(), "0.04".into());
        let profile = d.effective(&doc, Profile::Ion).unwrap();
        assert_eq!(profile.units.ambient_percent, 61);
        let dom = d.effective(&doc, Profile::Dominator).unwrap();
        assert_eq!(dom.units.level_units, 40);
        assert_eq!(dom.top_level_units, 100);
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
        assert!(!doc.dirty());
        d.apply(&mut doc).unwrap();
        doc.commit("light");
        assert_eq!(doc.ini_text("Lighting", "IonAmbient"), "0.61");
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
        assert!(doc.redo());
        d.values.insert("Ambient".into(), "NaN".into());
        assert!(d.apply(&mut doc).is_err());
    }
}
