//! Real-resource publication, remap, lighting and static bridge checks.
use anyhow::{Context, Result, ensure};
use ra2_map_document::{
    document::{MapDocument, Target, Value},
    new_map::NewMapOptions,
};
use ra2_map_io::publication::{Publisher, fit};
use ra2_map_scene::visual::ViewOptions;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p3-publication-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut publisher = Publisher::default();
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        level: 0,
        players: 4,
        ..Default::default()
    }
    .bytes()?;
    let mut bytes = String::from_utf8(bytes)?
        .replace("Ambient=1.000000", "Ambient=0.650000\nLevel=0.02")
        .replace("Red=1.000000", "Red=0.800000")
        .replace("Green=1.000000", "Green=0.900000")
        .into_bytes();
    bytes.extend_from_slice(
        b"\n[RedHouse]\nCountry=Russians\nColor=Red\n[BlueHouse]\nCountry=Americans\nColor=Blue\n",
    );
    let scene = publisher.scene(bytes, &directory)?;
    let mut doc = MapDocument::new(
        scene
            .source
            .edit_base
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?
            .clone(),
    );
    for (section, kind, at, owner) in [
        ("Structures", "GAPOWR", [35, 30], "RedHouse"),
        ("Structures", "GAPOWR", [43, 30], "BlueHouse"),
        ("Units", "HTNK", [35, 45], "RedHouse"),
        ("Units", "HTNK", [43, 45], "BlueHouse"),
        ("Units", "HTNK", [38, 38], "RedHouse"),
        ("Units", "HTNK", [41, 40], "BlueHouse"),
    ] {
        let id = doc.add_record(section, at, owner, kind)?;
        if at == [41, 40] {
            let r = doc.records()[&id].patched(&[(10, "1".into())])?;
            doc.set(Target::Record(id), Some(Value::Record(r)))?;
        }
    }
    // Stock high-bridge NS middle pieces, including the alternate frame family.
    let bridge = scene
        .catalog
        .overlays
        .iter()
        .position(|n| n.eq_ignore_ascii_case("BRIDGE1"))
        .context("no BRIDGE1")? as u8;
    for x in 37..=44 {
        doc.set(
            Target::Overlay([x, 40]),
            Some(Value::Overlay(bridge, if x < 41 { 0 } else { 9 })),
        )?;
    }
    doc.commit("publication fixture");
    let bytes = doc.snapshot().bytes()?;
    let scene = publisher.scene(bytes.clone(), &directory)?;
    let rect = scene.visual.bounds.context("empty visual scene")?;
    let mut remapped_pixels = 0usize;
    let mut unaltered_pixels = 0usize;
    for surface in &scene.visual.surfaces {
        for (a, b) in surface
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(surface.original.as_chunks::<4>().0)
        {
            if a[3] > 0 {
                if a[..3] != b[..3] {
                    remapped_pixels += 1;
                } else {
                    unaltered_pixels += 1;
                }
            }
        }
    }
    ensure!(
        remapped_pixels > 100 && unaltered_pixels > 100,
        "real SHP/VXL remap absent"
    );
    let mut ramps = std::collections::BTreeSet::new();
    let mut remap_by_kind = std::collections::BTreeMap::<String, usize>::new();
    for draw in scene.visual.draws.iter().filter(|d| d.owner.is_some()) {
        let surface = &scene.visual.surfaces[draw.surface];
        if surface.original != surface.rgba {
            let (section, _) = &scene.visual.owners[&draw.owner.unwrap()];
            *remap_by_kind.entry(section.clone()).or_default() += surface
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .zip(surface.original.as_chunks::<4>().0)
                .filter(|(a, b)| a[..3] != b[..3])
                .count();
            ramps.insert(
                surface
                    .rgba
                    .iter()
                    .fold(0u64, |h, &v| h.wrapping_mul(31).wrapping_add(u64::from(v))),
            );
        }
    }
    ensure!(ramps.len() >= 2, "House colors collapsed");
    ensure!(
        remap_by_kind.contains_key("Units") && remap_by_kind.contains_key("Structures"),
        "SHP and VXL must both remap: {remap_by_kind:?}"
    );
    let size = fit(rect, 1024);
    let normal = scene.visual.raster(rect, size, Default::default())?;
    let raw = scene.visual.raster(
        rect,
        size,
        ViewOptions {
            remap: false,
            lighting: false,
            ..Default::default()
        },
    )?;
    ensure!(normal != raw, "lighting/remap had no visible effect");
    ensure!(
        scene
            .visual
            .draws
            .iter()
            .any(|d| d.layer == 0 && d.tint[0] < 0.8),
        "map light ignored"
    );
    let mut bridge_visible = 0;
    let mut bridge_hidden = 0;
    for draw in scene.visual.draws.iter().filter(|d| d.owner.is_some()) {
        let id = draw.owner.unwrap();
        let (_, key) = &scene.visual.owners[&id];
        let record = doc
            .records()
            .into_values()
            .find(|r| r.section == "Units" && &r.key == key);
        if !record.is_some_and(|r| r.cell() == Some([38, 38])) {
            continue;
        }
        let s = &scene.visual.surfaces[draw.surface];
        for y in 0..s.size[1] {
            for x in 0..s.size[0] {
                if s.rgba[((y * s.size[0] + x) * 4 + 3) as usize] == 0 {
                    continue;
                }
                let point = egui::pos2(
                    draw.position[0] + x as f32 + 0.5,
                    draw.position[1] + y as f32 + 0.5,
                );
                if scene.visual.sample(point, Default::default()).1 == Some(id) {
                    bridge_visible += 1;
                } else {
                    bridge_hidden += 1;
                }
            }
        }
    }
    ensure!(bridge_hidden > 0, "bridge did not occlude the ground unit");
    let published = publisher.map_bytes(bytes.clone(), &directory)?;
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&published)?;
    let preview = vera20k_core::map::preview::decode_preview_image_from_ini(&ini)?
        .context("no PreviewPack")?;
    ensure!(
        preview.rgba
            == scene
                .visual
                .raster(rect, [preview.width, preview.height], Default::default())?,
        "PreviewPack mismatch"
    );
    std::fs::write(output.join("publish-fixture.map"), published)?;
    let png_size = publisher.png(
        bytes.clone(),
        &directory,
        &output.join("full-map.png"),
        4096,
        Default::default(),
    )?;
    // Replacing an existing export must work on Windows too.
    publisher.png(
        bytes.clone(),
        &directory,
        &output.join("full-map.png"),
        4096,
        Default::default(),
    )?;
    let native_size = publisher.png(
        bytes.clone(),
        &directory,
        &output.join("native-map.png"),
        0,
        Default::default(),
    )?;
    publisher.png(
        bytes,
        &directory,
        &output.join("unlit-raw.png"),
        4096,
        ViewOptions {
            remap: false,
            lighting: false,
            ..Default::default()
        },
    )?;
    let report = serde_json::json!({"passed":true,"remapped_pixels":remapped_pixels,"remap_by_kind":remap_by_kind,"unchanged_sprite_pixels":unaltered_pixels,"distinct_surfaces":ramps.len(),"bridge_unit_visible_pixels":bridge_visible,"bridge_unit_hidden_pixels":bridge_hidden,"png_size":png_size,"native_png_size":native_size,"preview_size":[preview.width,preview.height],"scope":"static fixture; no native-client pixel parity claim"});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
