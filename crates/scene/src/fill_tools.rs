//! Bounded ground painting: the preview and commit consume the same plan.
use crate::{
    map::MapScene,
    terrain_tools::{self, value},
};
use anyhow::{Result, ensure};
use ra2_map_assets::{catalog::TerrainBrush, terrain_library::TerrainCategory};
use ra2_map_document::document::{Cell, MapDocument, Target, TerrainValue, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Fill materials are flat, complete ground cells. LAT pieces are generated,
/// rather than offered as repeating materials; slopes and structures have tools.
pub fn material(scene: &MapScene, brush: &TerrainBrush) -> Result<()> {
    let key = brush.key;
    ensure!(
        scene
            .palette
            .templates
            .get(&(key.tile_id, key.variant))
            .is_some_and(|t| t.width == 1 && t.height == 1),
        "填充请选择 1×1 平坦地表"
    );
    ensure!(
        key.variant == 0
            && key.sub_tile == 0
            && ground(scene, brush.key.tile_id, 0)
            && scene.palette.images.contains_key(&key),
        "填充请选择平坦地表；坡道、悬崖和水岸请使用专用工具"
    );
    if let Some(ids) = scene.palette.tile_ids {
        ensure!(
            ![ids.rough_lat, ids.sand_lat, ids.green_lat, ids.pave_lat]
                .into_iter()
                .any(|base| base >= 0 && (base..base + 16).contains(&i32::from(key.tile_id))),
            "请选择基础地表，过渡边缘由自动 LAT 生成"
        );
    }
    Ok(())
}

fn ground(scene: &MapScene, tile: u16, sub: u8) -> bool {
    sub == 0
        && scene
            .palette
            .groups
            .get(&tile)
            .is_some_and(|g| g.category == TerrainCategory::Ground)
        && scene.palette.templates.get(&(tile, 0)).is_some_and(|t| {
            t.width == 1
                && t.height == 1
                && t.cells
                    .first()
                    .and_then(|c| *c)
                    .is_some_and(|c| c.height == 0 && c.slope == 0)
        })
}

fn paintable(scene: &MapScene, v: TerrainValue) -> bool {
    let tile = scene.palette.raw_ids.get(&v.tile).copied().or_else(|| {
        // Authored empty cells render using ClearTile, just like omitted records.
        matches!(v.tile, -1 | 65535)
            .then(|| {
                scene
                    .palette
                    .tile_ids
                    .and_then(|ids| u16::try_from(ids.clear).ok())
            })
            .flatten()
    });
    tile.is_some_and(|tile| ground(scene, tile, v.sub))
}

pub struct FillPlan {
    pub edits: BTreeMap<Cell, TerrainValue>,
    pub cells: BTreeSet<Cell>,
    pub skipped: BTreeSet<Cell>,
}
impl FillPlan {
    pub fn apply(&self, doc: &mut MapDocument) -> Result<Vec<Cell>> {
        for &at in self.edits.keys() {
            doc.check_cell(at)?;
        }
        let mut changed = Vec::new();
        for (&at, &v) in &self.edits {
            if doc.terrain(at) != Some(v) {
                doc.set(Target::Terrain(at), Some(Value::Terrain(v)))?;
                changed.push(at);
            }
        }
        Ok(changed)
    }
}

/// Clip to Size, preserve each cell's height/ice and retain structural terrain.
/// LAT may repair adjacent ground, which is included in the returned preview.
pub fn plan(
    doc: &MapDocument,
    scene: &MapScene,
    brush: &TerrainBrush,
    selected: BTreeSet<Cell>,
    auto_lat: bool,
) -> Result<FillPlan> {
    material(scene, brush)?;
    let mut result = FillPlan {
        edits: BTreeMap::new(),
        cells: BTreeSet::new(),
        skipped: BTreeSet::new(),
    };
    let mut scratch = doc.fork();
    for at in selected
        .into_iter()
        .filter(|&at| doc.check_cell(at).is_ok())
    {
        let old = value(doc, scene, at);
        if !paintable(scene, old) {
            result.skipped.insert(at);
            continue;
        }
        let v = TerrainValue {
            tile: brush.raw,
            sub: 0,
            ..old
        };
        result.cells.insert(at);
        scratch.set(Target::Terrain(at), Some(Value::Terrain(v)))?;
    }
    if auto_lat && !result.cells.is_empty() {
        for at in terrain_tools::recalculate(&mut scratch, scene, &result.cells)? {
            if paintable(scene, value(doc, scene, at)) {
                result.edits.insert(at, value(&scratch, scene, at));
            }
        }
    }
    for &at in &result.cells {
        result.edits.insert(at, value(&scratch, scene, at));
    }
    Ok(result)
}

#[cfg(test)]
#[path = "fill_tools_tests.rs"]
mod tests;
