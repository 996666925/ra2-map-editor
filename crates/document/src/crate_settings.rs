//! Map-local overrides for the engine's `[CrateRules]` reader.
use crate::document::MapDocument;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub integer: bool,
    pub min: f64,
    pub max: f64,
}

pub const FIELDS: [Field; 5] = [
    Field {
        key: "CrateMinimum",
        label: "最少箱子数",
        integer: true,
        min: 0.,
        max: 256.,
        hint: "初始箱子数量下限；原版参考值 1。上下限相同可指定固定数量。",
    },
    Field {
        key: "CrateMaximum",
        label: "最多箱子数",
        integer: true,
        min: 0.,
        max: 256.,
        hint: "初始箱子数量上限；原版参考值 255。设为 0 可禁止开局随机生成箱子。",
    },
    Field {
        key: "CrateRegen",
        label: "刷新间隔（游戏分钟）",
        integer: false,
        min: 0.002223,
        max: 1_000_000.,
        hint: "数值越小，未拾取箱子换位置越频繁；原版参考值 3。实际在约 0.5～2 倍间隔内随机。",
    },
    Field {
        key: "CrateRadius",
        label: "效果范围（格）",
        integer: false,
        min: 0.,
        max: 1024.,
        hint: "影响范围型箱子奖励的作用半径；原版参考值 3。",
    },
    Field {
        key: "SoloCrateMoney",
        label: "单人金钱奖励",
        integer: true,
        min: 0.,
        max: i32::MAX as f64,
        hint: "单人任务中金钱箱子的固定金额；原版参考值 2000。设为 0 使用随机金额。遭遇战奖励由 Powerups 决定。",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub values: BTreeMap<String, String>,
}

impl Draft {
    pub fn read(doc: &MapDocument) -> Self {
        Self {
            values: FIELDS
                .iter()
                .map(|f| (f.key.into(), doc.ini_text("CrateRules", f.key)))
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        for (key, value) in &self.values {
            let field = FIELDS
                .iter()
                .find(|f| f.key == key)
                .context("未知箱子设置字段")?;
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            let n = if field.integer {
                f64::from(
                    value
                        .parse::<i32>()
                        .with_context(|| format!("{}须为整数", field.label))?,
                )
            } else {
                value
                    .parse::<f64>()
                    .with_context(|| format!("{}须为数值", field.label))?
            };
            ensure!(
                n.is_finite() && (field.min..=field.max).contains(&n),
                "{}超出范围（{}～{}）",
                field.label,
                field.min,
                field.max
            );
        }
        let count = |key: &str| {
            self.values
                .get(key)
                .and_then(|v| v.trim().parse::<i32>().ok())
        };
        if let (Some(min), Some(max)) = (count("CrateMinimum"), count("CrateMaximum")) {
            // Maximum zero is a useful explicit off switch, even with an inherited minimum.
            ensure!(max == 0 || min <= max, "最少箱子数不能大于最多箱子数");
        }
        Ok(())
    }

    pub fn apply(&self, doc: &mut MapDocument) -> Result<()> {
        self.validate()?;
        doc.atomic(|doc| {
            for (key, value) in &self.values {
                if *value == doc.ini_text("CrateRules", key) {
                    continue;
                }
                let value = value.trim();
                doc.set_ini(
                    "CrateRules",
                    key,
                    (!value.is_empty()).then(|| value.as_bytes().to_vec()),
                )?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Baseline;
    use vera20k_core::{
        map::map_file,
        rules::{
            ini_parser::IniFile,
            native_processing::{RulesLayerKind, RulesLayerStack},
        },
    };

    #[test]
    fn crate_settings_roundtrip_through_engine_and_undo_without_changing_other_rules() {
        let mut doc = crate::test_support::document();
        doc.set_ini("CrateRules", "WoodCrate", Some(b"Veteran".to_vec()))
            .unwrap();
        doc.set_ini("CrateRules", "ModExtension", Some(b"keep".to_vec()))
            .unwrap();
        doc.commit("fixture");
        let before = doc.snapshot().bytes().unwrap();
        let mut draft = Draft::read(&doc);
        for (key, value) in [
            ("CrateMinimum", "8"),
            ("CrateMaximum", "8"),
            ("CrateRegen", "0.5"),
            ("CrateRadius", "4.5"),
            ("SoloCrateMoney", "3500"),
        ] {
            draft.values.insert(key.into(), value.into());
        }
        draft.apply(&mut doc).unwrap();
        doc.commit("箱子设置");
        let after = doc.snapshot().bytes().unwrap();
        let mut stack = RulesLayerStack::new(IniFile::from_str(
            "[CrateRules]\nCrateMinimum=1\nCrateMaximum=255\nCrateRegen=3\n",
        ));
        stack.push(
            RulesLayerKind::Scenario,
            IniFile::from_bytes(&after).unwrap(),
        );
        let rules = stack.process().unwrap();
        let crates = rules.crate_rules();
        assert_eq!((crates.minimum, crates.maximum), (8, 8));
        assert_eq!(f64::from_bits(crates.regen.bits()), 0.5);
        assert_eq!(crates.radius, 1152);
        assert_eq!(crates.solo_crate_money, 3500);
        assert_eq!(doc.ini_text("CrateRules", "WoodCrate"), "Veteran");
        assert_eq!(doc.ini_text("CrateRules", "ModExtension"), "keep");
        let map = map_file::load_from_bytes(after.clone()).unwrap();
        let reopened = MapDocument::new(Baseline::parse(after.clone().into(), &map).unwrap());
        assert_eq!(Draft::read(&reopened), draft);
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), after);
        draft.values.insert("CrateRegen".into(), String::new());
        draft.apply(&mut doc).unwrap();
        assert!(doc.ini_text("CrateRules", "CrateRegen").is_empty());
    }

    #[test]
    fn invalid_crate_settings_are_atomic_and_blank_fields_stay_inherited() {
        let mut doc = crate::test_support::document();
        let before = doc.snapshot().bytes().unwrap();
        Draft::read(&doc).apply(&mut doc).unwrap();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        for (key, value) in [
            ("CrateMinimum", "-1"),
            ("CrateMaximum", "257"),
            ("CrateMaximum", "1.5"),
            ("CrateRegen", "0"),
            ("CrateRegen", "NaN"),
            ("CrateRegen", "inf"),
            ("CrateRegen", "1e-12"),
            ("CrateRadius", "-1"),
            ("SoloCrateMoney", "2147483648"),
        ] {
            let mut draft = Draft::read(&doc);
            draft.values.insert("CrateMinimum".into(), "3".into());
            draft.values.insert(key.into(), value.into());
            assert!(draft.apply(&mut doc).is_err(), "{key}={value}");
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
        let mut draft = Draft::read(&doc);
        draft.values.insert("CrateMinimum".into(), "10".into());
        draft.values.insert("CrateMaximum".into(), "5".into());
        assert!(draft.validate().is_err());
        draft.values.insert("CrateMaximum".into(), "0".into());
        assert!(draft.validate().is_ok());
    }
}
