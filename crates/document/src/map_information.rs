//! Map-wide settings read by the pinned engine, independent of geometry and lighting.
//!
//! Owners: map::basic::{parse_basic_section, parse_special_flags_section},
//! map::skirmish_scenarios::SkirmishScenarioRecord::concrete_from_ini,
//! sim::scenario_bootstrap::initialize_campaign_current_house and
//! sim::game_options::GameOptions::from_multiplayer_dialog_settings.
//! MultiplayerOnly is the editor's existing mission/skirmish classification.
use crate::document::MapDocument;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Listing,
    Environment,
    Special,
    Campaign,
    Media,
    Multiplayer,
}

impl Group {
    pub const ALL: [Self; 6] = [
        Self::Listing,
        Self::Environment,
        Self::Special,
        Self::Campaign,
        Self::Media,
        Self::Multiplayer,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Listing => "基本信息",
            Self::Environment => "资源与场景规则",
            Self::Special => "特殊规则",
            Self::Campaign => "单人任务与战役流程",
            Self::Media => "音乐与过场动画",
            Self::Multiplayer => "多人开局默认值",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Boolean,
    Integer(i32, i32),
    Fraction,
    Text(usize),
    Briefing,
    Speed,
}

pub struct Field {
    pub group: Group,
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

const fn basic(group: Group, key: &'static str, label: &'static str, kind: Kind) -> Field {
    Field {
        group,
        section: "Basic",
        key,
        label,
        kind,
    }
}
const fn special(key: &'static str, label: &'static str) -> Field {
    Field {
        group: Group::Special,
        section: "SpecialFlags",
        key,
        label,
        kind: Kind::Boolean,
    }
}
const fn multiplayer(key: &'static str, label: &'static str, kind: Kind) -> Field {
    Field {
        group: Group::Multiplayer,
        section: "MultiplayerDialogSettings",
        key,
        label,
        kind,
    }
}

// Name/Author and MaxPlayers/start waypoints remain owned by the existing
// map-information controls. NewINIFormat is structural, never a metadata edit.
pub const FIELDS: &[Field] = &[
    basic(Group::Listing, "Briefing", "地图说明", Kind::Briefing),
    basic(Group::Listing, "GameMode", "适用游戏模式", Kind::Text(255)),
    basic(
        Group::Listing,
        "MinPlayers",
        "最低玩家人数",
        Kind::Integer(2, 30),
    ),
    basic(Group::Listing, "Official", "标记为官方地图", Kind::Boolean),
    basic(Group::Listing, "MultiplayerOnly", "多人地图", Kind::Boolean),
    basic(
        Group::Environment,
        "TiberiumGrowthEnabled",
        "允许矿石生长机制",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "IceGrowthEnabled",
        "允许冰层生长",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "VeinGrowthEnabled",
        "允许藤蔓生长",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "TiberiumDeathToVisceroid",
        "矿石致死生成变异体（Ares）",
        Kind::Boolean,
    ),
    basic(Group::Environment, "FreeRadar", "免费雷达", Kind::Boolean),
    basic(
        Group::Environment,
        "FillSilos",
        "开局填满矿仓",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "TrainCrate",
        "火车携带箱子",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "TruckCrate",
        "卡车携带箱子",
        Kind::Boolean,
    ),
    basic(
        Group::Environment,
        "IgnoreGlobalAITriggers",
        "忽略全局 AI 触发器",
        Kind::Boolean,
    ),
    special("MCVDeploy", "开局展开基地车"),
    special("InitialVeteran", "初始作战单位精英化"),
    special("TiberiumGrows", "矿石变密"),
    special("TiberiumSpreads", "矿石向邻格扩散"),
    special("DestroyableBridges", "桥梁可摧毁"),
    special("Inert", "禁止世界伤害"),
    special("FogOfWar", "特殊规则战争迷雾"),
    special("HarvesterImmune", "矿车免于自动索敌"),
    basic(Group::Campaign, "Player", "任务玩家所属方", Kind::Text(19)),
    basic(
        Group::Campaign,
        "HomeCell",
        "初始视野航点",
        Kind::Integer(0, i32::MAX),
    ),
    basic(
        Group::Campaign,
        "AltHomeCell",
        "备用初始视野航点",
        Kind::Integer(0, i32::MAX),
    ),
    basic(
        Group::Campaign,
        "NextScenario",
        "下一任务地图",
        Kind::Text(259),
    ),
    basic(
        Group::Campaign,
        "AltNextScenario",
        "备用下一任务地图",
        Kind::Text(259),
    ),
    basic(
        Group::Campaign,
        "TimerInherit",
        "继承上一任务计时器",
        Kind::Boolean,
    ),
    basic(
        Group::Campaign,
        "CarryOverMoney",
        "继承资金比例",
        Kind::Fraction,
    ),
    basic(
        Group::Campaign,
        "CarryOverCap",
        "继承资金上限",
        Kind::Integer(-1, i32::MAX),
    ),
    basic(Group::Campaign, "SkipScore", "跳过任务评分", Kind::Boolean),
    basic(
        Group::Campaign,
        "EndOfGame",
        "胜利后结束战役",
        Kind::Boolean,
    ),
    basic(
        Group::Campaign,
        "OneTimeOnly",
        "胜利后返回主菜单",
        Kind::Boolean,
    ),
    basic(
        Group::Campaign,
        "SkipMapSelect",
        "跳过战役选图",
        Kind::Boolean,
    ),
    basic(Group::Media, "Theme", "开局音乐", Kind::Text(127)),
    basic(Group::Media, "Intro", "开场动画", Kind::Text(127)),
    basic(Group::Media, "Brief", "简报动画", Kind::Text(127)),
    basic(Group::Media, "Action", "任务开始动画", Kind::Text(127)),
    basic(Group::Media, "Win", "胜利动画", Kind::Text(127)),
    basic(Group::Media, "Lose", "失败动画", Kind::Text(127)),
    basic(Group::Media, "PostScore", "评分后动画", Kind::Text(127)),
    basic(Group::Media, "PreMapSelect", "选图前动画", Kind::Text(127)),
    multiplayer("Bases", "开局有基地车", Kind::Boolean),
    multiplayer("Money", "开局资金默认值", Kind::Integer(0, i32::MAX)),
    multiplayer(
        "UnitCount",
        "开局部队数量默认值",
        Kind::Integer(0, i32::MAX),
    ),
    multiplayer("TechLevel", "科技等级默认值", Kind::Integer(0, 10)),
    multiplayer("GameSpeed", "游戏速度默认值", Kind::Speed),
    multiplayer("BridgeDestruction", "多人桥梁可摧毁", Kind::Boolean),
    multiplayer("Shroud", "开局黑幕", Kind::Boolean),
    multiplayer("TiberiumGrows", "多人矿石生长", Kind::Boolean),
    multiplayer("Crates", "随机箱子", Kind::Boolean),
    multiplayer("HarvesterTruce", "矿车停战", Kind::Boolean),
    multiplayer("MultiEngineer", "多工程师占领", Kind::Boolean),
    multiplayer("AllyChangeAllowed", "允许修改结盟", Kind::Boolean),
    multiplayer("ShortGame", "快速游戏", Kind::Boolean),
    multiplayer("SuperWeaponsAllowed", "允许超级武器", Kind::Boolean),
    multiplayer("BuildOffAlly", "允许依附盟友建造", Kind::Boolean),
    multiplayer("FogOfWar", "多人战争迷雾", Kind::Boolean),
    multiplayer("MCVRedeploys", "允许重新部署基地车", Kind::Boolean),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub values: BTreeMap<(&'static str, &'static str), String>,
}

impl Draft {
    pub fn read(doc: &MapDocument) -> Self {
        Self {
            values: FIELDS
                .iter()
                .map(|field| {
                    let mut value = doc.ini_text(field.section, field.key);
                    if matches!(field.kind, Kind::Briefing) {
                        value = value.replace('@', "\n");
                    }
                    ((field.section, field.key), value)
                })
                .collect(),
        }
    }

    /// Preserve existing Mod/legacy values; validate only deliberate changes.
    pub fn validate(&self, original: &Self) -> Result<()> {
        ensure!(self.values.len() == FIELDS.len(), "地图信息字段不完整");
        for field in FIELDS {
            let value = self
                .values
                .get(&(field.section, field.key))
                .context("缺少地图信息字段")?;
            if Some(value) == original.values.get(&(field.section, field.key)) {
                continue;
            }
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            ensure!(
                !value.contains(';')
                    && !value.chars().any(|c| c.is_control()
                        && !(matches!(field.kind, Kind::Briefing) && matches!(c, '\n' | '\r'))),
                "{}包含不支持的分号或控制字符",
                field.label
            );
            match field.kind {
                Kind::Boolean => ensure!(
                    matches!(value, "yes" | "no"),
                    "{}须为开启、关闭或默认",
                    field.label
                ),
                Kind::Integer(min, max) => {
                    let number = value
                        .parse::<i32>()
                        .with_context(|| format!("{}须为整数", field.label))?;
                    ensure!(
                        (min..=max).contains(&number),
                        "{}须在 {min}～{max} 之间",
                        field.label
                    );
                }
                Kind::Fraction => {
                    let number = value.trim_end_matches('%').parse::<f64>()?
                        / if value.ends_with('%') { 100. } else { 1. };
                    ensure!(
                        number.is_finite() && (0.0..=1.0).contains(&number),
                        "继承资金比例须在 0～1 或 0%～100% 之间"
                    );
                }
                Kind::Text(max) => ensure!(value.len() <= max, "{}最多 {max} 字节", field.label),
                Kind::Briefing => {
                    ensure!(value.chars().count() <= 2000, "地图说明最多 2000 个字符")
                }
                Kind::Speed => {
                    let speed = value
                        .parse::<u8>()
                        .context("游戏速度须为 0～6 或 8（90 帧）")?;
                    ensure!(
                        vera20k_core::sim::game_options::is_selectable_game_speed(speed),
                        "游戏速度须为 0～6 或 8（90 帧）"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn apply(&self, doc: &mut MapDocument) -> Result<()> {
        let original = Self::read(doc);
        self.validate(&original)?;
        let min_id = ("Basic", "MinPlayers");
        if self.values[&min_id] != original.values[&min_id]
            && let Ok(min) = self.values[&min_id].trim().parse::<i32>()
        {
            let maximum = doc.ini_text("Basic", "MaxPlayers");
            let ini = vera20k_core::rules::ini_parser::IniFile::from_str(&format!(
                "[Basic]\nMaxPlayers={maximum}\n"
            ));
            let max = ini.section_or_empty("Basic").read_int("MaxPlayers", -1);
            ensure!(max < 2 || min <= max, "最低玩家人数不能超过出生点数量");
        }
        doc.atomic(|doc| {
            for field in FIELDS {
                let id = (field.section, field.key);
                if self.values.get(&id) == original.values.get(&id) {
                    continue;
                }
                let value = self.values[&id]
                    .trim()
                    .replace("\r\n", "\n")
                    .replace('\r', "\n");
                let value = if matches!(field.kind, Kind::Briefing) {
                    value.replace('\n', "@")
                } else {
                    value
                };
                doc.set_ini(
                    field.section,
                    field.key,
                    (!value.is_empty()).then(|| value.into_bytes()),
                )?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Baseline, new_map::NewMapOptions};
    use vera20k_core::{
        map::{basic, map_file},
        sim::game_options::GameOptions,
    };

    fn document() -> MapDocument {
        let mut bytes = NewMapOptions::default().bytes().unwrap();
        bytes.extend_from_slice(
            b"\r\n[SpecialFlags]\r\nInert=custom\r\n[ModData]\r\nOpaque=\x81\xfe ; keep\r\n",
        );
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap())
    }

    #[test]
    fn map_information_roundtrips_through_engine_readers_and_history() {
        let mut doc = document();
        let before = doc.snapshot().bytes().unwrap();
        let opaque = doc.ini_value("ModData", "Opaque");
        let mut draft = Draft::read(&doc);
        draft.apply(&mut doc).unwrap();
        assert_eq!(
            doc.snapshot().bytes().unwrap(),
            before,
            "opening writes no defaults"
        );
        for (section, key, value) in [
            ("Basic", "Briefing", "守住基地\n等待增援"),
            ("Basic", "FreeRadar", "yes"),
            ("Basic", "TiberiumGrowthEnabled", "no"),
            ("Basic", "HomeCell", "699"),
            ("Basic", "CarryOverMoney", "50%"),
            ("Basic", "CarryOverCap", "-1"),
            ("Basic", "NextScenario", "NEXT.MAP"),
            ("Basic", "Intro", "INTRO"),
            ("Basic", "GameMode", "standard,teamgame"),
            ("Basic", "MinPlayers", "2"),
            ("SpecialFlags", "MCVDeploy", "yes"),
            ("SpecialFlags", "TiberiumSpreads", "no"),
            ("MultiplayerDialogSettings", "Money", "12000"),
            ("MultiplayerDialogSettings", "Bases", "no"),
            ("MultiplayerDialogSettings", "SuperWeaponsAllowed", "no"),
            ("MultiplayerDialogSettings", "GameSpeed", "8"),
        ] {
            draft.values.insert((section, key), value.into());
        }
        draft.apply(&mut doc).unwrap();
        doc.commit("map information");
        let bytes = doc.snapshot().bytes().unwrap();
        let map = map_file::load_from_bytes(bytes.clone()).unwrap();
        assert_eq!(map.basic.free_radar, Some(true));
        assert_eq!(map.basic.tiberium_growth_enabled, Some(false));
        assert_eq!(map.basic.home_cell, Some(699));
        assert_eq!(
            f64::from_bits(map.basic.carry_over_money.unwrap().bits()),
            0.5
        );
        assert_eq!(map.basic.carry_over_cap, Some(-1));
        assert_eq!(map.basic.next_scenario.as_deref(), Some("NEXT.MAP"));
        assert_eq!(map.basic.intro.as_deref(), Some("INTRO"));
        assert_eq!(map.special_flags.mcv_deploy, Some(true));
        assert_eq!(map.special_flags.tiberium_spreads, Some(false));
        assert_eq!(
            basic::briefing_text(&bytes).as_deref(),
            Some("守住基地\n等待增援")
        );
        let options = GameOptions::from_multiplayer_dialog_settings(&map.ini);
        assert_eq!(options.starting_credits, 12000);
        assert!(!options.bases);
        assert!(!options.super_weapons);
        assert_eq!(options.game_speed, 8);
        let record =
            vera20k_core::map::skirmish_scenarios::SkirmishScenarioRecord::concrete_from_ini(
                0,
                vera20k_core::map::skirmish_scenarios::SkirmishScenarioSource::Synthetic,
                "test.map",
                &map.ini,
            );
        assert_eq!(record.game_modes, ["standard", "teamgame"]);
        assert_eq!(record.min_players, Some(2));
        let reopened = MapDocument::new(Baseline::parse(bytes.clone().into(), &map).unwrap());
        assert_eq!(Draft::read(&reopened), draft);
        assert_eq!(reopened.ini_value("ModData", "Opaque"), opaque);
        assert_eq!(doc.ini_text("SpecialFlags", "Inert"), "custom");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.redo();
        assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    }

    #[test]
    fn map_information_defaults_remove_overrides_and_invalid_edits_are_atomic() {
        let mut doc = document();
        doc.set_ini("Basic", "FreeRadar", Some(b"yes".to_vec()))
            .unwrap();
        doc.set_ini("MultiplayerDialogSettings", "Bases", Some(b"no".to_vec()))
            .unwrap();
        doc.commit("radar");
        let original = Draft::read(&doc);
        let before = doc.snapshot().bytes().unwrap();
        for (key, value) in [
            ("CarryOverMoney", "NaN"),
            ("NextScenario", "ok\n[Basic]"),
            ("HomeCell", "-1"),
            ("MinPlayers", "30"),
        ] {
            let mut draft = original.clone();
            draft.values.insert(("Basic", "FillSilos"), "yes".into());
            draft.values.insert(("Basic", key), value.into());
            assert!(draft.apply(&mut doc).is_err());
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
        let mut draft = original;
        draft.values.insert(("Basic", "FreeRadar"), String::new());
        draft
            .values
            .insert(("MultiplayerDialogSettings", "Bases"), String::new());
        draft.apply(&mut doc).unwrap();
        let map = map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
        assert_eq!(map.basic.free_radar, None, "default removes the override");
        assert!(
            !map.ini
                .section_or_empty("MultiplayerDialogSettings")
                .is_present("Bases")
        );
        assert!(GameOptions::from_multiplayer_dialog_settings(&map.ini).bases);
    }
}
