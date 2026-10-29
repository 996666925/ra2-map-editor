//! Cooperative cancellation and safe record-only scene updates.
use crate::map::MapScene;
use anyhow::{Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Snapshot, Target},
    lighting::Profile,
    source::SourceMap,
};
use std::sync::Arc;

pub fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<()> {
    ensure!(!cancelled(), "operation cancelled");
    Ok(())
}

/// Terrain/INI/overlay changes use native resolution. Record-only edits retain
/// the authoritative resolved terrain and decoded images/materials.
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
    {
        return Ok(None);
    }
    let mut targets = std::collections::BTreeSet::new();
    targets.extend(before.changes.keys());
    targets.extend(after.changes.keys());
    if targets.iter().any(|key| {
        before.changes.get(key) != after.changes.get(key) && !matches!(key, Target::Record(_))
    }) {
        return Ok(None);
    }
    checkpoint(cancelled)?;
    let started = std::time::Instant::now();
    let (source, _) = SourceMap::parse(old.source.name.clone(), "编辑快照".into(), after.bytes()?)?;
    checkpoint(cancelled)?;
    let mut scene = old.clone();
    scene.source = source;
    scene.visual = Arc::new(crate::visual::VisualScene::build_cached(
        &scene,
        install,
        old.resolved.as_ref().unwrap(),
        profile,
        Some(&old.visual),
        cancelled,
    )?);
    scene.elapsed_ms = started.elapsed().as_millis();
    Ok(Some(scene))
}
