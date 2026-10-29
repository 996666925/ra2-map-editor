//! Real-resource terrain authoring checks, invoked by the window smoke.
use crate::{
    document::{Cell, MapDocument},
    editing::{Editor, Tool},
    map::MapScene,
};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use ra2_map_scene::terrain_tools::{self, TerrainMode};
use std::{collections::BTreeSet, path::Path};

fn click(editor: &mut Editor, scene: &MapScene, at: Cell) -> Result<()> {
    editor.pointer(scene, Some(at), true, false, true, None);
    ensure!(
        !editor.status.starts_with("本笔未修改"),
        "{}",
        editor.status
    );
    Ok(())
}

fn undo_redo(editor: &mut Editor, scene: &MapScene, before: Vec<u8>) -> Result<()> {
    let after = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    ensure!(before != after, "工具没有产生修改");
    editor.history(false, scene);
    ensure!(
        editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
        "整笔撤销不一致"
    );
    editor.history(true, scene);
    ensure!(
        editor.doc.as_ref().unwrap().snapshot().bytes()? == after,
        "整笔重做不一致"
    );
    Ok(())
}

pub(super) fn check(
    ctx: &egui::Context,
    scene: &MapScene,
    output: &Path,
) -> Result<serde_json::Value> {
    let mut editor = Editor::new(ctx.clone(), output.join("p3-state"))?;
    editor.attach(scene);
    let bytes = crate::new_map::NewMapOptions {
        theater: scene.source.header.theater.clone(),
        width: 80,
        height: 80,
        level: 4,
        ..Default::default()
    }
    .bytes()?;
    let (source, _) =
        crate::map::SourceMap::parse("terrain-tools.map".into(), "P3 smoke".into(), bytes)?;
    editor.doc = Some(MapDocument::new(
        source.edit_base.map_err(anyhow::Error::msg)?,
    ));
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Template;
    editor.brush = scene
        .brushes
        .iter()
        .position(|b| {
            b.key.variant == 0
                && b.label.to_ascii_lowercase().contains("road")
                && scene
                    .palette
                    .templates
                    .get(&(b.key.tile_id, 0))
                    .is_some_and(|t| t.cells.iter().flatten().count() > 1)
        })
        .context("缺少完整道路模板")?;
    let brush = scene.brushes[editor.brush].clone();
    let footprint = terrain_tools::footprint(scene, &brush, [60, 60])?;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    let width = scene.palette.templates[&(brush.key.tile_id, 0)].width;
    editor.pointer(scene, Some([60, 60]), true, true, false, None);
    editor.pointer(scene, Some([60 + width * 2, 60]), false, false, true, None);
    ensure!(
        !editor.status.starts_with("本笔未修改"),
        "{}",
        editor.status
    );
    for (at, sub, _) in &footprint {
        let v = editor
            .doc
            .as_ref()
            .unwrap()
            .terrain(*at)
            .context("整模板漏格")?;
        ensure!(v.tile == brush.raw && v.sub == *sub, "整模板子块编号错误");
    }
    undo_redo(&mut editor, scene, before)?;

    let ids = scene.palette.tile_ids.context("没有环境配置")?;
    editor.brush = scene
        .brushes
        .iter()
        .position(|b| {
            i32::from(b.key.tile_id) == ids.rough && b.key.variant == 0 && b.key.sub_tile == 0
        })
        .context("没有粗糙地面")?;
    editor.terrain_mode = TerrainMode::Subtile;
    editor.radius = 3;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [80, 80])?;
    let rough = scene.brushes[editor.brush].raw;
    ensure!(
        editor.doc.as_ref().unwrap().terrain([80, 80]).unwrap().tile == rough,
        "LAT 中心错误"
    );
    ensure!(
        editor.doc.as_ref().unwrap().terrain([79, 79]).unwrap().tile != rough,
        "LAT 边缘未连接"
    );
    undo_redo(&mut editor, scene, before)?;
    editor.terrain_mode = TerrainMode::Fill;
    editor.brush = scene
        .brushes
        .iter()
        .position(|b| {
            i32::from(b.key.tile_id) == ids.clear && b.key.variant == 0 && b.key.sub_tile == 0
        })
        .context("没有 clear")?;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [80, 80])?;
    ensure!(editor.feedback.len() >= 9, "填充未识别 LAT 同类边缘");
    undo_redo(&mut editor, scene, before)?;

    editor.terrain_mode = TerrainMode::Raise;
    editor.radius = 3;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    editor.pointer(scene, Some([90, 90]), true, true, false, None);
    editor.pointer(scene, Some([91, 90]), false, true, false, None);
    editor.pointer(scene, Some([90, 90]), false, true, false, None);
    editor.pointer(scene, Some([92, 90]), false, false, true, None);
    ensure!(
        !editor.status.starts_with("本笔未修改"),
        "{}",
        editor.status
    );
    for x in 89..=93 {
        ensure!(
            editor.doc.as_ref().unwrap().terrain([x, 90]).unwrap().level == 5,
            "升高重复或漏格"
        );
    }
    let plateau: BTreeSet<_> = editor.feedback.clone();
    ensure!(plateau.len() > 15, "没有生成外围坡道");
    undo_redo(&mut editor, scene, before)?;
    editor.terrain_mode = TerrainMode::Flatten;
    editor.flatten_level = 4;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [90, 90])?;
    ensure!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .terrain([90, 90])
            .unwrap()
            .level
            == 4,
        "平整未生效"
    );
    undo_redo(&mut editor, scene, before)?;
    editor.history(false, scene); // Keep a raised plateau in the saved visual fixture.
    editor.terrain_mode = TerrainMode::Lower;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [100, 100])?;
    ensure!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .terrain([100, 100])
            .unwrap()
            .level
            == 3,
        "降低未生效"
    );
    undo_redo(&mut editor, scene, before)?;

    editor.overlay_id = scene
        .catalog
        .overlays
        .iter()
        .position(|id| {
            scene
                .catalog
                .rules
                .section_or_empty(id)
                .read_bool("Wall", false)
        })
        .context("缺少墙体")? as u8;
    editor.tool = Tool::Overlay;
    editor.radius = 1;
    editor.overlay_frame = 0;
    for at in [[75, 70], [76, 70], [77, 70], [76, 71]] {
        click(&mut editor, scene, at)?;
    }
    ensure!(
        editor.doc.as_ref().unwrap().overlay([76, 70]).1 == 14,
        "墙体 T 连接错误"
    );
    editor.tool = Tool::Erase;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [76, 70])?;
    ensure!(
        editor.doc.as_ref().unwrap().overlay([75, 70]).1 == 0,
        "擦除未重连相邻墙"
    );
    undo_redo(&mut editor, scene, before)?;
    editor.history(false, scene);
    // A real multi-height cliff on a low map, with an explicitly chosen base.
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Template;
    editor.template_level = Some(2);
    editor.brush = scene
        .brushes
        .iter()
        .position(|b| {
            b.key.variant == 0
                && scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                    g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
                })
                && scene
                    .palette
                    .templates
                    .get(&(b.key.tile_id, 0))
                    .is_some_and(|t| t.cells.iter().flatten().any(|s| s.height > 0))
        })
        .context("缺少多层悬崖模板")?;
    let cliff = &scene.brushes[editor.brush];
    ensure!(
        scene.palette.previews.contains_key(&(cliff.key.tile_id, 0)),
        "缺少完整悬崖缩略图"
    );
    let planned = terrain_tools::template_placement(
        editor.doc.as_ref().unwrap(),
        scene,
        cliff,
        [85, 85],
        Some(2),
    )?;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    click(&mut editor, scene, [85, 85])?;
    for (at, v) in &planned {
        ensure!(
            editor.doc.as_ref().unwrap().terrain(*at) == Some(*v),
            "悬崖预览与落笔不一致"
        );
    }
    undo_redo(&mut editor, scene, before)?;
    let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    editor.template_level = Some(14);
    editor.pointer(scene, Some([95, 95]), true, false, true, None);
    ensure!(
        editor.status.starts_with("本笔未修改"),
        "悬崖高度超限未拒绝"
    );
    ensure!(
        editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
        "高度超限残留半笔修改"
    );
    let doc = editor.doc.as_ref().unwrap();
    let saved = doc.snapshot().bytes()?;
    let path = output.join("terrain-tools.map");
    std::fs::write(&path, &saved)?;
    let (source, _) = crate::map::SourceMap::parse("reopen.map".into(), "P3".into(), saved)?;
    let reopened = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
    for at in plateau {
        ensure!(
            doc.terrain(at) == reopened.terrain(at),
            "坡面保存重开发生变化"
        );
    }
    ensure!(reopened.overlay([76, 70]).1 == 14, "墙连接保存重开错误");
    for (at, v) in &planned {
        ensure!(reopened.terrain(*at) == Some(*v), "悬崖保存重开不一致");
    }
    let regions = editor.check_regions(scene, output)?;
    let automatic = automatic_plateaus(ctx, scene, output)?;
    let joining = automatic_joining(ctx, scene, output)?;
    Ok(
        serde_json::json!({"passed":true,"road_template_cells":footprint.len(),"template_drag_preserves_previous_stamp":true,"lat":true,"fill_lat_family":true,"height_raise_lower_flatten":true,"height_drag_deduplicated":true,"automatic_ramps":true,"wall_place_delete":true,"cliff_template":{"cells":planned.len(),"explicit_base_height":true,"preview_matches_stamp":true,"overflow_is_atomic":true,"undo_redo":true,"save_reopen":true},"automatic_plateaus":automatic,"automatic_joining":joining,"undo_redo":true,"save_reopen":true,"map":path,"regions_and_objects":regions}),
    )
}

fn automatic_plateaus(
    ctx: &egui::Context,
    scene: &MapScene,
    output: &Path,
) -> Result<serde_json::Value> {
    let mut editor = Editor::new(ctx.clone(), output.join("plateau-state"))?;
    editor.attach(scene);
    let bytes = crate::new_map::NewMapOptions {
        theater: scene.source.header.theater.clone(),
        width: 80,
        height: 80,
        level: 4,
        ..Default::default()
    }
    .bytes()?;
    let (source, _) =
        crate::map::SourceMap::parse("automatic-plateaus.map".into(), "P3".into(), bytes)?;
    editor.doc = Some(MapDocument::new(
        source.edit_base.map_err(anyhow::Error::msg)?,
    ));
    editor.tool = Tool::Terrain;
    let mut expected = std::collections::BTreeMap::new();
    let mut reports = Vec::new();
    for (mode, path) in [
        (TerrainMode::Plateau, vec![[60, 60], [73, 71]]),
        (
            TerrainMode::CliffOutline,
            vec![[88, 60], [106, 60], [106, 66], [95, 66], [95, 78], [88, 78]],
        ),
        (
            TerrainMode::CliffOutline,
            vec![[98, 102], [106, 94], [114, 102], [106, 110]],
        ),
    ] {
        editor.terrain_mode = mode;
        let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
        for (i, &at) in path.iter().enumerate() {
            editor.pointer(scene, Some(at), i == 0, true, false, None);
        }
        let plan = editor.plateau_preview_plan(scene, *path.last().unwrap())?;
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "高台预览提前写入文档"
        );
        editor.pointer(scene, None, false, false, true, None);
        ensure!(
            !editor.status.starts_with("本笔未修改"),
            "{}",
            editor.status
        );
        for (&at, &v) in &plan.edits {
            ensure!(
                editor.doc.as_ref().unwrap().terrain(at) == Some(v),
                "高台预览与落笔不一致"
            );
        }
        undo_redo(&mut editor, scene, before)?;
        expected.extend(plan.edits.iter().map(|(&at, &v)| (at, v)));
        reports.push(serde_json::json!({"mode":mode.name(),"cliff_stamps":plan.cliffs.len(),"preview_matches_release":true}));
    }
    let bytes = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    std::fs::write(output.join("automatic-plateaus.map"), &bytes)?;
    let (source, _) = crate::map::SourceMap::parse("reopen.map".into(), "P3".into(), bytes)?;
    let reopened = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
    for (&at, &v) in &expected {
        ensure!(reopened.terrain(at) == Some(v), "自动高台保存重开不一致");
    }
    Ok(
        serde_json::json!({"passed":true,"shapes":reports,"save_reopen_cells":expected.len(),"undo_redo":true,"save_reopen":true,"map":"automatic-plateaus.map"}),
    )
}

fn automatic_joining(
    ctx: &egui::Context,
    scene: &MapScene,
    output: &Path,
) -> Result<serde_json::Value> {
    let mut editor = Editor::new(ctx.clone(), output.join("joining-state"))?;
    editor.attach(scene);
    let bytes = crate::new_map::NewMapOptions {
        theater: scene.source.header.theater.clone(),
        width: 80,
        height: 80,
        level: 4,
        ..Default::default()
    }
    .bytes()?;
    let (source, _) = crate::map::SourceMap::parse(
        "automatic-joining.map".into(),
        "joining smoke".into(),
        bytes,
    )?;
    editor.doc = Some(MapDocument::new(
        source.edit_base.map_err(anyhow::Error::msg)?,
    ));
    editor.tool = Tool::Terrain;
    let mut reports = Vec::new();
    for (mode, a, b) in [
        (TerrainMode::Water, [60, 60], [72, 72]),
        (TerrainMode::Water, [68, 65], [80, 78]),
        (TerrainMode::LandFill, [64, 64], [66, 66]),
        (TerrainMode::LandFill, [55, 55], [85, 85]),
        (TerrainMode::Road, [55, 70], [85, 70]),
        (TerrainMode::Road, [70, 70], [70, 90]),
        (TerrainMode::Road, [70, 70], [70, 50]),
        (TerrainMode::RoadCut, [66, 70], [74, 70]),
    ] {
        editor.terrain_mode = mode;
        let before = editor.doc.as_ref().unwrap().snapshot().bytes()?;
        editor.pointer(scene, Some(a), true, true, false, None);
        let plan = editor.plateau_preview_plan(scene, b)?;
        ensure!(
            editor.doc.as_ref().unwrap().snapshot().bytes()? == before,
            "joining preview wrote map"
        );
        editor.pointer(scene, Some(b), false, false, true, None);
        ensure!(
            !editor.status.starts_with("本笔未修改"),
            "{}",
            editor.status
        );
        for (&at, &v) in &plan.edits {
            ensure!(
                editor.doc.as_ref().unwrap().terrain(at) == Some(v),
                "joining preview/release mismatch"
            );
        }
        undo_redo(&mut editor, scene, before)?;
        reports.push(serde_json::json!({"mode":mode.name(),"templates":plan.cliffs.len(),"preview_matches_release":true,"undo_redo":true}));
    }
    let bytes = editor.doc.as_ref().unwrap().snapshot().bytes()?;
    std::fs::write(output.join("automatic-joining.map"), &bytes)?;
    let (source, _) =
        crate::map::SourceMap::parse("reopened.map".into(), "joining smoke".into(), bytes.clone())?;
    let reopened = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
    ensure!(
        reopened.snapshot().bytes()? == bytes,
        "joining save/reopen mismatch"
    );
    Ok(serde_json::json!({"cases":reports,"save_reopen":true}))
}
