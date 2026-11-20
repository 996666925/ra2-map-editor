use anyhow::Result;
use ra2_map_assets::assets::Install;
use ra2_map_document::document::{MapDocument, Target, TerrainValue, Value};
use ra2_map_scene::{
    cliff_authoring,
    map::{MapScene, load_scene_bytes},
    visual::ViewOptions,
};
use std::{path::Path, time::Instant};

fn save(scene: &MapScene, range: [[u16; 2]; 2], path: &Path) -> Result<()> {
    let mut rect = egui::Rect::NOTHING;
    for cell in &scene.cells {
        if (range[0][0]..=range[1][0]).contains(&cell.rx)
            && (range[0][1]..=range[1][1]).contains(&cell.ry)
            && let Some(r) = scene.cell_rect(cell)
        {
            rect = rect.union(r);
        }
    }
    let size = [rect.width().ceil() as u32, rect.height().ceil() as u32];
    let rgba = scene.visual.raster(
        rect,
        size,
        ViewOptions {
            lighting: false,
            ..Default::default()
        },
    )?;
    image::save_buffer(path, &rgba, size[0], size[1], image::ColorType::Rgba8)?;
    Ok(())
}

fn main() -> Result<()> {
    let mode = std::env::args().nth(1).unwrap();
    let out = Path::new("target/coastal-assets-review/root-probe");
    std::fs::create_dir_all(out)?;
    let mut install = Install::open(Path::new("H:/Game/Red-Alert-2-Yuris-Revenge"))?;
    if mode.starts_with("outward-") {
        return outward(&mut install, out, &mode);
    }
    if mode.starts_with("shelf-") {
        return shelf(&mut install, out, &mode);
    }
    if mode.starts_with("local-") {
        return local_shelf(&mut install, out, &mode);
    }
    for name in ["x", "y"] {
        let file = format!("{name}.map");
        let bytes =
            std::fs::read(Path::new("target/coastal-assets-review/shoulder-final").join(&file))?;
        let scene = load_scene_bytes(&mut install, &file, "probe".into(), bytes, Instant::now())?;
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let p = |a, c| if name == "x" { [a, c] } else { [c, a] };
        let stamp = |doc: &mut MapDocument, tile: u16, origin: [u16; 2]| -> Result<()> {
            let t = &scene.palette.templates[&(tile, 0)];
            for (sub, item) in t.cells.iter().enumerate() {
                if let Some(s) = item {
                    let at = [
                        origin[0] + sub as u16 % t.width,
                        origin[1] + sub as u16 / t.width,
                    ];
                    doc.set(
                        Target::Terrain(at),
                        Some(Value::Terrain(TerrainValue {
                            tile: scene.catalog.raw_tiles[&tile],
                            sub: sub as u8,
                            level: s.height,
                            ice: 0,
                        })),
                    )?;
                }
            }
            Ok(())
        };
        for (slot, (initial, thin, c)) in (if name == "x" {
            [(105, 108, 60), (89, 92, 76)]
        } else {
            [(113, 116, 60), (97, 100, 76)]
        })
        .into_iter()
        .enumerate()
        {
            match mode.as_str() {
                "v1" => stamp(&mut doc, initial + 1, p(81, c))?,
                "v2" => stamp(&mut doc, initial + 2, p(81, c))?,
                "thin" => {
                    stamp(&mut doc, thin, p(81, c))?;
                    stamp(&mut doc, thin, p(82, c))?;
                }
                "rock" | "land" | "rock-v2" => {
                    let narrow = if name == "x" { 165 } else { 155 };
                    let near = c + slot as u16;
                    let far = if slot == 0 { near - 1 } else { near + 1 };
                    stamp(&mut doc, narrow, p(79, far))?;
                    stamp(
                        &mut doc,
                        narrow - if mode == "land" { 99 } else { 0 },
                        p(79, near),
                    )?;
                    if mode == "rock-v2" {
                        stamp(&mut doc, initial + 2, p(81, c))?;
                    }
                }
                "flare" => {
                    if slot == 0 {
                        stamp(&mut doc, thin, p(81, c - 1))?;
                        stamp(&mut doc, if name == "x" { 110 } else { 117 }, p(82, c - 1))?;
                        stamp(&mut doc, 505, p(81, c + 1))?;
                    } else {
                        stamp(&mut doc, thin, p(81, c + 1))?;
                        stamp(&mut doc, if name == "x" { 94 } else { 101 }, p(82, c))?;
                        stamp(&mut doc, 503, p(81, c))?;
                    }
                }
                "wcliff" => {
                    let wet = c + slot as u16;
                    let dry = c + (1 - slot) as u16;
                    stamp(&mut doc, if name == "x" { 172 } else { 171 }, p(80, wet))?;
                    stamp(&mut doc, 503, p(81, dry))?;
                    stamp(&mut doc, 493, p(82, dry))?;
                }
                _ => {}
            }
        }
        if mode.contains("geom") {
            let start = p(78, 63);
            let coast = cliff_authoring::ramp(&doc, &scene, start, start, 0)?;
            println!("{mode} {name}: {} edits", coast.edits.len());
            coast.apply(&mut doc)?;
        }
        if mode.contains("landcliff") || mode.contains("narrowcliff") {
            let ids = scene.palette.tile_ids.unwrap();
            let land = ids.cliffs.cliff_set.unwrap();
            let water = ids.cliffs.water_cliffs.unwrap();
            for (cross, side) in [(59, -1), (78, 1)] {
                let at = p(79, cross);
                let old = doc.terrain(at).unwrap();
                let tile = scene.palette.raw_ids[&old.tile];
                let narrow = water + if name == "x" { 17 } else { 7 };
                if mode.contains("narrowcliff") {
                    for cross in cross..=cross + 1 {
                        let near = cross == if side < 0 { 60 } else { 78 };
                        stamp(
                            &mut doc,
                            narrow - if near { water - land } else { 0 },
                            p(79, cross),
                        )?;
                    }
                } else {
                    stamp(&mut doc, tile - water + land, at)?;
                }
            }
        }
        if let Some(depth) = mode.strip_prefix("recess-") {
            let mut flags = depth.split('-');
            let depth: u16 = flags.next().unwrap().parse()?;
            let flags = flags.next().unwrap_or("");
            let (base_ramp, body) = if name == "x" { (386, 31) } else { (384, 32) };
            let clear = scene.catalog.raw_tiles[&0];
            let saved = doc.fork();
            let read = |a, c| saved.terrain(p(a, c)).unwrap();
            for a in 78 - depth..=81 {
                for c in 61..=76 {
                    let old = read(a, c);
                    let tile = scene.palette.raw_ids[&old.tile];
                    let new = read(a + depth, c);
                    let moved = [base_ramp, base_ramp + 1, body]
                        .contains(&scene.palette.raw_ids[&new.tile]);
                    if moved || [base_ramp, base_ramp + 1, body].contains(&tile) {
                        let v = if moved {
                            new
                        } else {
                            TerrainValue {
                                tile: clear,
                                sub: 0,
                                level: 0,
                                ice: 0,
                            }
                        };
                        doc.set(Target::Terrain(p(a, c)), Some(Value::Terrain(v)))?;
                    }
                }
            }
            if flags == "lat" || flags == "rock" {
                for (a, c, tile) in [(81, 62, 505), (81, 75, 503), (82, 62, 506), (82, 75, 503)] {
                    stamp(&mut doc, tile, p(a, c))?;
                }
            }
            if flags == "rock" {
                let narrow = if name == "x" { 165 } else { 155 };
                for c in [59, 60, 77, 78] {
                    stamp(&mut doc, narrow, p(79, c))?;
                }
            }
        }
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(out.join(format!("{mode}-{file}")), &bytes)?;
        let edited = load_scene_bytes(&mut install, &file, "probe".into(), bytes, Instant::now())?;
        save(
            &edited,
            if name == "y" {
                [[59, 74], [78, 85]]
            } else {
                [[74, 59], [85, 78]]
            },
            &out.join(format!("{mode}-{name}.png")),
        )?;
    }
    Ok(())
}

fn shelf(install: &mut Install, out: &Path, mode: &str) -> Result<()> {
    use ra2_map_document::new_map::NewMapOptions;
    use ra2_map_scene::{cliff_tools::rectangle, shore_tools};
    use std::collections::BTreeSet;
    let mut flags = mode.split('-').skip(1);
    let depth: u16 = flags.next().unwrap().parse()?;
    let span: u16 = flags.next().unwrap_or("4").parse()?;
    let margin: u16 = flags.next().unwrap_or("4").parse()?;
    let scene = load_scene_bytes(
        install,
        "empty.map",
        "probe".into(),
        NewMapOptions::default().bytes()?,
        Instant::now(),
    )?;
    for name in ["x", "y"] {
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let p = |a, c| if name == "x" { [a, c] } else { [c, a] };
        let collect = |lo: [u16; 2], hi: [u16; 2]| rectangle(p(lo[0], lo[1]), p(hi[0], hi[1]));
        shore_tools::plan(&doc, &scene, rectangle([45, 45], [90, 90]), Some(true))?
            .apply(&mut doc)?;
        let mut shore = collect([53, 54], [83, 82]);
        for c in 54u16..=82 {
            let retreat = c.abs_diff(68).saturating_sub(8).div_ceil(2);
            for a in (84 - retreat)..=83 {
                shore.remove(&p(a, c));
            }
        }
        if let Err(e) = shore_tools::plan(&doc, &scene, shore, Some(false))
            .and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} shore: {e:#}");
            continue;
        }
        let mut high = collect([58, 58], [78, 78]);
        if margin > 0 {
            let removed: BTreeSet<_> = (79 - depth..=78)
                .flat_map(|a| ((60 - margin)..=(77 + margin).min(78)).map(move |c| p(a, c)))
                .collect();
            high.retain(|at| !removed.contains(at));
        } else {
            high = collect([58, 58], [78 - depth, 78]);
        }
        if span > 4 {
            // Leave both corners on the original shoreline axis to exercise the notch.
            high.extend(collect([58, 58], [78, 58 + span - 4]));
            high.extend(collect([58, 78 - (span - 4)], [78, 78]));
        }
        if let Err(e) = cliff_authoring::reshape(&doc, &scene, high, 0, false)
            .and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} cliff: {e:#}");
            continue;
        }
        let start = p(78 - depth, 63);
        let end = p(78 - depth, 73);
        if let Err(e) =
            cliff_authoring::ramp(&doc, &scene, start, end, 0).and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} ramp: {e:#}");
            continue;
        }
        let mut dump = String::new();
        for a in 72..=86 {
            dump.push_str(&format!(
                "{a}: {}\n",
                (51..=85)
                    .map(|c| {
                        let v = doc.terrain(p(a, c)).unwrap();
                        format!("{:03}.{}", scene.palette.raw_ids[&v.tile], v.sub)
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        std::fs::write(out.join(format!("{mode}-{name}.txt")), dump)?;
        let bytes = doc.snapshot().bytes()?;
        let file = format!("{mode}-{name}.map");
        std::fs::write(out.join(&file), &bytes)?;
        let edited = load_scene_bytes(install, &file, "probe".into(), bytes, Instant::now())?;
        save(
            &edited,
            if name == "y" {
                [[52, 69], [84, 86]]
            } else {
                [[69, 52], [86, 84]]
            },
            &out.join(format!("{mode}-{name}.png")),
        )?;
    }
    Ok(())
}
fn local_shelf(install: &mut Install, out: &Path, mode: &str) -> Result<()> {
    use ra2_map_document::new_map::NewMapOptions;
    use ra2_map_scene::{cliff_tools::rectangle, shore_tools};
    use std::collections::BTreeSet;
    let mut options = mode.split('-').skip(1);
    let depth: u16 = options.next().unwrap().parse()?;
    let margin: u16 = options.next().unwrap().parse()?;
    let steps: u16 = options.next().unwrap_or("0").parse()?;
    let scene = load_scene_bytes(
        install,
        "empty.map",
        "probe".into(),
        NewMapOptions::default().bytes()?,
        Instant::now(),
    )?;
    let ids = scene.palette.tile_ids.unwrap();
    for name in ["x", "y"] {
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let p = |a, c| if name == "x" { [a, c] } else { [c, a] };
        let collect = |lo: [u16; 2], hi: [u16; 2]| rectangle(p(lo[0], lo[1]), p(hi[0], hi[1]));
        shore_tools::plan(&doc, &scene, rectangle([45, 45], [90, 90]), Some(true))?
            .apply(&mut doc)?;
        let high = collect([58, 55], [78 - depth, 81]);
        cliff_authoring::reshape(&doc, &scene, high, 0, false)?.apply(&mut doc)?;
        let lo = 61 - margin;
        let hi = 76 + margin;
        let mut pad = BTreeSet::new();
        for c in lo..=hi {
            let retreat = (lo + steps)
                .saturating_sub(c)
                .max(c.saturating_sub(hi - steps));
            for a in (81 - depth)..=84 - retreat {
                let v = doc.terrain(p(a, c)).unwrap();
                let tile = scene.palette.raw_ids[&v.tile];
                if (ids.water_base..ids.water_base + 14).contains(&i32::from(tile)) {
                    pad.insert(p(a, c));
                }
            }
        }
        let treatment = options.next().unwrap_or("");
        if treatment == "wrap" {
            for c in 52..=84 {
                for a in 70..=84 {
                    let retreat = match c {
                        52 | 84 => 2,
                        53 | 83 => 1,
                        _ => 0,
                    };
                    let front = if a >= 79 - depth {
                        84 - retreat
                    } else {
                        78 - depth
                    };
                    let v = doc.terrain(p(a, c)).unwrap();
                    let tile = scene.palette.raw_ids[&v.tile];
                    if a <= front
                        && (ids.water_base..ids.water_base + 14).contains(&i32::from(tile))
                    {
                        pad.insert(p(a, c));
                    }
                }
            }
        }
        if treatment == "lip" {
            // Turn the ends back briefly around the front corners. Try the same
            // lower pad in both axes without covering any cliff subtiles.
            for c in (55 - margin)..=(81 + margin) {
                for a in (78 - depth - steps)..=84 {
                    let v = doc.terrain(p(a, c)).unwrap();
                    let tile = scene.palette.raw_ids[&v.tile];
                    if (ids.water_base..ids.water_base + 14).contains(&i32::from(tile)) {
                        pad.insert(p(a, c));
                    }
                }
            }
        }
        if let Err(e) =
            shore_tools::plan(&doc, &scene, pad, Some(false)).and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} shore: {e:#}");
            continue;
        }
        let start = p(78 - depth, 63);
        let end = p(78 - depth, 73);
        if let Err(e) =
            cliff_authoring::ramp(&doc, &scene, start, end, 0).and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} ramp: {e:#}");
            continue;
        }
        let mut dump = String::new();
        for a in 72..=86 {
            dump.push_str(&format!(
                "{a}: {}\n",
                (51..=85)
                    .map(|c| {
                        let v = doc.terrain(p(a, c)).unwrap();
                        format!("{:03}.{}", scene.palette.raw_ids[&v.tile], v.sub)
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        std::fs::write(out.join(format!("{mode}-{name}.txt")), dump)?;
        let bytes = doc.snapshot().bytes()?;
        let file = format!("{mode}-{name}.map");
        std::fs::write(out.join(&file), &bytes)?;
        let edited = load_scene_bytes(install, &file, "probe".into(), bytes, Instant::now())?;
        save(
            &edited,
            if name == "y" {
                [[53, 72], [83, 86]]
            } else {
                [[72, 53], [86, 83]]
            },
            &out.join(format!("{mode}-{name}.png")),
        )?;
    }
    Ok(())
}
fn outward(install: &mut Install, out: &Path, mode: &str) -> Result<()> {
    use ra2_map_document::new_map::NewMapOptions;
    use ra2_map_scene::{cliff_tools::rectangle, shore_tools};
    let mut args = mode.split('-').skip(1);
    let depth: u16 = args.next().unwrap().parse()?;
    let gap: u16 = args.next().unwrap().parse()?;
    let pad_width: Option<u16> = args.next().map(str::parse).transpose()?;
    let pad_depth: u16 = args.next().unwrap_or("3").parse()?;
    let scene = load_scene_bytes(
        install,
        "empty.map",
        "probe".into(),
        NewMapOptions::default().bytes()?,
        Instant::now(),
    )?;
    for name in ["x", "y"] {
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let p = |a, c| if name == "x" { [a, c] } else { [c, a] };
        let collect = |lo: [u16; 2], hi: [u16; 2]| rectangle(p(lo[0], lo[1]), p(hi[0], hi[1]));
        shore_tools::plan(&doc, &scene, rectangle([42, 42], [96, 96]), Some(true))?
            .apply(&mut doc)?;
        let mut high = collect([53, 51], [78 + depth, 87]);
        for at in collect([79, 61 - gap], [78 + depth, 76 + gap]) {
            high.remove(&at);
        }
        if let Err(e) = cliff_authoring::reshape(&doc, &scene, high, 0, false)
            .and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} cliff: {e:#}");
            continue;
        }
        if let Some(width) = pad_width {
            let ids = scene.palette.tile_ids.unwrap();
            let pad = collect([81, 61 - width], [80 + pad_depth, 76 + width])
                .into_iter()
                .filter(|&at| {
                    let v = doc.terrain(at).unwrap();
                    (ids.water_base..ids.water_base + 14)
                        .contains(&i32::from(scene.palette.raw_ids[&v.tile]))
                })
                .collect();
            if let Err(e) = shore_tools::plan(&doc, &scene, pad, Some(false))
                .and_then(|plan| plan.apply(&mut doc))
            {
                eprintln!("{mode} {name} shore: {e:#}");
                continue;
            }
        }
        let start = p(78, 63);
        let end = p(78, 73);
        if let Err(e) =
            cliff_authoring::ramp(&doc, &scene, start, end, 0).and_then(|plan| plan.apply(&mut doc))
        {
            eprintln!("{mode} {name} ramp: {e:#}");
            continue;
        }
        let bytes = doc.snapshot().bytes()?;
        let file = format!("{mode}-{name}.map");
        std::fs::write(out.join(&file), &bytes)?;
        let edited = load_scene_bytes(install, &file, "probe".into(), bytes, Instant::now())?;
        save(
            &edited,
            [p(74, 55), p(89, 82)],
            &out.join(format!("{mode}-{name}.png")),
        )?;
    }
    Ok(())
}
