//! Authored campaign catalog. Paths are resolved by IO, never by the document.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const GLOBAL_COUNT: usize = 50;
#[cfg(test)]
#[path = "campaign_tests.rs"]
mod campaign_tests;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub id: String,
    pub entry: String,
    pub missions: Vec<Mission>,
    #[serde(default)]
    pub strings: BTreeMap<String, String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub file: String,
    pub source: String,
    pub side: String,
    pub ui_name: String,
    pub briefing: String,
    pub load_message: String,
    pub load_briefing: String,
    pub background_640: String,
    pub background_800: String,
    pub background_palette: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub source: String,
    pub name: String,
}
/// Runtime assets live in one flat CCFile namespace. Reject ambiguous spellings.
pub fn filename(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 120
            && value.is_ascii()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            && !value.starts_with('.')
            && !value.ends_with('.')
            && !value.contains(".."),
        "文件名须为不含路径的 ASCII 名称：{value}"
    );
    let stem = value.split('.').next().unwrap().to_ascii_uppercase();
    ensure!(
        !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(1..=9).any(|i| stem == format!("COM{i}") || stem == format!("LPT{i}")),
        "文件名是 Windows 保留名称"
    );
    Ok(())
}
impl Project {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "不支持的战役项目版本");
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 24
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "战役 ID 须为 1～24 字节字母、数字或下划线"
        );
        ensure!(
            !self.missions.is_empty() && self.missions.len() <= 100,
            "战役须有 1～100 个关卡"
        );
        let mut names = std::collections::BTreeSet::new();
        for m in &self.missions {
            filename(&m.file)?;
            ensure!(
                m.file.to_ascii_lowercase().ends_with(".map"),
                "关卡文件须为 .map"
            );
            ensure!(names.insert(m.file.to_ascii_uppercase()), "重复关卡文件名");
            ensure!(
                matches!(m.side.as_str(), "GDI" | "Nod" | "ThirdSide"),
                "关卡 Side 须为 GDI/Nod/ThirdSide"
            );
            ensure!(!m.source.is_empty(), "关卡缺少源文件");
            for (s, max) in [
                (&m.ui_name, 31),
                (&m.briefing, 255),
                (&m.load_message, 30),
                (&m.load_briefing, 30),
            ] {
                ensure!(
                    s.len() <= max && s.is_ascii() && !s.contains(['\r', '\n', ';', '=']),
                    "关卡字符串标签超出容量或含 INI 控制字符"
                );
            }
            for name in [&m.background_640, &m.background_800, &m.background_palette] {
                if !name.is_empty() {
                    filename(name)?;
                    ensure!(name.len() <= 63, "载入图文件名超过 63 字节");
                }
            }
        }
        ensure!(
            names.contains(&self.entry.to_ascii_uppercase()),
            "战役起始关卡不存在"
        );
        for a in &self.assets {
            filename(&a.name)?;
            ensure!(
                !a.source.is_empty() && names.insert(a.name.to_ascii_uppercase()),
                "资源路径为空或输出名冲突"
            );
            ensure!(
                !matches!(
                    a.name.to_ascii_lowercase().as_str(),
                    "battlemd.ini"
                        | "missionmd.ini"
                        | "mapselmd.ini"
                        | "ra2md.csf"
                        | "config.toml"
                        | "manifest.json"
                        | "dependencies.json"
                        | "install.txt"
                ),
                "资源不能覆盖生成的战役文件"
            );
            ensure!(
                !matches!(
                    a.name
                        .rsplit('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "exe" | "dll" | "mix" | "map"
                ),
                "发布资源仅接受独立数据文件"
            );
        }
        let mut labels = std::collections::BTreeSet::new();
        for (key, value) in &self.strings {
            ensure!(
                !key.is_empty() && key.len() <= 31 && key.is_ascii() && !key.contains('\0'),
                "CSF 标签须为 1～31 字节 ASCII"
            );
            ensure!(
                labels.insert(key.to_ascii_uppercase()) && !value.contains('\0'),
                "CSF 标签冲突或文本含 NUL"
            );
        }
        Ok(())
    }
}

/// Select only direct destinations. MAPSELMD membership is validated by IO.
pub fn destination(
    basic: &vera20k_core::map::basic::BasicSection,
    global_zero: bool,
) -> Option<&str> {
    if basic.end_of_game == Some(true) || basic.one_time_only == Some(true) {
        return None;
    }
    if global_zero {
        basic.alt_next_scenario.as_deref()
    } else {
        basic.next_scenario.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_refuse_escape_devices_and_case_collisions() {
        for name in ["../escape.map", "C:evil.map", "NUL.wav", "a..map", "a."] {
            assert!(filename(name).is_err());
        }
        let mut p = Project {
            version: 1,
            id: "TEST".into(),
            entry: "a.map".into(),
            missions: vec![Mission {
                file: "a.map".into(),
                source: "a.map".into(),
                side: "GDI".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(p.validate().is_ok());
        p.missions.push(p.missions[0].clone());
        assert!(p.validate().is_err());
    }
}
