use image::{Rgba, RgbaImage, imageops};
use ra2_map_assets::assets::Install;
use ra2_map_document::document::MapDocument;
use ra2_map_scene::{cliff_tools::rectangle, ice_tools, map, shore_tools, terrain_tools::value};
use std::{path::Path, time::Instant};

fn main() -> anyhow::Result<()> {
    let game = std::env::var("RA2_DIR")?;
    let mut install = Install::open(Path::new(&game))?;
    let bytes = ra2_map_document::new_map::NewMapOptions {
        theater: "SNOW".into(),
        width: 36,
        height: 36,
        ..Default::default()
    }
    .bytes()?;
    let plain = map::load_scene_bytes(
        &mut install,
        "stock.map",
        "review".into(),
        bytes,
        Instant::now(),
    )?;
    let scene = map::load_scene(&mut install, "vera-ice-lake.map")?;
    let mut doc = MapDocument::new(plain.source.edit_base.unwrap());
    let sets = scene.palette.bundled_ice_sets.unwrap();

    std::fs::create_dir_all("target/ice-coast-review")?;
    let mut sheet = RgbaImage::from_pixel(64 * 16, 38 * 9, Rgba([25, 29, 35, 255]));
    for (family, base) in sets.into_iter().enumerate() {
        for piece in 17..=62u16 {
            let art = &scene.palette.previews[&(base + piece, 0)];
            let rgba =
                RgbaImage::from_raw(art.info.width, art.info.height, art.rgba.clone()).unwrap();
            let index = i64::from(piece - 17);
            imageops::overlay(
                &mut sheet,
                &rgba,
                index % 16 * 64,
                (index / 16 * 3 + family as i64) * 38,
            );
        }
    }
    sheet.save("target/ice-coast-review/ice-edge-variants.png")?;
    let mut lake = rectangle([31, 31], [41, 42]);
    lake.extend(rectangle([29, 33], [30, 39]));
    lake.extend(rectangle([32, 43], [40, 44]));
    let shore = shore_tools::plan(&doc, &scene, lake, Some(true))?;
    shore.apply(&mut doc)?;
    doc.commit("stock shore");
    let selection = rectangle([28, 29], [43, 43]);
    let mut cells: Vec<_> = rectangle([24, 25], [47, 46]).into_iter().collect();
    cells.sort_by_key(|&[x, y]| (x + y, x));
    let mut comparison = RgbaImage::from_pixel(1508, 540, Rgba([25, 29, 35, 255]));
    let plan = ice_tools::plan(&doc, &scene, selection, Default::default())?;
    for (i, mixed) in [false, true].into_iter().enumerate() {
        let mut image = RgbaImage::from_pixel(750, 540, Rgba([25, 29, 35, 255]));
        for &at @ [x, y] in &cells {
            let v = plan
                .edits
                .get(&at)
                .copied()
                .unwrap_or_else(|| value(&doc, &scene, at));
            let mut tile_id = scene.palette.raw_ids.get(&v.tile).copied().unwrap_or(0);
            if !mixed
                && let Some(piece) = sets.into_iter().find_map(|base| {
                    tile_id
                        .checked_sub(base)
                        .filter(|piece| (17..=62).contains(piece))
                })
            {
                tile_id = sets[0] + piece;
            }
            let key = vera20k_core::map::theater::TileKey {
                tile_id,
                sub_tile: v.sub,
                variant: 0,
            };
            let Some(art) = scene.palette.images.get(&key) else {
                continue;
            };
            let rgba =
                RgbaImage::from_raw(art.info.width, art.info.height, art.rgba.clone()).unwrap();
            imageops::overlay(
                &mut image,
                &rgba,
                i64::from((i32::from(x) - i32::from(y)) * 30 + art.info.offset[0] + 390),
                i64::from((i32::from(x) + i32::from(y)) * 15 + art.info.offset[1] - 815),
            );
        }
        imageops::overlay(&mut comparison, &image, 758 * i as i64, 0);
    }
    comparison.save("target/ice-coast-review/ice-bank-variants-comparison.png")?;
    Ok(())
}
