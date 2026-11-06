//! Cooperative cancellation and conservative scene updates from snapshot changes.
use crate::map::MapScene;
use anyhow::{Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Snapshot, Target},
    lighting::Profile,
    source::SourceMap,
};
use std::sync::Arc;
use vera20k_core::rules::ini_parser::IniFile;

pub fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure!(!cancelled(), "operation cancelled");
    Ok(())
}

/// Known mission fields retain the complete visual scene. Record edits retain
/// resolved terrain/materials. Terrain edits reuse installation materials;
/// unknown INI and mixed terrain/INI edits fall back to a full load.
pub fn update_records(
    old: &MapScene,
    before: &Snapshot,
    after: &Snapshot,
    install: &Install,
    profile: Profile,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<MapScene>> {
    if !Arc::ptr_eq(&before.base, &after.base)
        || old.revision != install.revision
        || old.resolved.is_none()
        || old.visual.profile != Some(profile)
    {
        return Ok(None);
    }
    let Some(targets) = after.changed_targets_since(before) else {
        return Ok(None);
    };
    let targets: Vec<_> = targets.collect();
    let terrain_changed = targets
        .iter()
        .any(|key| matches!(key, Target::Terrain(_) | Target::Overlay(_)));
    if terrain_changed
        && targets.iter().any(|key| {
            !matches!(
                key,
                Target::Terrain(_) | Target::Overlay(_) | Target::Record(_)
            )
        })
    {
        return Ok(None);
    }
    checkpoint(cancelled)?;
    let started = std::time::Instant::now();
    let bytes = after.bytes()?;
    if !terrain_changed && targets.iter().any(|key| !matches!(key, Target::Record(_))) {
        let previous_ini = IniFile::from_bytes(old.source.bytes())?;
        let next_ini = IniFile::from_bytes(&bytes)?;
        let fields = crate::scene_changes::MissionFields::new(
            &previous_ini,
            &next_ini,
            &old.catalog,
            &install.catalog,
        );
        if targets.iter().any(|target| {
            !matches!(target, Target::Record(_))
                && !after
                    .ini_target(target)
                    .is_some_and(|(section, key)| fields.contains(section, key))
        }) {
            return Ok(None);
        }
    }
    let (source, map) = SourceMap::parse(old.source.name.clone(), "编辑快照".into(), bytes)?;
    checkpoint(cancelled)?;
    let mut scene = old.clone();
    scene.source = source;
    if terrain_changed {
        scene.rebuild_terrain(&map, install, profile, Some(old), cancelled)?;
    } else if targets.iter().any(|key| matches!(key, Target::Record(_))) {
        scene.visual = Arc::new(crate::visual::VisualScene::build_cached(
            &scene,
            install,
            old.resolved.as_ref().unwrap(),
            profile,
            Some(crate::visual::SceneReuse::Records(&old.visual)),
            cancelled,
        )?);
    }
    scene.elapsed_ms = started.elapsed().as_millis();
    Ok(Some(scene))
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
