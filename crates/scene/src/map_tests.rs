use super::*;

fn source_bytes() -> Vec<u8> {
    let mut bytes=b"; keep comments, CRLF and old encoding: \xD6\xD0\xCE\xC4\r\n[Map]\r\nTheater=TEMPERATE\r\nSize=0,0,3,3\r\nLocalSize=0,0,3,3\r\n[Basic]\r\nName=Test\r\n[IsoMapPack5]\r\n1=DwALABwCAAIAAAAAAAACBxEAAA==\r\n[UnknownExtension]\r\nDuplicate=first\r\nDuplicate=second\r\nEmpty=\r\n[Digest]\r\n1=stale\r\n".to_vec();
    bytes.extend_from_slice(b"; eof without newline");
    bytes
}
fn source() -> SourceMap {
    SourceMap::parse("合成.map".into(), "fixture".into(), source_bytes())
        .unwrap()
        .0
}

#[test]
#[ignore = "requires stock YR assets in RA2_DIR and the engine ice pack"]
fn ice_lake_loads_and_ordinary_snow_restores_the_stock_palette() {
    let root = std::env::var_os("RA2_DIR").expect("RA2_DIR required");
    let mut install = Install::open(Path::new(&root)).unwrap();
    let stock = ra2_map_document::new_map::NewMapOptions {
        theater: "SNOW".into(),
        width: 20,
        height: 20,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let load_stock = |install: &mut Install| {
        load_scene_bytes(
            install,
            "stock.map",
            "fixture".into(),
            stock.clone(),
            Instant::now(),
        )
        .unwrap()
    };
    let plain = load_stock(&mut install);
    assert!(!plain.palette.supports_ice);
    assert!(!plain.palette.raw_ids.contains_key(&4096));
    let lake = load_scene(&mut install, "vera-ice-lake.map").unwrap();
    assert!(lake.palette.supports_ice);
    assert_eq!(lake.palette.bundled_ice_sets, Some([4096, 4160, 4224]));
    assert!(lake.brushes.iter().any(|brush| brush.raw == 4096));
    assert!(lake.cells.iter().any(|cell| cell.tile_id == 4096));
    assert_eq!(
        lake.palette.templates[&(4159, 0)].cells[0].unwrap().terrain,
        9
    );
    assert!(lake.images.keys().any(|key| key.tile_id == 4096));

    // Real stock water receives VERA ice and water-side edges, both saved in the map.
    use ra2_map_document::document::{MapDocument, Target, TerrainValue, Value};
    let mut authored = MapDocument::new(plain.source.edit_base.as_ref().unwrap().clone());
    authored
        .set_ini("Map", "VeraIce", Some(b"yes".to_vec()))
        .unwrap();
    let water = lake.palette.tile_ids.unwrap().water_base as u16;
    let raw = lake.catalog.raw_tiles[&water];
    let selection = crate::cliff_tools::rectangle([19, 19], [22, 21]);
    for &at in &selection {
        authored
            .set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue {
                    tile: raw,
                    sub: (at[0] % 2 + (at[1] % 2) * 2) as u8,
                    level: 2,
                    ice: 0,
                })),
            )
            .unwrap();
    }
    // The last stock WaterSet piece is a one-cell shallow crossing, not open water.
    let shallows = lake.catalog.raw_tiles[&(water + 13)];
    authored
        .set(
            Target::Terrain([19, 19]),
            Some(Value::Terrain(TerrainValue {
                tile: shallows,
                sub: 0,
                level: 2,
                ice: 0,
            })),
        )
        .unwrap();
    authored.commit("water fixture");
    let stroke = std::collections::BTreeSet::from([[20, 20], [21, 20]]);
    let plan =
        crate::ice_tools::plan(&authored, &lake, stroke.clone(), Default::default()).unwrap();
    assert_eq!(plan.skipped.len(), 0);
    assert_eq!(plan.edits[&[20, 20]].tile, 4110);
    assert_eq!(plan.edits[&[21, 20]].tile, 4104);
    for (at, tile) in [([22, 20], 4120), ([22, 19], 4147)] {
        let edge = plan.edits[&at];
        assert!(edge.tile == tile || edge.tile == tile + 64);
        assert_eq!((edge.level, edge.ice), (2, 0));
        assert_eq!(
            lake.palette.templates[&(edge.tile as u16, 0)].cells[0]
                .unwrap()
                .terrain,
            9
        );
    }
    assert!(!plan.edits.contains_key(&[18, 20]));
    assert!(!plan.edits.contains_key(&[19, 19]));
    plan.apply(&mut authored).unwrap();
    authored.commit("freeze water");
    assert!(
        crate::ice_tools::plan(&authored, &lake, stroke, Default::default())
            .unwrap()
            .edits
            .is_empty()
    );
    let reloaded = load_scene_bytes(
        &mut install,
        "ice-fill.map",
        "fixture".into(),
        authored.snapshot().bytes().unwrap(),
        Instant::now(),
    )
    .unwrap();
    assert!(reloaded.palette.supports_ice);
    let mut reloaded = MapDocument::new(reloaded.source.edit_base.unwrap());
    assert_eq!(reloaded.terrain([20, 20]), Some(plan.edits[&[20, 20]]));
    assert_eq!(reloaded.terrain([22, 19]), Some(plan.edits[&[22, 19]]));

    assert!(reloaded.set_ini("Map", "VeraIce", None).is_err());
    let thaw = crate::shore_tools::plan(&reloaded, &lake, [[21, 20]].into(), Some(true)).unwrap();
    assert!(thaw.cliffs.is_empty());
    let edge = thaw.edits[&[21, 20]];
    assert!(matches!(edge.tile, 4120 | 4184));
    assert_eq!(edge.ice, 0);
    assert_eq!(thaw.edits[&[20, 20]].tile, 4096); // Still solid, now open on all sides.
    assert_eq!(thaw.edits[&[20, 20]].ice, 1);
    for at in [[22, 20], [22, 19]] {
        assert!(crate::shore_tools::flat_water(&lake, thaw.edits[&at]));
        assert!(!crate::ice_tools::bundled_tile(&lake, thaw.edits[&at]));
    }
    thaw.apply(&mut reloaded).unwrap();
    assert!(
        crate::shore_tools::plan(&reloaded, &lake, [[21, 20]].into(), Some(true))
            .unwrap()
            .edits
            .is_empty()
    );
    let last = crate::shore_tools::plan(&reloaded, &lake, [[20, 20]].into(), Some(true)).unwrap();
    assert!(crate::shore_tools::flat_water(&lake, last.edits[&[21, 20]]));
    last.apply(&mut reloaded).unwrap();
    reloaded.commit("thaw all ice");

    // Repaint a complete stock shoreline, including its wet corner subtiles.
    let mut bank_doc = MapDocument::new(plain.source.edit_base.as_ref().unwrap().clone());
    let lake_box = crate::cliff_tools::rectangle([15, 15], [24, 24]);
    let shore = crate::shore_tools::plan(&bank_doc, &lake, lake_box.clone(), Some(true)).unwrap();
    shore.apply(&mut bank_doc).unwrap();
    bank_doc.commit("stock shore");
    let direct = crate::ice_tools::plan(
        &bank_doc,
        &lake,
        lake_box.clone(),
        crate::ice_tools::Options {
            soften_shore: false,
            ..Default::default()
        },
    )
    .unwrap();
    let transition =
        crate::ice_tools::plan(&bank_doc, &lake, lake_box.clone(), Default::default()).unwrap();
    let fringe = direct
        .cells
        .difference(&transition.cells)
        .copied()
        .collect::<Vec<_>>();
    assert!(!fringe.is_empty());
    assert!(!transition.cells.is_empty());
    let mut styles = std::collections::BTreeSet::new();
    for at in fringe {
        let tile = transition.edits[&at];
        assert!((4113..4159).contains(&tile.tile) || (4177..4223).contains(&tile.tile));
        styles.insert((tile.tile - 4096) / 64);
        assert_eq!(
            lake.palette.templates[&(tile.tile as u16, 0)].cells[0]
                .unwrap()
                .terrain,
            9
        );
        assert_eq!(tile.ice, 0);
    }
    assert_eq!(styles, std::collections::BTreeSet::from([0, 1]));
    assert_ne!(
        lake.palette.previews[&(4116, 0)].rgba,
        lake.palette.previews[&(4180, 0)].rgba
    );
    for (&at, &v) in &shore.edits {
        if crate::shore_tools::shore(&lake, lake.palette.raw_ids[&v.tile]) {
            assert!(!transition.edits.contains_key(&at));
        }
    }
    transition.apply(&mut bank_doc).unwrap();
    assert!(
        crate::ice_tools::plan(&bank_doc, &lake, lake_box.clone(), Default::default())
            .unwrap()
            .edits
            .is_empty()
    );
    bank_doc.commit("ice at the stock shore");

    // Partial water repaint has no artificial beach between water and ice.
    let center = [20, 20];
    let center_ice = bank_doc.terrain(center).unwrap();
    assert!(crate::ice_tools::bundled_tile(&lake, center_ice));
    assert_eq!(center_ice.ice, 1);
    let inner = crate::shore_tools::plan(&bank_doc, &lake, [center].into(), Some(true)).unwrap();
    assert!(inner.cliffs.is_empty());
    assert!(inner.edits.len() > 1);
    assert_eq!(inner.edits[&center].ice, 0);
    let inner_edge = inner.edits[&center].tile;
    assert!(matches!(inner_edge, 4127 | 4191));
    assert_eq!(
        lake.palette.templates[&(inner_edge as u16, 0)].cells[0]
            .unwrap()
            .terrain,
        9
    );
    inner.apply(&mut bank_doc).unwrap();
    bank_doc.commit("open water in the ice");

    // Painting out to the old bank uses the normal shoreline fitter.
    let [[left, top], [right, bottom]] = [[15, 15], [24, 24]];
    let water = crate::shore_tools::polyline(
        &bank_doc,
        &lake,
        &[[left, top], [right, top], [right, bottom], [left, bottom]],
    )
    .unwrap();
    water.apply(&mut bank_doc).unwrap();
    for at in lake_box {
        let v = crate::terrain_tools::value(&bank_doc, &lake, at);
        assert!(crate::shore_tools::wet(&lake, v));
        assert!(!crate::ice_tools::bundled_tile(&lake, v));
        assert_eq!(v.ice, 0);
    }

    // The stock shoreline fits a diagonal as a run of short, convex steps.
    // This is the case where there is no long straight shore to back a corner.
    let mut stairs = MapDocument::new(plain.source.edit_base.as_ref().unwrap().clone());
    let path = [[14, 15], [22, 23], [27, 18], [19, 10]];
    let shore = crate::shore_tools::outline(&stairs, &lake, &path).unwrap();
    shore.apply(&mut stairs).unwrap();
    let selection = crate::cliff_tools::rectangle([13, 9], [28, 24]);
    let hard = crate::ice_tools::plan(
        &stairs,
        &lake,
        selection.clone(),
        crate::ice_tools::Options {
            soften_shore: false,
            ..Default::default()
        },
    )
    .unwrap();
    hard.apply(&mut stairs).unwrap();
    let rounded =
        crate::ice_tools::plan(&stairs, &lake, selection.clone(), Default::default()).unwrap();
    assert!(hard.cells.len() - rounded.cells.len() >= 12);
    for at in hard.cells.difference(&rounded.cells) {
        let piece = rounded.edits[at];
        let id = lake.palette.raw_ids[&piece.tile];
        assert_eq!(
            lake.palette.templates[&(id, 0)].cells[0].unwrap().terrain,
            9
        );
        assert_eq!(piece.ice, 0);
    }
    for (&at, &v) in &shore.edits {
        if crate::shore_tools::shore(&lake, lake.palette.raw_ids[&v.tile]) {
            assert!(!rounded.edits.contains_key(&at));
        }
    }
    rounded.apply(&mut stairs).unwrap();
    assert!(
        crate::ice_tools::plan(&stairs, &lake, selection, Default::default())
            .unwrap()
            .edits
            .is_empty()
    );

    // After thawing all pieces, removal is safe and must rebuild the theater.
    let mut opt = reloaded;
    let before = opt.snapshot();
    opt.set_ini("Map", "VeraIce", None).unwrap();
    opt.commit("disable ice");
    assert!(
        crate::work::update_records(
            &lake,
            &before,
            &opt.snapshot(),
            &install,
            Default::default(),
            &|| false,
        )
        .unwrap()
        .is_none()
    );

    let mut doc = ra2_map_document::document::MapDocument::new(
        plain.source.edit_base.as_ref().unwrap().clone(),
    );
    let before = doc.snapshot();
    doc.waypoint(0, [20, 20]).unwrap();
    doc.commit("move start");
    // If a canceled preview left the install on the ice palette, the next
    // stock snapshot needs a full reload even with the same resource revision.
    assert!(
        crate::work::update_records(
            &plain,
            &before,
            &doc.snapshot(),
            &install,
            Default::default(),
            &|| false
        )
        .unwrap()
        .is_none()
    );
    let plain = load_stock(&mut install);
    assert!(!plain.palette.supports_ice);
    assert!(!plain.palette.raw_ids.contains_key(&4096));
    let disabled = load_scene_bytes(
        &mut install,
        "thawed.map",
        "fixture".into(),
        opt.snapshot().bytes().unwrap(),
        Instant::now(),
    )
    .unwrap();
    assert!(!disabled.palette.supports_ice);
    assert!(
        !disabled
            .diagnostics
            .iter()
            .any(|d| d.code == "INVALID_TILE_ID")
    );
}

#[test]
fn exact_source_export_preserves_unknown_fields_encoding_and_pack_tail() {
    let bytes = source_bytes();
    let (source, parsed) =
        SourceMap::parse("合成.map".into(), "fixture".into(), bytes.clone()).unwrap();
    assert_eq!(parsed.cells.len(), 1);
    assert_eq!(parsed.cells[0].z, 2);
    // Runtime projection omits the record's ice_growth=7. The source must keep it.
    assert_eq!(source.bytes(), bytes);
    let path = std::env::temp_dir().join(format!(
        "yr-p1-原文-{}-{}.map",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    source.export_new(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let (_, again) = SourceMap::parse(
        "reopen".into(),
        "export".into(),
        std::fs::read(&path).unwrap(),
    )
    .unwrap();
    assert_eq!(again.header.width, 3);
    assert_eq!(again.cells[0].z, 2);
    // Refuse overwrite even if the caller picks the original path.
    assert!(source.export_new(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn bad_pack_and_oversized_dimensions_are_rejected() {
    let text = String::from_utf8_lossy(&source_bytes()).into_owned();
    assert!(
        SourceMap::parse(
            "bad".into(),
            "fixture".into(),
            text.replace("DwALABwCAAIAAAAAAAACBxEAAA==", "bad!")
                .into_bytes()
        )
        .is_err()
    );
    // The updated core admits 512x512 headers using its extended cell grid.
    assert!(
        SourceMap::parse(
            "large".into(),
            "fixture".into(),
            text.replace("Size=0,0,3,3", "Size=0,0,513,512")
                .into_bytes()
        )
        .is_err()
    );
}
fn cell(tile_id: u16, z: u8) -> TerrainCell {
    TerrainCell {
        screen_x: -2.,
        screen_y: -2.,
        tile_id,
        sub_tile: 0,
        z,
        rx: 2,
        ry: 2,
        variant: 0,
        tint: [1.; 3],
        radar_left: [0; 3],
        radar_right: [0; 3],
    }
}
fn scene() -> MapScene {
    let back = cell(0, 0);
    let front = cell(1, 1);
    let image = || TileImage {
        rgba: [100, 150, 200, 255].repeat(16),
        depth: vec![0; 16],
        width: 4,
        height: 4,
        offset_x: 0,
        offset_y: 0,
    };
    let mut fg = image();
    fg.depth[0] = 255;
    fg.rgba[7] = 0;
    let source = source();
    let playfield = PlayfieldBounds::from_map_header(&source.header);
    let mut scene = MapScene {
        resolved: None,
        authored_bridge_decks: Default::default(),
        high_bridge_overlay_ids: Vec::new(),
        revision: Default::default(),
        mission_definitions: Default::default(),
        mission_resources: Default::default(),
        visual: Default::default(),
        source,
        cells: vec![back, front],
        images: std::sync::Arc::new(HashMap::from([
            (
                TileKey {
                    tile_id: 0,
                    sub_tile: 0,
                    variant: 0,
                },
                image(),
            ),
            (
                TileKey {
                    tile_id: 1,
                    sub_tile: 0,
                    variant: 0,
                },
                fg,
            ),
        ])),
        bounds: egui::Rect::from_min_max(egui::pos2(-2., -2.), egui::pos2(2., 2.)),
        diagnostics: vec![],
        elapsed_ms: 0,
        playfield,
        buckets: HashMap::new(),
        catalog: Default::default(),
        brushes: vec![],
        palette: Default::default(),
        sprites: Default::default(),
    };
    scene.index_pixels();
    scene
}
#[test]
fn picking_obeys_depth_alpha_negative_buckets_and_half_open_edges() {
    let scene = scene();
    assert_eq!(
        scene.pick(egui::pos2(-1.5, -1.5)),
        Some(0),
        "TMP Z beats draw order and height"
    );
    assert_eq!(
        scene.pick(egui::pos2(-0.5, -1.5)),
        Some(0),
        "transparent front texel exposes back"
    );
    assert_eq!(
        scene.pick(egui::pos2(0.5, -1.5)),
        Some(1),
        "raised front texel wins"
    );
    assert_eq!(scene.pick(egui::pos2(-2., 0.)), Some(1));
    assert_eq!(scene.pick(egui::pos2(2., 0.)), None);
    assert_eq!(scene.pick(egui::pos2(0., 2.)), None);
    assert_eq!(scene.pick(egui::pos2(-2.01, 0.)), None);
    assert_eq!(scene.pick(egui::pos2(500., 500.)), None);
}
#[test]
fn equal_depth_last_pixel_wins_and_raw_height_is_signed() {
    let mut scene = scene();
    scene.cells[1].z = 0;
    std::sync::Arc::get_mut(&mut scene.images)
        .unwrap()
        .get_mut(&TileKey {
            tile_id: 1,
            sub_tile: 0,
            variant: 0,
        })
        .unwrap()
        .depth
        .fill(0);
    assert_eq!(scene.pick(egui::pos2(0.5, -1.5)), Some(1));
    let signed = cell(0, 255);
    assert_eq!(pixel_depth(&signed, 0), 32755, "raw FF means level -1");
}
#[test]
fn native_fill_missing_tiles_and_missing_theater_keep_source() {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    let mut install = fixture.install();
    let path = fixture.0.join("只读.map");
    std::fs::write(&path, source_bytes()).unwrap();
    let scene = load_scene(&mut install, path.to_str().unwrap()).unwrap();
    assert_eq!(
        scene.cells.len(),
        15,
        "Size(3,3) must materialize (2*w-1)*h cells"
    );
    assert_eq!(scene.source.bytes(), source_bytes());
    assert_eq!(
        scene
            .cells
            .iter()
            .find(|c| (c.rx, c.ry) == (2, 2))
            .unwrap()
            .z,
        2
    );
    assert!(!scene.diagnostics.iter().any(|d| d.code == "MISSING_TILE"));
    assert!(!scene.buckets.is_empty());

    let urban = String::from_utf8_lossy(&source_bytes())
        .replace("TEMPERATE", "URBAN")
        .into_bytes();
    std::fs::write(&path, &urban).unwrap();
    let unsupported = load_scene(&mut install, path.to_str().unwrap()).unwrap();
    assert_eq!(unsupported.source.header.theater, "URBAN");
    assert_eq!(unsupported.source.bytes(), urban);
    assert!(unsupported.cells.is_empty());
    assert!(
        unsupported
            .diagnostics
            .iter()
            .any(|d| d.code == "UNSUPPORTED_THEATER")
    );

    // Remove one distinct resource; valid clear terrain still renders and
    // missing images become actionable diagnostics instead of a crash.
    let ini =
        b"[General]\nClearTile=0\n[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=2\n";
    std::fs::write(fixture.0.join("temperatmd.ini"), ini).unwrap();
    let broken = source_bytes();
    // Cell now references tile 1 (clear02.tem, absent).
    let text = String::from_utf8_lossy(&broken).replace(
        "DwALABwCAAIAAAAAAAACBxEAAA==",
        "DwALABwCAAIAAQAAAAACBxEAAA==",
    );
    std::fs::write(&path, text.as_bytes()).unwrap();
    let missing = load_scene(&mut fixture.install(), path.to_str().unwrap()).unwrap();
    assert!(
        missing
            .diagnostics
            .iter()
            .any(|d| d.code == "MISSING_TILE" && d.cell == Some([2, 2]))
    );
    assert!(!missing.images.is_empty());
}

#[test]
fn entirely_missing_tmp_keeps_header_bytes_and_actionable_diagnostics() {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    std::fs::remove_file(fixture.0.join("clear01.tem")).unwrap();
    let path = fixture.0.join("无资源.map");
    std::fs::write(&path, source_bytes()).unwrap();
    let scene = load_scene(&mut fixture.install(), path.to_str().unwrap()).unwrap();
    assert!(scene.images.is_empty());
    assert_eq!(scene.source.bytes(), source_bytes());
    assert!(
        scene
            .diagnostics
            .iter()
            .any(|d| d.code == "NO_RENDERABLE_TERRAIN")
    );
}
