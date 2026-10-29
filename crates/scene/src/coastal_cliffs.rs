//! Match compatible front cliff pieces to water without changing their geometry.
use crate::{
    cliff_authoring,
    cliff_tools::{CliffStamp, PlateauPlan},
    map::MapScene,
    shore_tools::{id, offset, protect, wet},
    terrain_tools::value,
};
use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Cell, MapDocument};
use std::collections::{BTreeMap, BTreeSet};

/// WaterCliffs only shares the first 22 orientations with CliffSet. Later
/// entries are caves/shore joins, not the land set's back faces.
pub(crate) fn piece(scene: &MapScene, tile: u16) -> Option<(u16, bool)> {
    let ids = scene.palette.tile_ids?;
    if let Some(b) = ids.cliffs.cliff_set
        && (b..b + 40).contains(&tile)
    {
        return Some((tile - b, false));
    }
    if let Some(b) = ids.cliffs.water_cliffs
        && (b..b + 22).contains(&tile)
        && supports(scene, tile - b)
    {
        return Some((tile - b, true));
    }
    None
}
/// Some theaters declare WaterCliffs but point it at unrelated templates.
pub fn supports(scene: &MapScene, index: u16) -> bool {
    if index >= 22 {
        return false;
    }
    let Some(ids) = scene.palette.tile_ids else {
        return false;
    };
    let (Some(land), Some(water)) = (ids.cliffs.cliff_set, ids.cliffs.water_cliffs) else {
        return false;
    };
    let (Some(a), Some(b)) = (
        scene.palette.templates.get(&(land + index, 0)),
        scene.palette.templates.get(&(water + index, 0)),
    ) else {
        return false;
    };
    a.width == b.width
        && a.height == b.height
        && a.cells.len() == b.cells.len()
        && a.cells.iter().zip(&b.cells).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => a.height == b.height && a.slope == b.slope,
            (None, None) => true,
            _ => false,
        })
}
pub(crate) fn adapt(doc: &MapDocument, scene: &MapScene, p: &mut PlateauPlan) -> Result<()> {
    adapt_inner(doc, scene, p, false)
}
fn adapt_inner(
    doc: &MapDocument,
    scene: &MapScene,
    p: &mut PlateauPlan,
    strict: bool,
) -> Result<()> {
    let ids = scene.palette.tile_ids.context("缺少地形配置")?;
    let land = ids.cliffs.cliff_set.context("缺少陆地悬崖")?;
    for s in &mut p.cliffs {
        let Some((index, _)) = piece(scene, s.tile) else {
            continue;
        };
        if index >= 22 {
            continue;
        }
        let original = &scene.palette.templates[&(s.tile, 0)];
        let mut water = false;
        for (sub, meta) in original.cells.iter().enumerate() {
            if meta.is_none_or(|m| m.height != 0) {
                continue;
            }
            let at = offset(
                s.origin,
                sub as i32 % i32::from(original.width),
                sub as i32 / i32::from(original.width),
            )
            .context("悬崖越界")?;
            for n in std::iter::once(at).chain(crate::terrain_tools::neighbors(at)) {
                let v = p
                    .edits
                    .get(&n)
                    .copied()
                    .unwrap_or_else(|| value(doc, scene, n));
                if doc.check_cell(n).is_ok() && v.level == s.base && wet(scene, v) {
                    water = true;
                }
            }
        }
        if water && !supports(scene, index) {
            ensure!(
                !strict,
                "当前环境缺少与第 {} 块悬崖兼容的水悬崖模板",
                index + 1
            );
            continue;
        }
        let target = if water {
            ids.cliffs
                .water_cliffs
                .context("临水悬崖缺少 WaterCliffs 资源")?
                + index
        } else {
            land + index
        };
        if target == s.tile {
            continue;
        }
        let t = scene
            .palette
            .templates
            .get(&(target, 0))
            .context("缺少临水悬崖对应方向模板")?;
        ensure!(
            t.width == original.width
                && t.height == original.height
                && t.cells.len() == original.cells.len()
                && t.cells
                    .iter()
                    .zip(&original.cells)
                    .all(|(a, b)| match (a, b) {
                        (Some(a), Some(b)) => a.height == b.height && a.slope == b.slope,
                        (None, None) => true,
                        _ => false,
                    }),
            "水悬崖与陆地悬崖几何不同，无法完整替换"
        );
        p.edits.extend(cliff_authoring::stamp(
            doc, scene, target, s.origin, s.base,
        )?);
        s.tile = target;
    }
    Ok(())
}
pub(crate) fn gather(
    doc: &MapDocument,
    scene: &MapScene,
    selection: &BTreeSet<Cell>,
) -> Result<Vec<CliffStamp>> {
    let mut stamps = BTreeSet::new();
    for &at in selection {
        doc.check_cell(at)?;
        let v = value(doc, scene, at);
        let Some(tile) = id(scene, v).filter(|&t| piece(scene, t).is_some_and(|(i, _)| i < 22))
        else {
            continue;
        };
        let t = scene
            .palette
            .templates
            .get(&(tile, 0))
            .context("悬崖模板缺失")?;
        let origin = offset(
            at,
            -(i32::from(v.sub) % i32::from(t.width)),
            -(i32::from(v.sub) / i32::from(t.width)),
        )
        .context("悬崖越界")?;
        let base = v
            .level
            .checked_sub(
                t.cells
                    .get(usize::from(v.sub))
                    .and_then(|s| *s)
                    .context("悬崖子块无效")?
                    .height,
            )
            .context("悬崖高度无效")?;
        stamps.insert((tile, origin, base));
    }
    stamps
        .into_iter()
        .map(|(tile, origin, base)| {
            let cells = cliff_authoring::stamp(doc, scene, tile, origin, base)?;
            ensure!(
                cells.iter().all(|(&at, &v)| value(doc, scene, at) == v),
                "已有悬崖模板不完整，请先修复该模板"
            );
            Ok(CliffStamp { tile, origin, base })
        })
        .collect()
}
pub(crate) fn barrier(scene: &MapScene, v: ra2_map_document::document::TerrainValue) -> bool {
    id(scene, v).is_some_and(|t| {
        piece(scene, t).is_some()
            && scene
                .palette
                .templates
                .get(&(t, 0))
                .and_then(|t| t.cells.get(usize::from(v.sub)))
                .and_then(|s| *s)
                .is_some_and(|s| s.height == 0)
    })
}
pub fn plan(doc: &MapDocument, scene: &MapScene, selection: BTreeSet<Cell>) -> Result<PlateauPlan> {
    let stamps = gather(doc, scene, &selection)?;
    ensure!(
        !stamps.is_empty(),
        "请选择标准正面悬崖；水悬崖不包含陆地背面方向"
    );
    let mut p = PlateauPlan {
        edits: BTreeMap::new(),
        overlays: BTreeMap::new(),
        cliffs: Vec::new(),
        interior: BTreeSet::new(),
        base: 0,
    };
    p.cliffs = stamps;
    adapt_inner(doc, scene, &mut p, true)?;
    p.edits.retain(|&at, v| *v != value(doc, scene, at));
    protect(doc, scene, &p.edits)?;
    Ok(p)
}
