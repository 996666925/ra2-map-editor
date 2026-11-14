//! Capability inventory for the pinned engine. A dispatch arm is not a promise
//! of supported parameters, notifications or presentation in every mode.
use ra2_map_document::mission::schema::{self, Opcode, ParameterKind};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Family {
    Event,
    Action,
    Order,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSupport {
    /// An actual handler exists; parameters/modes still need a schema audit.
    Handler,
    /// The engine explicitly ignores the opcode. Never offered as a feature.
    NoOp,
    Unsupported,
    /// Ares extension handler; the editor requires its Ares authoring build.
    Ares,
    /// The known handler explicitly refuses network games (Script22).
    SinglePlayer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SdkCoverage {
    Missing,
    Partial,
    Complete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Binding {
    Logic,
    LogicOrTag,
    TagNotification,
    CellOrObject,
    Object,
    House,
    Team,
    LiveTrigger,
    AuditRequired,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub index: usize,
    pub label: &'static str,
    /// Includes reference family, numerical limits, encoding and units.
    pub encoding: Operand,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Operand {
    Integer {
        min: i32,
        max: i32,
        unit: &'static str,
    },
    Reference {
        registry: &'static str,
    },
    Waypoint {
        letters: bool,
    },
    Country,
    Type {
        registry: &'static str,
    },
    Csf,
    Sound,
    Eva,
    Theme,
    Cell128,
}
impl From<ParameterKind> for Operand {
    fn from(value: ParameterKind) -> Self {
        match value {
            ParameterKind::Integer { min, max, unit } => Self::Integer { min, max, unit },
            ParameterKind::Reference(kind) => Self::Reference {
                registry: kind.registry(),
            },
            ParameterKind::Waypoint { letters } => Self::Waypoint { letters },
            ParameterKind::Country => Self::Country,
            ParameterKind::Type(registry) => Self::Type { registry },
            ParameterKind::Csf => Self::Csf,
            ParameterKind::Sound => Self::Sound,
            ParameterKind::Eva => Self::Eva,
            ParameterKind::Theme => Self::Theme,
            ParameterKind::Cell128 => Self::Cell128,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub family: Family,
    pub opcode: i32,
    pub name: Option<&'static str>,
    pub runtime: RuntimeSupport,
    /// Whether this editor build contains the required engine capability.
    pub enabled: bool,
    pub sdk: SdkCoverage,
    /// False means that the dispatch arm is inventoried, not fully audited.
    pub schema_audited: bool,
    pub binding: Binding,
    pub parameter_type: Option<i32>,
    pub parameters: Option<Vec<Parameter>>,
    pub notes: Option<&'static str>,
    pub implementation: &'static str,
}

const ACTION_HANDLERS: &[i32] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43, 44, 45, 46, 47, 48, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78,
    79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103,
    104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122,
    123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141,
    142, 143, 144, 145,
];
const ORDER_HANDLERS: &[i32] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 30, 31, 32, 33, 34, 37, 38, 39, 40, 41, 42, 43, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
    57, 58, 59, 60, 61, 62, 63, 64,
];

fn named(family: Family, id: i32) -> Option<&'static Opcode> {
    match family {
        Family::Event => schema::opcode(true, id),
        Family::Action => schema::opcode(false, id),
        Family::Order => schema::scripts::opcode(id),
    }
    .or_else(|| {
        match family {
            Family::Event => schema::ares::EVENTS,
            Family::Action => schema::ares::ACTIONS,
            Family::Order => schema::ares::ORDERS,
        }
        .iter()
        .find(|entry| entry.id == id)
    })
}

pub fn capability(family: Family, id: i32) -> Capability {
    let schema = named(family, id);
    let runtime = match family {
        Family::Event => match id {
            33 | 44 => RuntimeSupport::SinglePlayer,
            1..=17 | 19..=61 => RuntimeSupport::Handler,
            62..=65 | 67..=88 => RuntimeSupport::Ares,
            _ => RuntimeSupport::Unsupported,
        },
        Family::Action => {
            if matches!(id, 1 | 2 | 10 | 67 | 68 | 69 | 110) {
                RuntimeSupport::SinglePlayer
            } else if id == 0 {
                RuntimeSupport::NoOp
            } else if matches!(id, 92 | 93 | 146..=149) {
                RuntimeSupport::Ares
            } else if ACTION_HANDLERS.contains(&id) {
                RuntimeSupport::Handler
            } else {
                RuntimeSupport::Unsupported
            }
        }
        Family::Order => {
            if id == 22 {
                RuntimeSupport::SinglePlayer
            } else if ORDER_HANDLERS.contains(&id) {
                RuntimeSupport::Handler
            } else if matches!(id, 67 | 68) {
                RuntimeSupport::Ares
            } else {
                RuntimeSupport::Unsupported
            }
        }
    };
    let sdk = match (family, id) {
        (Family::Event, 62..=65 | 67..=88)
        | (Family::Action, 92 | 93 | 146..=149)
        | (Family::Order, 67 | 68) => SdkCoverage::Complete,
        (Family::Event, 1 | 4..=17 | 19..=61)
        | (
            Family::Action,
            1..=8
            | 11
            | 14
            | 16
            | 19..=29
            | 53
            | 54
            | 56
            | 57
            | 67
            | 68
            | 74
            | 75
            | 80
            | 103
            | 104
            | 107
            | 108,
        )
        | (Family::Action, 137 | 138)
        | (Family::Action, 52)
        | (Family::Order, 10 | 13 | 22 | 30 | 34 | 46 | 47 | 51..=64)
        | (Family::Action, 41 | 42 | 43 | 88)
        | (
            Family::Action,
            9
            | 12
            | 13
            | 30
            | 35
            | 37
            | 38
            | 58
            | 59
            | 63..=66
            | 69..=73
            | 76..=79
            | 85..=87
            | 89
            | 94..=97
            | 106
            | 113
            | 118
            | 142..=144,
        )
        | (
            Family::Action,
            10
            | 32..=34
            | 36
            | 44..=47
            | 60..=62
            | 81..=84
            | 90
            | 91
            | 98..=100
            | 102
            | 105
            | 109..=111
            | 116..=117
            | 119..=136
            | 140
            | 141,
        )
        | (Family::Action, 17 | 18 | 31 | 40 | 48 | 51 | 55 | 101 | 112 | 114 | 115 | 139 | 145)
        | (
            Family::Order,
            0..=9 | 11 | 12 | 14..=17 | 19..=21 | 23..=27 | 31..=33 | 37..=43 | 48..=50,
        ) => SdkCoverage::Complete,
        _ => SdkCoverage::Missing,
    };
    let binding = match (family, id) {
        (Family::Event, 62..=65 | 67..=74 | 79 | 80 | 83 | 84) => Binding::Object,
        (Family::Event, 75..=78 | 81 | 82 | 85 | 87 | 88) => Binding::House,
        (Family::Event, 86) => Binding::Logic,
        (Family::Action, 147 | 149) => Binding::Object,
        (Family::Event, 1 | 24 | 31) => Binding::CellOrObject,
        (Family::Event, 25 | 26 | 53 | 54 | 59) => Binding::CellOrObject,
        (Family::Event, 4 | 6 | 7 | 29 | 33..=35 | 38..=44 | 48 | 49) => Binding::Object,
        (Family::Event, 2) => Binding::Object,
        (Family::Event, 3 | 5 | 9..=12 | 15..=17 | 19..=22 | 30 | 32 | 52 | 55..=58) => {
            Binding::House
        }
        (Family::Event, 23) => Binding::TagNotification,
        (Family::Event, 8) => Binding::LogicOrTag,
        (Family::Event, 13 | 14 | 27 | 28 | 36 | 37 | 47 | 60 | 61) => Binding::Logic,
        (Family::Event, 45 | 46 | 50 | 51) => Binding::Logic,
        (Family::Action, 14 | 32 | 52 | 60..=62 | 81 | 91 | 105 | 111) => Binding::Object,
        (Family::Action, 3 | 6 | 74 | 75 | 137 | 138) => Binding::LiveTrigger,
        (Family::Action, _) if schema.is_some() => Binding::Logic,
        (Family::Order, _) => Binding::Team,
        _ => Binding::AuditRequired,
    };
    Capability {
        family,
        opcode: id,
        name: schema.map(|s| s.name),
        runtime,
        enabled: runtime != RuntimeSupport::Unsupported
            && (runtime != RuntimeSupport::Ares || vera20k_core::rules::ARES_BUILD),
        sdk,
        schema_audited: schema.is_some(),
        binding,
        parameter_type: schema.map(|s| s.parameter_type),
        parameters: schema.map(|s| {
            s.parameters
                .iter()
                .map(|p| Parameter {
                    index: p.index,
                    label: p.label,
                    encoding: p.kind.into(),
                })
                .collect()
        }),
        notes: match (family, id) {
            (Family::Event, 66) => Some("Ares 只有恒真谓词，没有对应的生产通知；不提供 SDK 接口。"),
            (Family::Action, 1) => Some(
                "仅单人；比较 Country 与当前玩家 House 的 Country，相同=玩家胜、不同=玩家负。结果属于 PlayerPtr，不是触发器 owner。",
            ),
            (Family::Action, 2) => Some(
                "仅单人；比较 Country 与当前玩家 House 的 Country，相同=玩家负、不同=玩家胜。结果属于 PlayerPtr，不是触发器 owner。",
            ),
            (Family::Action, 67) => Some(
                "仅单人；立即宣告当前玩家胜利并跳过 SavourDelay 等待，不因触发器 owner 改变结果对象。",
            ),
            (Family::Action, 68) => Some(
                "仅单人；立即宣告当前玩家失败并跳过 SavourDelay 等待，不因触发器 owner 改变结果对象。",
            ),
            (Family::Action, 69) => Some(
                "仅单人；无既有结果时声明玩家胜利并等待 SavourDelay；已有结果时恢复其暂停的计时器。",
            ),
            (Family::Order, 7) => Some(
                "仅单人；Team 所属 House 不决定结果对象，始终作用于当前 Player，经过 SavourDelay。",
            ),
            (Family::Order, 23) => Some(
                "仅单人；Team 所属 House 不决定结果对象，始终作用于当前 Player，经过 SavourDelay。",
            ),
            (Family::Event, 7) => Some(
                "对象销毁通知要求存在伤害来源；Script31 无来源杀死不会发布7，步兵可发布48/29。",
            ),
            (Family::Order, 31) => Some("无攻击来源的伤害链；计入损失，但不发布要求来源的事件7。"),
            (Family::Order, 37) => {
                Some("Limbo/UnInit 移除，不经过伤害/RecordKill，不保证发布事件7/48/29。")
            }
            (Family::Event, 8) => Some(
                "始终为真；注册到 Logic/House/Map 及对象通知，无显式绑定也可在常规轮询触发。一次性共享 Tag 可能因此提前消耗。",
            ),
            (Family::Event, 2) => {
                Some("只有条件处理器，固定引擎的真实渗透路径没有事件2通知；不提供默认 SDK 接口。")
            }
            (Family::Event, 3) => Some(
                "原占领 House 的通知状态已写入，但条件还要求事件3通知；固定引擎没有该生产入口，不能用普通 House 轮询替代。",
            ),
            (Family::Event, 44) => {
                Some("伤害来源的实际 House 数组索引；SDK 仅单人，不能替换为 Country 序号。")
            }
            _ => schema.map(|s| s.hint),
        },
        implementation: match family {
            Family::Event | Family::Action if runtime == RuntimeSupport::Ares => {
                "vendor/vera20k/src/sim/trigger_runtime_ares.rs"
            }
            Family::Event => {
                "vendor/vera20k/src/sim/trigger_runtime.rs:has_occurred/evaluate_event"
            }
            Family::Action => {
                "vendor/vera20k/src/sim/trigger_runtime.rs:apply_live_trigger_action/execute_live_trigger_action"
            }
            Family::Order => {
                "vendor/vera20k/src/sim/team_script_vm/actions.rs:action_is_ported/team_step_script"
            }
        },
    }
}

/// Includes holes and no-ops so omission cannot silently become support.
/// Unknown IDs outside these pinned ranges still return Unsupported via capability().
pub fn matrix() -> Vec<Capability> {
    [
        (Family::Event, 88),
        (Family::Action, 149),
        (Family::Order, 68),
    ]
    .into_iter()
    .flat_map(|(family, max)| (0..=max).map(move |id| capability(family, id)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn unsupported_defaults_noops_and_mode_gates_are_not_sdk_features() {
        assert_eq!(
            capability(Family::Event, 999).runtime,
            RuntimeSupport::Unsupported
        );
        assert_eq!(
            capability(Family::Event, 18).runtime,
            RuntimeSupport::Unsupported
        );
        assert_eq!(capability(Family::Action, 0).runtime, RuntimeSupport::NoOp);
        assert_eq!(
            capability(Family::Order, 22).runtime,
            RuntimeSupport::SinglePlayer
        );
        assert_eq!(capability(Family::Order, 67).runtime, RuntimeSupport::Ares);
        assert_eq!(capability(Family::Order, 11).sdk, SdkCoverage::Complete);
        for entry in matrix()
            .into_iter()
            .filter(|entry| entry.sdk != SdkCoverage::Missing)
        {
            assert!(entry.schema_audited);
            assert!(matches!(
                entry.runtime,
                RuntimeSupport::Handler | RuntimeSupport::SinglePlayer | RuntimeSupport::Ares
            ));
        }
    }

    #[test]
    fn event_handlers_are_exposed_only_when_the_runtime_can_raise_them() {
        let supported: BTreeSet<_> = (1..=17).chain(19..=61).collect();
        let catalogued: BTreeSet<_> = schema::EVENTS.iter().map(|entry| entry.id).collect();
        let sdk: BTreeSet<_> = supported
            .difference(&BTreeSet::from([2, 3]))
            .copied()
            .collect();

        assert_eq!(catalogued, sdk);
        for id in supported {
            let entry = capability(Family::Event, id);
            if matches!(id, 2 | 3) {
                assert_eq!(entry.runtime, RuntimeSupport::Handler);
                assert_eq!(entry.sdk, SdkCoverage::Missing, "event {id}");
                assert!(!entry.schema_audited, "event {id}");
                assert!(entry.notes.is_some(), "event {id} missing reason");
            } else {
                assert_eq!(entry.sdk, SdkCoverage::Complete, "event {id}");
                assert!(entry.schema_audited, "event {id}");
            }
        }
    }

    #[test]
    fn exported_effect_and_camera_operands_match_the_runtime_contract() {
        let matrix = serde_json::to_value(matrix()).unwrap();
        let entry = |family: &str, opcode: i32| {
            matrix
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["family"] == family && entry["opcode"] == opcode)
                .unwrap()
        };
        for (opcode, unit) in [(41, "AnimType 原生数组"), (42, "WeaponType 原生数组")] {
            assert_eq!(
                entry("action", opcode)["parameters"][0]["encoding"]["unit"],
                unit
            );
        }
        for (family, opcode, index, max) in [
            ("action", 58, 2, 3),
            ("action", 48, 2, 4),
            ("order", 34, 1, 4),
        ] {
            let parameter = &entry(family, opcode)["parameters"][0];
            assert_eq!(parameter["index"], index);
            assert_eq!(parameter["encoding"]["min"], 0);
            assert_eq!(parameter["encoding"]["max"], max);
        }
    }

    #[test]
    fn catalog_schemas_are_all_represented_with_their_operands() {
        for (family, entries) in [
            (Family::Event, schema::EVENTS),
            (Family::Action, schema::ACTIONS),
            (Family::Order, schema::scripts::OPCODES),
            (Family::Event, schema::ares::EVENTS),
            (Family::Action, schema::ares::ACTIONS),
            (Family::Order, schema::ares::ORDERS),
        ] {
            for entry in entries {
                let capability = capability(family, entry.id);
                assert!(capability.schema_audited);
                assert_eq!(capability.parameters.unwrap().len(), entry.parameters.len());
                assert_ne!(capability.runtime, RuntimeSupport::Unsupported);
            }
        }
        assert_eq!(matrix().len(), 89 + 150 + 69);
        serde_json::to_string(&matrix()).unwrap();
    }

    #[test]
    fn ares_inventory_matches_the_pinned_extension_dispatchers() {
        let source = include_str!("../../../vendor/vera20k/src/sim/trigger_runtime_ares.rs");
        let arms = |marker: &str| -> BTreeSet<i32> {
            let body = source
                .split(marker)
                .nth(1)
                .unwrap()
                .split("            _ => return None,")
                .next()
                .unwrap();
            let mut ids = BTreeSet::new();
            for line in body.lines() {
                let Some(line) = line
                    .strip_prefix("            ")
                    .filter(|line| !line.starts_with(' '))
                else {
                    continue;
                };
                let Some((pattern, _)) = line.split_once(" =>") else {
                    continue;
                };
                for item in pattern.split('|').map(str::trim) {
                    if let Some((start, end)) = item.split_once("..=") {
                        ids.extend(start.parse::<i32>().unwrap()..=end.parse::<i32>().unwrap());
                    } else if let Ok(id) = item.parse::<i32>() {
                        ids.insert(id);
                    }
                }
            }
            ids
        };
        let mut events = arms("Some(match kind {");
        // 60/61 replace stock predicates; 66 has a true predicate but no
        // production spotlight notification and is deliberately not offered.
        for id in [60, 61, 66] {
            assert!(events.remove(&id));
        }
        assert_eq!(
            events,
            schema::ares::EVENTS.iter().map(|entry| entry.id).collect()
        );
        assert_eq!(
            arms("Some(match action.entry.kind {"),
            schema::ares::ACTIONS.iter().map(|entry| entry.id).collect()
        );
        assert_eq!(
            capability(Family::Event, 66).runtime,
            RuntimeSupport::Unsupported
        );
        for id in [62, 77, 88] {
            assert_eq!(
                capability(Family::Event, id).enabled,
                vera20k_core::rules::ARES_BUILD
            );
        }
    }

    #[test]
    fn house_bindings_follow_the_engine_registration_flags() {
        let source = include_str!("../../../vendor/vera20k/src/sim/trigger_runtime.rs");
        let classification = source
            .split("fn event_classification(")
            .nth(1)
            .unwrap()
            .split("fn action_classification(")
            .next()
            .unwrap();
        let house_pattern = classification
            .split("flags |= 4;")
            .nth(1)
            .unwrap()
            .split("flags |= 8;")
            .next()
            .unwrap();
        let mut registered: BTreeSet<i32> = house_pattern
            .split(|ch: char| !ch.is_ascii_digit())
            .filter(|word| !word.is_empty())
            .map(|word| word.parse().unwrap())
            .collect();
        // Event8 has several registration lanes; event18 is an unsupported
        // legacy number, still present in the engine's classification table.
        assert!(registered.remove(&8));
        assert_eq!(capability(Family::Event, 8).binding, Binding::LogicOrTag);
        assert!(registered.remove(&18));
        assert_eq!(
            capability(Family::Event, 18).runtime,
            RuntimeSupport::Unsupported
        );
        let inventoried = matrix()
            .into_iter()
            .filter(|entry| {
                entry.family == Family::Event
                    && entry.binding == Binding::House
                    && entry.runtime != RuntimeSupport::Ares
            })
            .map(|entry| entry.opcode)
            .collect();
        assert_eq!(registered, inventoried);
    }

    #[test]
    fn action_inventory_tracks_the_pinned_dispatcher() {
        // This tests the integration boundary, not copied opcode expectations:
        // upgrading the engine requires auditing added/removed dispatch arms.
        let source = include_str!("../../../vendor/vera20k/src/sim/trigger_runtime.rs");
        let mut constants: Vec<_> = source
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("const ACTION_")?;
                let (name, value) = rest.split_once(": i32 = ")?;
                Some((
                    format!("ACTION_{name}"),
                    value.trim_end_matches(';').to_owned(),
                ))
            })
            .collect();
        // Replace longer identifiers first (PLAY_SOUND_AT_WAYPOINT contains
        // PLAY_SOUND). Otherwise the adapter would silently miss action 99.
        constants.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        let body = source
            .split("fn execute_live_trigger_action(")
            .nth(1)
            .unwrap()
            .split("\nimpl TriggerRuntime {")
            .next()
            .unwrap();
        let mut handlers = BTreeSet::from([22]); // Force lives in a separate recursion frame.
        for line in body.lines() {
            let Some(rest) = line
                .strip_prefix("            ")
                .filter(|rest| !rest.starts_with(' '))
            else {
                continue;
            };
            let Some((pattern, _)) = rest.split_once(" =>") else {
                continue;
            };
            let mut pattern = pattern.to_owned();
            for (name, value) in &constants {
                pattern = pattern.replace(name, value);
            }
            for arm in pattern.split('|').map(str::trim) {
                if let Some((start, end)) = arm.split_once("..=") {
                    handlers.extend(start.parse::<i32>().unwrap()..=end.parse::<i32>().unwrap());
                } else if let Ok(value) = arm.parse::<i32>() {
                    handlers.insert(value);
                }
            }
        }
        handlers.remove(&0); // Explicit no-op, inventoried separately.
        assert_eq!(handlers, ACTION_HANDLERS.iter().copied().collect());
        for opcode in ACTION_HANDLERS {
            assert_eq!(
                capability(Family::Action, *opcode).sdk,
                SdkCoverage::Complete,
                "未暴露动作 {opcode}"
            );
        }
    }

    #[test]
    fn order_inventory_tracks_the_pinned_dispatcher_and_ares_switch() {
        // The engine's admission list is the integration contract. Do not
        // copy its numbers here: an engine upgrade must expose omissions.
        let source = include_str!("../../../vendor/vera20k/src/sim/team_script_vm/actions.rs");
        let body = source
            .split("const fn action_is_ported(action_id: i32, ares: bool) -> bool {")
            .nth(1)
            .expect("pinned script admission function")
            .split("\n}")
            .next()
            .unwrap();
        let compact: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
        let (baseline, gated) = compact
            .split_once("||")
            .expect("baseline and Ares script admission");
        let parse = |pattern: &str| -> BTreeSet<i32> {
            pattern
                .split('|')
                .map(|opcode| opcode.parse().expect("numeric script admission arm"))
                .collect()
        };
        let baseline = parse(
            baseline
                .strip_prefix("matches!(action_id,")
                .and_then(|pattern| pattern.strip_suffix(')'))
                .expect("baseline script admission pattern"),
        );
        let gated = parse(
            gated
                .strip_prefix("(ares&&matches!(action_id,")
                .and_then(|pattern| pattern.strip_suffix("))"))
                .expect("Ares script admission pattern"),
        );
        assert_eq!(baseline, ORDER_HANDLERS.iter().copied().collect());
        let inventoried_gated = matrix()
            .into_iter()
            .filter(|entry| entry.family == Family::Order && entry.runtime == RuntimeSupport::Ares)
            .map(|entry| entry.opcode)
            .collect();
        assert_eq!(gated, inventoried_gated);
        for opcode in baseline {
            assert_eq!(
                capability(Family::Order, opcode).sdk,
                SdkCoverage::Complete,
                "未暴露命令 {opcode}"
            );
        }
        for opcode in gated {
            assert_eq!(capability(Family::Order, opcode).sdk, SdkCoverage::Complete);
        }
    }
}
