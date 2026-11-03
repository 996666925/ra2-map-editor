//! Audited engine actions beyond the original editor catalog.
use super::*;
use crate::catalog::ResourceKind;

pub(super) fn encode(
    action: &Action,
    doc: &MapDocument,
    catalog: &Catalog,
    teams: &BTreeMap<String, String>,
    mode: MissionMode,
) -> Result<Vec<String>> {
    let row = match action {
        Action::SetSpotlightBehavior { mode } => {
            ensure!(*mode <= 3, "探照灯模式须为 0～3");
            action_row(52, Some(mode.to_string()), None)
        }
        Action::PlayAnimationAt { animation, at } => action_row(
            41,
            Some(
                catalog
                    .resource_index(animation, ResourceKind::Animation)?
                    .to_string(),
            ),
            Some(waypoint(at, catalog)?),
        ),
        Action::ExplosionAt { weapon, at } => action_row(
            42,
            Some(
                catalog
                    .resource_index(weapon, ResourceKind::Weapon)?
                    .to_string(),
            ),
            Some(waypoint(at, catalog)?),
        ),
        Action::CreateParticleSystem { system, at } => action_row(
            88,
            Some(
                catalog
                    .resource_index(system, ResourceKind::ParticleSystem)?
                    .to_string(),
            ),
            Some(waypoint(at, catalog)?),
        ),
        Action::SetAmbient { percent }
        | Action::SetLightingRed { percent }
        | Action::SetLightingGreen { percent }
        | Action::SetLightingBlue { percent } => {
            let opcode = match action {
                Action::SetAmbient { .. } => 73,
                Action::SetLightingRed { .. } => 142,
                Action::SetLightingGreen { .. } => 143,
                _ => 144,
            };
            action_row(opcode, Some(percent.to_string()), None)
        }
        Action::SetAmbientStep { value } | Action::SetAmbientRate { value } => {
            ensure!(
                value.is_finite() && *value >= 0.0,
                "环境亮度步长/变化率须为非负有限数"
            );
            action_row(
                if matches!(action, Action::SetAmbientStep { .. }) {
                    71
                } else {
                    72
                },
                Some((value.to_bits() as i32).to_string()),
                None,
            )
        }
        Action::CombatFlash { at, strength } => {
            ensure!(*strength <= 2, "战斗闪光强度须为 0～2");
            waypoint(at, catalog)?;
            action_row(64 + i32::from(*strength), Some(at.id.to_string()), None)
        }
        Action::DamageBridgeAt { at } => {
            waypoint(at, catalog)?;
            action_row(63, Some(at.id.to_string()), None)
        }
        Action::MeteorShower { at, intensity } => {
            ensure!((0..=3).contains(intensity), "陨石雨强度须为 0～3");
            action_row(
                58,
                Some(intensity.to_string()),
                Some(waypoint(at, catalog)?),
            )
        }
        Action::ReduceTiberiumAt { at } => {
            waypoint(at, catalog)?;
            action_row(59, Some(at.id.to_string()), None)
        }
        Action::IonBlastAt { at }
        | Action::NukeAt { at }
        | Action::ChemicalStrikeAt { at }
        | Action::MarkParticlesDone { at } => {
            let opcode = match action {
                Action::IonBlastAt { .. } => 94,
                Action::NukeAt { .. } => 95,
                Action::ChemicalStrikeAt { .. } => 96,
                _ => 89,
            };
            action_row(opcode, None, Some(waypoint(at, catalog)?))
        }
        Action::SetVeinGrowth { enabled }
        | Action::SetTiberiumGrowth { enabled }
        | Action::SetIceGrowth { enabled }
        | Action::SetAutoBaseBuilding { enabled } => {
            let opcode = match action {
                Action::SetVeinGrowth { .. } => 85,
                Action::SetTiberiumGrowth { .. } => 86,
                Action::SetIceGrowth { .. } => 87,
                _ => 30,
            };
            action_row(opcode, Some(u8::from(*enabled).to_string()), None)
        }
        Action::ToggleTrainCrate => action_row(97, None, None),
        Action::ClearSmudges => action_row(118, None, None),
        Action::FireSale { house }
        | Action::EnableAutocreate { house }
        | Action::MakeAlly { house }
        | Action::MakeEnemy { house }
        | Action::Cheer { house } => {
            let opcode = match action {
                Action::FireSale { .. } => 9,
                Action::EnableAutocreate { .. } => 13,
                Action::MakeAlly { .. } => 37,
                Action::MakeEnemy { .. } => 38,
                _ => 113,
            };
            action_row(
                opcode,
                Some(rules::action_house(house, mode, catalog)?),
                None,
            )
        }
        Action::SetAiTriggerRatio { value }
        | Action::SetAircraftRatio { value }
        | Action::SetInfantryRatio { value }
        | Action::SetVehicleRatio { value }
        | Action::SetSuperTargetKind { value } => {
            let opcode = match action {
                Action::SetAiTriggerRatio { .. } => 76,
                Action::SetAircraftRatio { .. } => 77,
                Action::SetInfantryRatio { .. } => 78,
                Action::SetVehicleRatio { .. } => 79,
                _ => 35,
            };
            action_row(opcode, Some(value.to_string()), None)
        }
        Action::SetTechLevel { r#type, level } => action_row(
            106,
            Some(inline_type(catalog, r#type, ResourceKind::Techno)?),
            Some(level.to_string()),
        ),
        Action::EndScenario => {
            ensure!(
                mode == MissionMode::SinglePlayer,
                "结束场景仅支持单人结果流程"
            );
            action_row(69, None, None)
        }
        Action::DeleteRule { .. } | Action::DeleteTag { .. } => {
            unreachable!("definition references encoded by caller")
        }
        Action::PlayMovie { movie } | Action::PlayRadarMovie { movie, .. } => {
            ensure!(mode == MissionMode::SinglePlayer, "影片动作仅支持单人任务");
            let opcode = match action {
                Action::PlayMovie { .. } => 10,
                Action::PlayRadarMovie { pause: true, .. } => 117,
                _ => 100,
            };
            action_row(
                opcode,
                Some(
                    catalog
                        .resource_index(movie, ResourceKind::Movie)?
                        .to_string(),
                ),
                None,
            )
        }
        Action::PlaySoundAt { sound, at } => action_row(
            99,
            Some(catalog.resource_id(sound, ResourceKind::Sound)?.into()),
            Some(waypoint(at, catalog)?),
        ),
        Action::PlaySoundAtRandomWaypoint { sound } => {
            ensure!(
                !catalog.waypoints.is_empty(),
                "随机位置声音需要至少一个地图航点"
            );
            action_row(
                98,
                Some(catalog.resource_id(sound, ResourceKind::Sound)?.into()),
                None,
            )
        }
        Action::StopSoundAt { at } => action_row(116, None, Some(waypoint(at, catalog)?)),
        Action::LockInput => action_row(46, None, None),
        Action::UnlockInput => action_row(47, None, None),
        Action::PauseScene { duration } => {
            ensure!(
                mode == MissionMode::SinglePlayer && (1..=35_791_394).contains(duration),
                "定时暂停仅支持单人，duration 须为 1～35791394；0 无脚本恢复入口"
            );
            action_row(110, Some(duration.to_string()), None)
        }
        Action::ChronoScreen { duration } => {
            ensure!(
                *duration <= i32::MAX as u32,
                "超时空画面时间超出原生计时范围"
            );
            action_row(127, Some(duration.to_string()), None)
        }
        Action::TalkBubble { team, bubble } => {
            ensure!(*bubble <= 3, "对话气泡须为 0～3");
            action_row(
                105,
                Some(named(teams, team, "team")?),
                Some(bubble.to_string()),
            )
        }
        Action::ClearTalkBubble => action_row(105, Some("-1".into()), Some("0".into())),
        Action::DestroyAttached => action_row(32, None, None),
        Action::SellAttachedBuildings => action_row(60, None, None),
        Action::SetAttachedBuildingsOnline { online } => {
            action_row(if *online { 62 } else { 61 }, None, None)
        }
        Action::MakeAttachedInfantryPsycho => action_row(91, None, None),
        Action::WakeAttached => action_row(81, None, None),
        Action::WakeOtherHouseSleepers => action_row(82, None, None),
        Action::WakeHouseHarmless => action_row(83, None, None),
        Action::WakeGroup { group } => action_row(84, Some(group.to_string()), None),
        Action::CreateBuilding { building, at } => action_row(
            125,
            Some(inline_type(catalog, building, ResourceKind::Building)?),
            Some(waypoint(at, catalog)?),
        ),
        Action::ReleaseGarrison => action_row(111, None, None),
        Action::DestroyHouseObjects { house, subset } => {
            ensure!(*subset <= 3, "House 对象集合须为 0～3");
            action_row(
                119 + i32::from(*subset),
                Some(rules::action_house(house, mode, catalog)?),
                None,
            )
        }
        Action::ControlHouseBuildings { house, restore } => action_row(
            if *restore { 124 } else { 123 },
            Some(rules::action_house(house, mode, catalog)?),
            None,
        ),
        Action::RestoreInitialObjects {
            house,
            buildings_only,
        } => action_row(
            if *buildings_only { 130 } else { 126 },
            Some(rules::action_house(house, mode, catalog)?),
            None,
        ),
        Action::ChangeAllHouseObjectsOwner { house } => {
            action_row(36, Some(rules::action_house(house, mode, catalog)?), None)
        }
        Action::FlashHouseBuildings { building, frames } => action_row(
            131,
            Some(inline_type(catalog, building, ResourceKind::Building)?),
            Some(frames.to_string()),
        ),
        Action::GrantSuperWeapon { weapon, .. }
        | Action::SetSuperCharge { weapon, .. }
        | Action::SetSuperRecharge { weapon, .. }
        | Action::ResetSuperRecharge { weapon }
        | Action::ResetSuperWeapon { weapon } => {
            let (opcode, tail) = match action {
                Action::GrantSuperWeapon { one_shot, .. } => {
                    (if *one_shot { 33 } else { 34 }, None)
                }
                Action::SetSuperCharge { percent, .. } => {
                    ensure!(*percent <= 100, "超武充能百分比须为 0～100");
                    (129, Some(percent.to_string()))
                }
                Action::SetSuperRecharge { frames, .. } => {
                    ensure!(*frames <= i32::MAX as u32, "超武充能时间超出逻辑帧范围");
                    (132, Some(frames.to_string()))
                }
                Action::ResetSuperRecharge { .. } => (133, None),
                _ => (134, None),
            };
            action_row(
                opcode,
                Some(
                    catalog
                        .resource_index(weapon, ResourceKind::SuperWeapon)?
                        .to_string(),
                ),
                tail,
            )
        }
        Action::SetSuperTarget { at }
        | Action::SetSuperDefense { at }
        | Action::LightningAt { at }
        | Action::LightningStormAt { at }
        | Action::ChronoshiftHouse { at }
        | Action::IronCurtainAt { at } => {
            let opcode = match action {
                Action::SetSuperTarget { .. } => 135,
                Action::SetSuperDefense { .. } => 140,
                Action::LightningAt { .. } => 90,
                Action::LightningStormAt { .. } => 102,
                Action::ChronoshiftHouse { .. } => 128,
                _ => 109,
            };
            action_row(opcode, None, Some(waypoint(at, catalog)?))
        }
        Action::ClearSuperTarget => action_row(136, None, None),
        Action::ClearSuperDefense => action_row(141, None, None),
        Action::StartLightningStorm { frames } => {
            ensure!(
                *frames > 0 && *frames <= i32::MAX as u32,
                "闪电风暴持续时间须为正逻辑帧"
            );
            action_row(44, Some(frames.to_string()), None)
        }
        Action::StopLightningStorm => action_row(45, None, None),
        Action::CenterCamera { at, velocity } => {
            ensure!((0..=4).contains(velocity), "镜头速度须为 0～4 原生档位");
            action_row(48, Some(velocity.to_string()), Some(waypoint(at, catalog)?))
        }
        Action::JumpCamera { at } => action_row(112, None, Some(waypoint(at, catalog)?)),
        Action::HomeCamera => action_row(145, None, None),
        Action::RevealAt { at } | Action::RevealZone { at } | Action::ConcealAt { at } => {
            waypoint(at, catalog)?;
            let opcode = match action {
                Action::RevealAt { .. } => 17,
                Action::RevealZone { .. } => 18,
                _ => 101,
            };
            action_row(opcode, Some(at.id.to_string()), None)
        }
        Action::GrowShroud => action_row(31, None, None),
        Action::ReshroudMap => action_row(51, None, None),
        Action::VisibleMapArea {
            x,
            y,
            width,
            height,
        } => {
            let bounds: Vec<i32> = doc
                .ini_text("Map", "Size")
                .split(',')
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            ensure!(
                bounds.len() == 4
                    && *width > 0
                    && *height > 0
                    && i32::from(*x) >= bounds[0]
                    && i32::from(*y) >= bounds[1]
                    && i32::from(*x) + i32::from(*width) <= bounds[0] + bounds[2]
                    && i32::from(*y) + i32::from(*height) <= bounds[1] + bounds[3],
                "显示范围须为 Map/Size 内的非空矩形"
            );
            let mut row = action_row(40, None, None);
            for (column, value) in [*x, *y, *width, *height].into_iter().enumerate() {
                row[column + 3] = value.to_string();
            }
            row
        }
        Action::RadarEvent { event, at } => {
            ensure!(*event <= 16, "雷达事件类型须为 0～16");
            action_row(55, Some(event.to_string()), Some(waypoint(at, catalog)?))
        }
        Action::SidebarTab { tab } => {
            ensure!(*tab <= 3, "侧栏页须为 0～3");
            action_row(114, Some(tab.to_string()), None)
        }
        Action::FlashCameo { r#type, frames } => {
            let id = catalog.resource_id(r#type, ResourceKind::Techno)?;
            ensure!(id.len() <= 24, "侧栏科技类型 ID 超出 24 字节");
            action_row(115, Some(id.into()), Some(frames.to_string()))
        }
        Action::RadarOutage { frames } => {
            ensure!(*frames >= 0, "雷达中断时间须为非负逻辑帧");
            action_row(139, Some(frames.to_string()), None)
        }
        _ => anyhow::bail!("该动作尚未接入 SDK 编码"),
    };
    Ok(row)
}

fn inline_type(catalog: &Catalog, reference: &Reference, kind: ResourceKind) -> Result<String> {
    let id = catalog.resource_id(reference, kind)?;
    ensure!(id.len() <= 24, "内联类型 ID 超出原生 24 字节");
    Ok(id.into())
}
