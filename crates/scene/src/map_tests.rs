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
