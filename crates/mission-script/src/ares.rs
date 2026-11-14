//! Ares SDK lowering. The pinned engine owns execution and INI semantics.
//! Trigger operands follow trigger_runtime_ares.rs; no runtime effects are emulated here.
use super::*;
use crate::catalog::ResourceKind;

pub fn enabled() -> bool {
    vera20k_core::rules::ARES_BUILD
}
fn require() -> Result<()> {
    ensure!(
        enabled(),
        "此任务需要使用 --features ares 构建的编辑器及启用 Ares 的 VERA20k 客户端"
    );
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AresEvent {
    UnderEmp {
        object: Reference,
    },
    UnderEmpByHouse {
        object: Reference,
        house: Reference,
    },
    EmpEnded {
        object: Reference,
    },
    EmpEndedByHouse {
        object: Reference,
        house: Reference,
    },
    DriverKilled {
        object: Reference,
    },
    DriverKilledByHouse {
        object: Reference,
        house: Reference,
    },
    VehicleTaken {
        object: Reference,
    },
    VehicleTakenByHouse {
        object: Reference,
        house: Reference,
    },
    Abducted {
        object: Reference,
    },
    AbductedByHouse {
        object: Reference,
        house: Reference,
    },
    Abducts {
        object: Reference,
    },
    AbductsFromHouse {
        object: Reference,
        house: Reference,
    },
    SuperActivated {
        superweapon: Reference,
    },
    SuperDeactivated {
        superweapon: Reference,
    },
    SuperFiredNear {
        superweapon: Reference,
        waypoint: Waypoint,
    },
    ReverseEngineered {
        r#type: Reference,
    },
    ObjectReverseEngineered {
        object: Reference,
    },
    ObjectReverseEngineeredType {
        object: Reference,
        r#type: Reference,
    },
    OwnsAtLeast {
        r#type: Reference,
        count: u32,
    },
    OwnsAtMost {
        r#type: Reference,
        count: u32,
    },
    AttackedOrDestroyed {
        object: Reference,
    },
    AttackedOrDestroyedByHouse {
        object: Reference,
        house: Reference,
    },
    DestroyedByHouse {
        house: Reference,
    },
    ExistsAtMost {
        r#type: Reference,
        count: u32,
    },
    NoKeepAlive {
        house: ActionHouse,
    },
    NoKeepAliveBuildings {
        house: ActionHouse,
    },
}
impl AresEvent {
    pub(super) fn object(&self) -> Option<&Reference> {
        match self {
            Self::UnderEmp { object }
            | Self::UnderEmpByHouse { object, .. }
            | Self::EmpEnded { object }
            | Self::EmpEndedByHouse { object, .. }
            | Self::DriverKilled { object }
            | Self::DriverKilledByHouse { object, .. }
            | Self::VehicleTaken { object }
            | Self::VehicleTakenByHouse { object, .. }
            | Self::Abducted { object }
            | Self::AbductedByHouse { object, .. }
            | Self::Abducts { object }
            | Self::AbductsFromHouse { object, .. }
            | Self::ObjectReverseEngineered { object }
            | Self::ObjectReverseEngineeredType { object, .. }
            | Self::AttackedOrDestroyed { object }
            | Self::AttackedOrDestroyedByHouse { object, .. } => Some(object),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AresAction {
    Firestorm { enabled: bool },
    AddAuxPower { house: ActionHouse, amount: i32 },
    KillDrivers { house: ActionHouse },
    SetEvaVoice { index: i32 },
    SetGroup { group: i32 },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AresOrder {
    EnterOccupiableVehicle,
    ConvertScript,
}

pub(super) fn event(event: &AresEvent, context: &mut rules::Events<'_>) -> Result<Vec<String>> {
    require()?;
    if let Some(object) = event.object() {
        context.bind_object(object, context.tag)?;
    }
    let mut type_name = None;
    let mut value = "0".to_owned();
    let house_index = |house: &Reference| -> Result<String> {
        ensure!(
            context.mode == MissionMode::SinglePlayer,
            "来源 House 数组索引的 Ares 事件仅支持单人任务"
        );
        let id = named(context.house_ids, house, "house")?;
        Ok(context
            .house_indices
            .get(&id)
            .context("Ares 事件来源 House 未在地图注册")?
            .to_string())
    };
    let opcode = match event {
        AresEvent::UnderEmp { .. } => 62,
        AresEvent::UnderEmpByHouse { house, .. } => {
            value = house_index(house)?;
            63
        }
        AresEvent::EmpEnded { .. } => 64,
        AresEvent::EmpEndedByHouse { house, .. } => {
            value = house_index(house)?;
            65
        }
        AresEvent::DriverKilled { .. } => 67,
        AresEvent::DriverKilledByHouse { house, .. } => {
            value = house_index(house)?;
            68
        }
        AresEvent::VehicleTaken { .. } => 69,
        AresEvent::VehicleTakenByHouse { house, .. } => {
            value = house_index(house)?;
            70
        }
        AresEvent::Abducted { .. } => 71,
        AresEvent::AbductedByHouse { house, .. } => {
            value = house_index(house)?;
            72
        }
        AresEvent::Abducts { .. } => 73,
        AresEvent::AbductsFromHouse { house, .. } => {
            value = house_index(house)?;
            74
        }
        AresEvent::SuperActivated { superweapon } | AresEvent::SuperDeactivated { superweapon } => {
            value = context
                .catalog
                .resource_index(superweapon, ResourceKind::SuperWeapon)?
                .to_string();
            if matches!(event, AresEvent::SuperActivated { .. }) {
                75
            } else {
                76
            }
        }
        AresEvent::SuperFiredNear {
            superweapon,
            waypoint: at,
        } => {
            waypoint(at, context.catalog)?;
            value = at.id.to_string();
            type_name = Some(
                context
                    .catalog
                    .resource_id(superweapon, ResourceKind::SuperWeapon)?
                    .to_owned(),
            );
            77
        }
        AresEvent::ReverseEngineered { r#type }
        | AresEvent::ObjectReverseEngineeredType { r#type, .. } => {
            type_name = Some(
                context
                    .catalog
                    .resource_id(r#type, ResourceKind::Techno)?
                    .to_owned(),
            );
            if matches!(event, AresEvent::ReverseEngineered { .. }) {
                78
            } else {
                80
            }
        }
        AresEvent::ObjectReverseEngineered { .. } => 79,
        AresEvent::OwnsAtLeast { r#type, count }
        | AresEvent::OwnsAtMost { r#type, count }
        | AresEvent::ExistsAtMost { r#type, count } => {
            ensure!(
                *count < i32::MAX as u32,
                "Ares 数量须为 0～2147483646，避免至多比较溢出"
            );
            value = count.to_string();
            type_name = Some(
                context
                    .catalog
                    .resource_id(r#type, ResourceKind::Techno)?
                    .to_owned(),
            );
            match event {
                AresEvent::OwnsAtLeast { .. } => 81,
                AresEvent::OwnsAtMost { .. } => 82,
                _ => 86,
            }
        }
        AresEvent::AttackedOrDestroyed { .. } => 83,
        AresEvent::AttackedOrDestroyedByHouse { house, .. } => {
            value = house_index(house)?;
            84
        }
        AresEvent::DestroyedByHouse { house } => {
            value = house_index(house)?;
            85
        }
        AresEvent::NoKeepAlive { house } | AresEvent::NoKeepAliveBuildings { house } => {
            ensure!(
                !matches!(house, ActionHouse::RetainedHouse { .. }),
                "Ares 存续事件不接受保留 House 8997"
            );
            value = rules::action_house(house, context.mode, context.catalog)?;
            if matches!(event, AresEvent::NoKeepAlive { .. }) {
                87
            } else {
                88
            }
        }
    };
    let mut row = mission::schema::row(true, opcode).context("Ares 事件未启用")?;
    row[2] = value;
    if let Some(name) = type_name {
        ensure!(
            name.is_ascii() && name.len() <= 24 && !name.contains([',', ';', '\r', '\n']),
            "Ares 类型 ID 须为不含分隔符的 1～24 字节 ASCII"
        );
        row[3] = name;
    }
    Ok(row)
}

pub(super) fn action(
    action: &AresAction,
    catalog: &Catalog,
    mode: MissionMode,
) -> Result<Vec<String>> {
    require()?;
    Ok(match action {
        AresAction::Firestorm { enabled } => action_row(if *enabled { 92 } else { 93 }, None, None),
        AresAction::AddAuxPower { house, amount } => action_row(
            146,
            Some(rules::action_house(house, mode, catalog)?),
            Some(amount.to_string()),
        ),
        AresAction::KillDrivers { house } => {
            action_row(147, Some(rules::action_house(house, mode, catalog)?), None)
        }
        AresAction::SetEvaVoice { index } => {
            ensure!(*index >= -1, "EVA 语音序号须为 -1 或非负整数");
            action_row(148, Some(index.to_string()), None)
        }
        AresAction::SetGroup { group } => action_row(149, Some(group.to_string()), None),
    })
}
pub(super) fn order(order: &AresOrder) -> Result<(i32, String)> {
    require()?;
    Ok((
        match order {
            AresOrder::EnterOccupiableVehicle => 67,
            AresOrder::ConvertScript => 68,
        },
        "0".into(),
    ))
}
