//! Geometry/history/publication and native lighting using an actual installation.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    base_nodes::{self, Node},
    document::{Baseline, MapDocument, Target, TerrainValue, Value},
    lighting::{Draft, Profile},
    map_settings::{self, Footprints, Options},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::{
    editor_io,
    publication::{Publisher, fit},
};
use ra2_map_scene::map::load_scene_bytes_with_profile;
use std::{path::PathBuf, time::Instant};
use vera20k_core::{
    map::{lighting, map_file},
    rules::ini_parser::IniFile,
};

fn put(doc: &mut MapDocument, section: &str, key: &str, value: impl AsRef<[u8]>) -> Result<()> {
    doc.set_ini(section, key, Some(value.as_ref().to_vec()))
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p3-map-settings".into()),
    );
    std::fs::create_dir_all(&output)?;
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    mission::create(&mut doc, Kind::House, "Americans", "Americans", None)?;
    for (section, kind, at) in [
        ("Structures", "GAPOWR", [24, 40]),
        ("Units", "MTNK", [30, 40]),
        ("Infantry", "E1", [32, 40]),
        ("Aircraft", "ORCA", [34, 40]),
        ("Terrain", "TREE01", [36, 40]),
        ("Smudge", "CR1", [38, 40]),
    ] {
        doc.add_record(section, at, "Americans", kind)?;
    }
    doc.waypoint(20, [28, 42])?;
    doc.set(
        Target::Terrain([24, 40]),
        Some(Value::Terrain(TerrainValue {
            tile: 0,
            sub: 0,
            level: 5,
            ice: 7,
        })),
    )?;
    doc.set(Target::Overlay([26, 40]), Some(Value::Overlay(102, 3)))?;
    put(&mut doc, "CellTags", "40024", "TAG")?;
    put(&mut doc, "Tags", "TAG", "0,kept,TRIGGER")?;
    put(&mut doc, "Tubes", "0", "24,40,2,26,40,2,2,-1")?;
    base_nodes::write(&mut doc, "Americans", &[Node::new("GAPOWR", [24, 40])?])?;
    mission::create(&mut doc, Kind::Script, "MOVE", "Americans", None)?;
    put(&mut doc, "MOVE", "0", format!("4,{}", 40 * 128 + 24))?;
    put(&mut doc, "Basic", "HomeCell", "20")?;
    let mut action = mission::action(40, "0");
    for (i, v) in [3, 5, 28, 24].iter().enumerate() {
        action[3 + i] = v.to_string();
    }
    put(&mut doc, "Actions", "RECT", mission::encode_list(&[action]))?;
    doc.commit("geometry fixture");
    let before = doc.snapshot().bytes()?;
    let mut install = Install::open(&directory)?;
    let scene = load_scene_bytes_with_profile(
        &mut install,
        "fixture.map",
        "audit".into(),
        before.clone(),
        Instant::now(),
        Profile::Normal,
    )?;
    let footprints: Footprints = doc
        .records()
        .values()
        .filter(|r| r.section == "Structures")
        .map(|r| {
            (
                r.type_id(),
                ra2_map_scene::placement::offsets(&scene, &r.section, &r.type_id()),
            )
        })
        .collect();
    ensure!(
        footprints.values().any(|f| f.len() > 1),
        "real ART foundation unavailable"
    );
    let mut o = Options::from_document(&doc);
    o.width = 44;
    o.height = 44;
    o.offset = [2, 1];
    o.local = [2, 4, 40, 34];
    let p = map_settings::plan(&doc, o, &footprints)?;
    ensure!(p.can_apply(), "unexpected crop {:?}", p.summary);
    let summary = serde_json::to_value(&p.summary)?;
    ensure!(doc.snapshot().bytes()? == before, "planning modified map");
    p.apply(&mut doc)?;
    ensure!(
        doc.terrain([27, 43]).unwrap().ice == 7 && doc.overlay([29, 43]) == (102, 3),
        "packed migration"
    );
    ensure!(
        doc.ini_text("CellTags", "43027") == "TAG" && doc.ini_text("Basic", "HomeCell") == "20",
        "tag/home waypoint"
    );
    ensure!(
        base_nodes::read(&doc, "Americans")?[0].cell == [27, 43],
        "base migration"
    );
    ensure!(
        doc.ini_text("Tubes", "0") == "27,43,2,29,43,2,2,-1",
        "tube migration"
    );
    ensure!(
        doc.ini_text("MOVE", "0") == format!("4,{}", 43 * 128 + 27),
        "script migration"
    );
    for r in doc.records().values().filter(|r| r.section != "Waypoints") {
        ensure!(r.cell().is_some(), "migrated record has no cell");
    }
    let expanded = doc.snapshot().bytes()?;
    ensure!(
        doc.undo() && doc.snapshot().bytes()? == before,
        "resize undo"
    );
    ensure!(
        doc.redo() && doc.snapshot().bytes()? == expanded,
        "resize redo"
    );
    let mut o = Options::from_document(&doc);
    o.width = 40;
    o.height = 40;
    o.offset = [-2, -1];
    o.local = [2, 4, 36, 30];
    map_settings::plan(&doc, o, &footprints)?.apply(&mut doc)?;
    ensure!(
        doc.terrain([24, 40]).unwrap().ice == 7 && doc.overlay([26, 40]) == (102, 3),
        "crop roundtrip"
    );
    let mut o = Options::from_document(&doc);
    o.local = [3, 5, 30, 25];
    map_settings::plan(&doc, o, &footprints)?.apply(&mut doc)?;
    ensure!(map_settings::bounds(&doc).off_fc == 3, "LocalSize");
    let mut draft = Draft::read(&doc);
    for (key, value) in [
        ("Ambient", "0.85"),
        ("IonAmbient", "0.55"),
        ("IonBlue", "0.8"),
        ("DominatorAmbient", "0.3"),
        ("DominatorRed", "0.7"),
        ("DominatorLevel", "0.04"),
    ] {
        draft.values.insert(key.into(), value.into());
    }
    let unlit = doc.snapshot().bytes()?;
    let mut images = Vec::new();
    let mut rgba_profiles = Vec::new();
    for profile in Profile::ALL {
        let snapshot = draft.snapshot(&doc)?;
        let bytes = snapshot.bytes()?;
        let scene = load_scene_bytes_with_profile(
            &mut install,
            "light.map",
            "audit preview".into(),
            bytes.clone(),
            Instant::now(),
            profile,
        )?;
        let ini = IniFile::from_bytes(&bytes)?;
        let native = lighting::parse_lighting_profiles(&ini);
        let native_profile = match profile {
            Profile::Normal => native.normal.into(),
            Profile::Ion => native.ion.into(),
            Profile::Dominator => lighting::CellRelightProfile {
                units: native.dominator,
                top_level_units: 100,
            },
        };
        let grid = lighting::build_cell_light_grid_from_heights_and_units_with_detail(
            scene.cells.iter().map(|c| ((c.rx, c.ry), c.z)),
            native_profile,
            2,
        );
        for (cell, draw) in scene.cells.iter().zip(&scene.visual.draws) {
            ensure!(
                draw.tint == grid.terrain_tile_tint_at((cell.rx, cell.ry)),
                "native terrain light mismatch"
            );
        }
        let rect = scene.visual.bounds.context("no visual bounds")?;
        let size = fit(rect, 1024);
        let rgba = scene.visual.raster(rect, size, Default::default())?;
        let file = format!("lighting-{profile:?}.png");
        image::save_buffer(
            output.join(&file),
            &rgba,
            size[0],
            size[1],
            image::ColorType::Rgba8,
        )?;
        images.push(file);
        rgba_profiles.push(rgba);
    }
    ensure!(
        rgba_profiles[0] != rgba_profiles[1] && rgba_profiles[1] != rgba_profiles[2],
        "profiles did not relight"
    );
    ensure!(doc.snapshot().bytes()? == unlit, "draft dirtied source");
    draft.apply(&mut doc)?;
    doc.commit("lighting");
    let applied = doc.snapshot().bytes()?;
    ensure!(
        doc.undo() && doc.snapshot().bytes()? == unlit,
        "lighting undo"
    );
    ensure!(
        doc.redo() && doc.snapshot().bytes()? == applied,
        "lighting redo"
    );
    let mut publisher = Publisher::default();
    let published = publisher.map_bytes(applied, &directory)?;
    editor_io::validate(&published)?;
    let saved = output.join("map-settings.map");
    editor_io::replace_map(&saved, &published, editor_io::Stamp::file(&saved)?)?;
    let reopened = std::fs::read(&saved)?;
    let map = map_file::load_from_bytes(reopened.clone())?;
    let reopened = MapDocument::new(Baseline::parse(reopened.into(), &map)?);
    ensure!(
        reopened.base.header.width == 40 && reopened.base.header.local_left == 3,
        "saved dimensions"
    );
    ensure!(
        reopened.ini_text("Lighting", "IonAmbient") == "0.55"
            && reopened.terrain([24, 40]).unwrap().ice == 7,
        "saved content"
    );
    let report = serde_json::json!({"passed":true,"game_directory":directory,"geometry":summary,
        "migrated_tables":["IsoMapPack5","OverlayPack","OverlayDataPack","Structures","Units","Infantry","Aircraft","Terrain","Smudge","Waypoints","CellTags","House nodes","Tubes","Script4","Action40"],
        "active_art_foundations":true,"undo_redo_byte_exact":true,"crop_roundtrip":true,"local_size":true,
        "draft_read_only":true,"native_grid_matches_all_terrain_draws":true,"profiles":images,
        "published_save_reopen":true,"scope":"static profiles; no fades or local-light acceptance"});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", output.join("report.json").display());
    Ok(())
}
