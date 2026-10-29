//! Physical INI fields participate in the same history as spatial edits.
use crate::document::{MapDocument, Target, Value};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn validate_target(section: &str, key: &str) -> Result<()> {
    ensure!(
        !section.is_empty()
            && section.len() <= 64
            && section.is_ascii()
            && !section
                .bytes()
                .any(|b| b.is_ascii_control() || matches!(b, b'[' | b']' | b';' | b'=')),
        "INI section 名称无效"
    );
    ensure!(
        !key.is_empty()
            && key.len() <= 64
            && key.is_ascii()
            && !key
                .bytes()
                .any(|b| b.is_ascii_control() || matches!(b, b'[' | b']' | b';' | b'=')),
        "INI key 无效"
    );
    ensure!(
        !matches!(
            section,
            "Map"
                | "Units"
                | "Infantry"
                | "Structures"
                | "Aircraft"
                | "Waypoints"
                | "Terrain"
                | "Smudge"
                | "IsoMapPack5"
                | "OverlayPack"
                | "OverlayDataPack"
                | "Preview"
                | "PreviewPack"
                | "Digest"
        ) && !(section == "Basic" && key == "NewINIFormat"),
        "此字段须通过专用地图工具修改"
    );
    Ok(())
}
impl MapDocument {
    /// Physical order and duplicate identities survive; pending additions follow.
    pub fn ini_entries(&self, section: &str) -> Vec<(Target, String, Vec<u8>)> {
        let mut out = Vec::new();
        for (line, row) in self.base.raw.entries(section) {
            let target = Target::IniLine(line);
            if let Some(Value::Ini(bytes)) = self.value(&target) {
                out.push((target, row.key.clone().unwrap(), bytes));
            }
        }
        for target in self.changes.keys() {
            if let Target::IniNew(s, key) = target
                && s == section
                && let Some(Value::Ini(bytes)) = self.value(target)
            {
                out.push((target.clone(), key.clone(), bytes));
            }
        }
        out
    }
    pub fn ini_value(&self, section: &str, key: &str) -> Option<Vec<u8>> {
        self.ini_entries(section)
            .into_iter()
            .find(|(_, k, v)| k == key && !v.is_empty())
            .map(|(_, _, v)| v)
    }
    pub fn ini_text(&self, section: &str, key: &str) -> String {
        self.ini_value(section, key)
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .unwrap_or_default()
    }
    pub fn ini_fields(&self, section: &str) -> BTreeMap<String, Vec<u8>> {
        let mut fields = BTreeMap::new();
        for (_, key, bytes) in self.ini_entries(section) {
            fields.entry(key).or_insert(bytes);
        }
        fields
    }
    /// Exact names; updating targets a physical line, deleting removes shadows
    /// too so that reload cannot resurrect an intentionally deleted entry.
    pub fn set_ini(&mut self, section: &str, key: &str, value: Option<Vec<u8>>) -> Result<()> {
        validate_target(section, key)?;
        let entries: Vec<_> = self
            .ini_entries(section)
            .into_iter()
            .filter(|(_, k, _)| k == key)
            .collect();
        if value.is_some() {
            let bodies = self
                .base
                .raw
                .lines
                .iter()
                .filter(|r| {
                    r.section == section
                        && self.base.raw.bytes[r.bytes.clone()]
                            .trim_ascii_start()
                            .starts_with(b"[")
                })
                .count();
            ensure!(
                bodies <= 1 && entries.len() <= 1,
                "存在重复 section/key，表单无法确定原生 CRC 查找命中的物理行；请先用文本工具消歧（其他原文保持不变）"
            );
        }
        if let Some((target, _, _)) = entries.first() {
            if let Some(value) = value {
                self.set(target.clone(), Some(Value::Ini(value)))?;
            } else {
                for (target, _, _) in entries {
                    self.set(target, None)?;
                }
            }
        } else if let Some(value) = value {
            self.set(
                Target::IniNew(section.into(), key.into()),
                Some(Value::Ini(value)),
            )?;
        }
        Ok(())
    }
}
