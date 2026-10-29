//! YR AITriggerTypeClass ReadINI41F580: eighteen fields, a 32-byte comparison.
use crate::mission;
use anyhow::{Result, ensure};

pub const CONDITIONS: &[(i32, &str)] = &[
    (-1, "始终"),
    (0, "敌方拥有对象数量"),
    (1, "己方拥有对象数量"),
    (2, "敌方电力余量小于 100"),
    (3, "敌方电力不足"),
    (4, "敌方资金"),
    (5, "己方铁幕接近就绪"),
    (6, "己方超时空接近就绪"),
    (7, "平民拥有对象数量"),
];
pub fn decode(raw: &str) -> Result<Vec<String>> {
    let f = mission::csv(raw);
    ensure!(
        f.len() == 18 && f.iter().all(|s| !s.is_empty()),
        "AI 触发器须为 18 个非空字段；扩展原文保留"
    );
    ensure!(raw.len() < 512, "AI 触发器超过原生 511 字节读取范围");
    comparison(&f[6])?;
    Ok(f)
}
pub fn comparison(raw: &str) -> Result<[u8; 32]> {
    ensure!(
        raw.len() == 64 && raw.bytes().all(|b| b.is_ascii_hexdigit()),
        "比较参数须为 64 位十六进制"
    );
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}
pub fn compare_words(raw: &str) -> Result<(i32, i32)> {
    let bytes = comparison(raw)?;
    Ok((
        i32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        i32::from_le_bytes(bytes[4..8].try_into().unwrap()),
    ))
}
pub fn with_comparison(raw: &str, amount: i32, operation: i32) -> Result<String> {
    ensure!((0..=5).contains(&operation), "比较运算须为 0～5");
    let mut bytes = comparison(raw)?;
    bytes[..4].copy_from_slice(&amount.to_le_bytes());
    bytes[4..8].copy_from_slice(&operation.to_le_bytes());
    Ok(bytes.iter().map(|b| format!("{b:02X}")).collect())
}
pub fn default_row(id: &str, team: &str) -> String {
    format!(
        "{id},{team},<all>,0,-1,<none>,{},40,10,60,1,0,0,0,<none>,1,1,1",
        "00".repeat(32)
    )
}
pub fn errors(f: &[String]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for i in [4, 10, 12, 13, 15, 16, 17] {
        if f[i].parse::<i32>().is_err() {
            out.push((i, "须为完整的 32 位整数".into()));
        }
    }
    for i in [7, 8, 9] {
        if f[i].parse::<i32>().is_err() || f[i].parse::<i32>().is_ok_and(|n| n < 0) {
            out.push((
                i,
                "权重建议为 0～2147483647 的整数；原生小数会截断，负数会无符号回绕".into(),
            ));
        }
    }
    if let (Ok(min), Ok(max)) = (f[8].parse::<i32>(), f[9].parse::<i32>())
        && min > max
    {
        out.push((8, "最小权重大于最大权重".into()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Baseline, MapDocument},
        mission::{self, Kind},
        new_map::NewMapOptions,
    };
    use vera20k_core::{
        map::map_file,
        rules::{ini_parser::IniFile, team_ai_ini::TeamAiIniRegistry},
    };
    fn document() -> MapDocument {
        let bytes = NewMapOptions::default().bytes().unwrap();
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
        mission::seed(&mut doc).unwrap();
        doc.commit("seed");
        doc
    }
    #[test]
    fn comparison_edits_preserve_extension_bytes_and_use_little_endian_words() {
        let raw = "A5".repeat(32);
        let edited = with_comparison(&raw, -123, 3).unwrap();
        assert_eq!(compare_words(&edited).unwrap(), (-123, 3));
        assert_eq!(&edited[16..], &raw[16..]);
        assert!(with_comparison(&raw, 0, 6).is_err());
        assert!(comparison(&"zz".repeat(32)).is_err());
    }
    #[test]
    fn ai_trigger_and_enable_roundtrip_through_engine_and_mode_specific_defaults() {
        let mut doc = document();
        let before = doc.snapshot().bytes().unwrap();
        mission::create(&mut doc, Kind::AiTrigger, "MAP_AI", "Russians", None).unwrap();
        let mut f = decode(&doc.ini_text("AITriggerTypes", "MAP_AI")).unwrap();
        f[4] = "1".into();
        f[5] = "E2".into();
        f[6] = with_comparison(&f[6], 2, 3).unwrap();
        f[15] = "1".into();
        f[16] = "0".into();
        f[17] = "1".into();
        mission::write_fields(
            &mut doc,
            Kind::AiTrigger,
            "MAP_AI",
            &[("AITriggerTypes".into(), f.join(",").into_bytes())],
        )
        .unwrap();
        doc.commit("AI");
        let saved = doc.snapshot().bytes().unwrap();
        let map = map_file::load_from_bytes(saved.clone()).unwrap();
        let registry = TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &map.ini, false);
        let ai = registry
            .ai_triggers
            .iter()
            .find(|r| r.id == "MAP_AI")
            .unwrap();
        assert!(ai.enabled);
        assert_eq!(ai.condition, 1);
        assert_eq!(ai.object_type.as_deref(), Some("E2"));
        assert_eq!(ai.difficulty_enabled, [true, false, true]);
        assert_eq!(ai.comparison_mask[0], 2);
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), saved);
        doc.set_ini("AITriggerTypesEnable", "MAP_AI", Some(b"no".to_vec()))
            .unwrap();
        let ini = map_file::load_from_bytes(doc.snapshot().bytes().unwrap())
            .unwrap()
            .ini;
        assert!(
            !TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &ini, false).ai_triggers[0]
                .enabled
        );
        assert!(
            TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &ini, true).ai_triggers[0]
                .enabled
        );
        doc.set_ini("AITriggerTypesEnable", "MAP_AI", None).unwrap();
        let ini = map_file::load_from_bytes(doc.snapshot().bytes().unwrap())
            .unwrap()
            .ini;
        assert!(
            !TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &ini, false).ai_triggers[0]
                .enabled
        );
    }
    #[test]
    fn team_rename_copy_and_delete_track_both_ai_team_columns_and_enable() {
        let mut doc = document();
        let team = mission::ids(&doc, Kind::Team)[0].clone();
        mission::create(&mut doc, Kind::AiTrigger, "MAP_AI", "Russians", None).unwrap();
        let mut f = decode(&doc.ini_text("AITriggerTypes", "MAP_AI")).unwrap();
        f[14] = team.clone();
        doc.set_ini("AITriggerTypes", "MAP_AI", Some(f.join(",").into_bytes()))
            .unwrap();
        doc.commit("AI");
        assert!(mission::delete(&mut doc, Kind::Team, &team).is_err());
        mission::graph::rename(&mut doc, Kind::Team, &team, "NEW_TEAM").unwrap();
        doc.commit("rename");
        let f = decode(&doc.ini_text("AITriggerTypes", "MAP_AI")).unwrap();
        assert_eq!(
            (&f[1], &f[14]),
            (&"NEW_TEAM".to_owned(), &"NEW_TEAM".to_owned())
        );
        mission::graph::copy(&mut doc, Kind::AiTrigger, "MAP_AI", "COPY_AI", true).unwrap();
        doc.commit("copy");
        let copied = decode(&doc.ini_text("AITriggerTypes", "COPY_AI")).unwrap();
        assert_eq!(copied[1], copied[14]);
        assert_ne!(copied[1], "NEW_TEAM");
        assert_eq!(doc.ini_text("AITriggerTypesEnable", "COPY_AI"), "yes");
        mission::graph::rename(&mut doc, Kind::AiTrigger, "MAP_AI", "RENAMED_AI").unwrap();
        doc.commit("rename AI");
        assert!(doc.ini_value("AITriggerTypesEnable", "MAP_AI").is_none());
        assert_eq!(doc.ini_text("AITriggerTypesEnable", "RENAMED_AI"), "yes");
        mission::delete(&mut doc, Kind::AiTrigger, "RENAMED_AI").unwrap();
        doc.commit("delete AI");
        assert!(
            doc.ini_value("AITriggerTypesEnable", "RENAMED_AI")
                .is_none()
        );
    }
    #[test]
    fn fixed_enable_candidates_and_diagnostics_distinguish_map_and_unknown_extensions() {
        let mut doc = document();
        let mut fixed = mission::ExternalDefinitions {
            source: "aimd.ini".into(),
            ..Default::default()
        };
        fixed.ai_triggers.insert("FIXED_AI".into());
        doc.set_ini("AITriggerTypesEnable", "fixed_ai", Some(b"no".to_vec()))
            .unwrap();
        assert_eq!(
            fixed.candidates(&doc, Kind::AiTrigger),
            vec![("fixed_ai".into(), false)]
        );
        assert!(
            !mission::diagnostics(&doc, Some(&fixed))
                .iter()
                .any(|d| d.issue.kind == Kind::AiTrigger)
        );
        doc.set_ini(
            "AITriggerTypes",
            "MAP_BAD",
            Some(b"unknown,extension".to_vec()),
        )
        .unwrap();
        doc.commit("extension");
        let before = doc.snapshot().bytes().unwrap();
        assert!(
            mission::diagnostics(&doc, Some(&fixed))
                .iter()
                .any(|d| d.issue.id == "MAP_BAD" && d.severity == mission::Severity::Warning)
        );
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}
