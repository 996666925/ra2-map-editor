use anyhow::{Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
use ra2_map_scene::{cliff_authoring, cliff_tools, map::load_scene_bytes};
use std::time::Instant;
fn main() -> Result<()> {
    let mut install = Install::open(std::path::Path::new("D:/rc2/yl"))?;
    let scene = load_scene_bytes(
        &mut install,
        "probe.map",
        "probe".into(),
        NewMapOptions {
            width: 60,
            height: 60,
            level: 4,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    let plateau = cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)?;
    plateau.apply(&mut doc)?;
    doc.commit("plateau");
    let ids = scene.palette.tile_ids.unwrap();
    let mut failed = Vec::new();
    for stamp in &plateau.cliffs {
        let piece = stamp.tile - ids.cliffs.cliff_set.unwrap() + 1;
        if ![1, 12, 21, 29].contains(&piece) {
            continue;
        }
        let t = &scene.palette.templates[&(stamp.tile, 0)];
        for (sub, c) in t.cells.iter().enumerate() {
            if c.is_none() {
                continue;
            }
            let at = [
                stamp.origin[0] + sub as u16 % t.width,
                stamp.origin[1] + sub as u16 / t.width,
            ];
            match cliff_authoring::ramp(
                &doc,
                &scene,
                at,
                at,
                cliff_authoring::infer_base(&doc, &scene, at),
            ) {
                Ok(_) => (),
                Err(e) => failed.push(format!("cliff piece {piece} sub {sub} at {at:?}: {e:#}")),
            }
        }
    }
    for start in [[35, 35], [65, 35], [35, 65], [65, 65]] {
        let p = cliff_authoring::ramp(&doc, &scene, start, start, 4)?;
        p.apply(&mut doc)?;
        doc.commit("ramp");
        for (&at, v) in &p.edits {
            let id = scene.palette.raw_ids[&v.tile];
            if !(ids.slope_set_pieces..ids.slope_set_pieces + 10).contains(&i32::from(id)) {
                continue;
            }
            let index = i32::from(id) - ids.slope_set_pieces;
            let axis = usize::from(!(index <= 1 || (4..=6).contains(&index)));
            for sign in [-1, 1] {
                let mut end = at;
                end[axis] = (i32::from(end[axis]) + sign * 4) as u16;
                if let Err(e) = cliff_authoring::ramp(
                    &doc,
                    &scene,
                    at,
                    end,
                    cliff_authoring::infer_base(&doc, &scene, at),
                ) {
                    failed.push(format!(
                        "corner {start:?} cap {index} sub {} at {at:?}->{end:?}: {e:#}",
                        v.sub
                    ));
                }
            }
        }
        doc.undo();
    }
    for f in &failed {
        println!("{f}");
    }
    ensure!(failed.is_empty(), "{} failed", failed.len());
    println!("all passed");
    Ok(())
}
