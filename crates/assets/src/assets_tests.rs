use super::*;
use crate::test_support::Fixture;

// Tiny uncompressed frames with independent crops on a shared 4x4 canvas.
fn write_shp_frames(fixture: &Fixture, name: &str, frames: &[([u16; 2], [u8; 2])]) {
    let mut shp = Vec::new();
    for value in [0u16, 4, 4, frames.len() as u16] {
        shp.extend(value.to_le_bytes());
    }
    for (index, (at, _)) in frames.iter().enumerate() {
        for value in [at[0], at[1], 2, 1] {
            shp.extend(value.to_le_bytes());
        }
        shp.extend([0; 12]);
        shp.extend((8 + frames.len() as u32 * 24 + index as u32 * 2).to_le_bytes());
    }
    for (_, pixels) in frames {
        shp.extend(pixels);
    }
    std::fs::write(fixture.0.join(name), shp).unwrap();
}

#[test]
fn building_art_anchor_stays_on_north_west_cell_for_all_foundations() {
    let fixture = Fixture::new();
    let mut rules = String::from("[BuildingTypes]\n");
    let mut art = String::new();
    for (index, foundation) in ["1x1", "3x2", "2x3", "3x3"].iter().enumerate() {
        let id = format!("BUILD{index}");
        rules.push_str(&format!("{index}={id}\n"));
        art.push_str(&format!(
            "[{id}]\nFoundation={foundation}\nXDrawOffset=3\nYDrawOffset=4\n"
        ));
        write_shp_frames(
            &fixture,
            &format!("build{index}.shp"),
            &[([1, 2], [1, 0]), ([2, 3], [1, 0])],
        );
    }
    std::fs::write(fixture.0.join("rulesmd.ini"), rules).unwrap();
    std::fs::write(fixture.0.join("artmd.ini"), art).unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let sprites = crate::sprites::SpritePalette::load(&install, &catalog);
    for index in 0..4 {
        let building = &sprites.objects[&("Structures".into(), format!("BUILD{index}"))];
        assert_eq!(
            building.anchor,
            [3., -11.],
            "foundation must not translate art"
        );
        assert_eq!(building.image(0).info.offset, [-1, 0]);
        assert_eq!(building.shadow.as_ref().unwrap().info.offset, [0, 1]);
    }
}

#[test]
fn building_shadow_uses_companion_crop_and_nonzero_indices_and_respects_no_shadow() {
    let fixture = Fixture::new();
    let frames = [
        ([1, 2], [1, 0]), // Healthy body.
        ([1, 2], [1, 1]), // Damaged body.
        ([5, 6], [2, 0]), // Healthy shadow; palette index 2 is black.
        ([7, 8], [0, 1]), // Damaged shadow must not be selected.
    ];
    write_shp_frames(&fixture, "gi.shp", &frames);
    write_shp_frames(&fixture, "empty.shp", &[frames[0], ([5, 6], [0, 0])]);
    write_shp_frames(&fixture, "single.shp", &frames[..1]);
    write_shp_frames(&fixture, "broken.shp", &frames[..2]);
    let mut broken = std::fs::read(fixture.0.join("broken.shp")).unwrap();
    broken.truncate(broken.len() - 2); // Body readable, companion payload missing.
    std::fs::write(fixture.0.join("broken.shp"), broken).unwrap();
    std::fs::write(fixture.0.join("rulesmd.ini"), b"[BuildingTypes]\n0=GI\n1=DISABLED\n2=EMPTY\n3=SINGLE\n4=BROKEN\n[DISABLED]\nImage=GI\nNoShadow=yes\n").unwrap();
    std::fs::write(
        fixture.0.join("artmd.ini"),
        b"[GI]\nXDrawOffset=3\nYDrawOffset=4\n",
    )
    .unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let sprites = crate::sprites::SpritePalette::load(&install, &catalog);
    let building = &sprites.objects[&("Structures".into(), "GI".into())];
    assert_eq!(building.anchor, [3., -11.]);
    let body = building.image(0);
    assert_eq!(body.info.offset, [-1, 0]);
    assert_eq!(body.rgba, [60, 80, 100, 255, 0, 0, 0, 0]);
    let shadow = building.shadow.as_ref().unwrap();
    assert_eq!(shadow.info.index, 2);
    assert_eq!(shadow.info.offset, [3, 4]);
    assert_eq!((shadow.info.width, shadow.info.height), (2, 1));
    assert_eq!(shadow.rgba, [0, 0, 0, 128, 0, 0, 0, 0]);
    for id in ["DISABLED", "EMPTY", "SINGLE", "BROKEN"] {
        let object = &sprites.objects[&("Structures".into(), id.into())];
        assert!(object.shadow.is_none(), "{id} must not fabricate a shadow");
        assert_eq!(object.image(0).rgba[3], 255, "{id} body remains visible");
    }
}

#[test]
fn building_bib_and_opted_in_animation_shadows_keep_layer_offsets_without_double_darkening() {
    let fixture = Fixture::new();
    let frames = [([1, 2], [1, 0]), ([2, 2], [2, 0])];
    for name in ["gi.shp", "bib.shp", "glow.shp", "plain.shp", "flat.shp"] {
        write_shp_frames(&fixture, name, &frames);
    }
    std::fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[BuildingTypes]\n0=GI\n[Animations]\n0=GLOW\n1=PLAIN\n2=FLAT\n",
    )
    .unwrap();
    std::fs::write(fixture.0.join("artmd.ini"), b"[GI]\nBibShape=BIB\nActiveAnim=GLOW\nActiveAnimX=3\nActiveAnimY=2\nIdleAnim=PLAIN\nIdleAnimX=20\nSuperAnim=FLAT\nSuperAnimX=30\n[GLOW]\nShadow=yes\nXDrawOffset=2\nYDrawOffset=-7\n[PLAIN]\nShadow=no\n[FLAT]\nShadow=yes\nFlat=yes\n").unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let sprites = crate::sprites::SpritePalette::load(&install, &catalog);
    let building = &sprites.objects[&("Structures".into(), "GI".into())];
    let shadow = building.shadow.as_ref().unwrap();
    assert_eq!(shadow.info.offset, [0, 0]);
    assert_eq!((shadow.info.width, shadow.info.height), (7, 3));
    let pixel = |x: usize, y: usize| &shadow.rgba[(y * 7 + x) * 4..(y * 7 + x + 1) * 4];
    assert_eq!(pixel(0, 0), [0, 0, 0, 128], "body and bib overlap once");
    assert_eq!(
        pixel(5, 2),
        [0, 0, 0, 128],
        "animation uses its own crop and slot/X offset"
    );
    assert_eq!(
        shadow
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count(),
        2
    );
}

#[test]
fn map_shp_uses_canvas_offsets_and_native_infantry_direction_without_thumbnail_scaling() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("rulesmd.ini"), b"[InfantryTypes]\n0=GI\n").unwrap();
    std::fs::write(
        fixture.0.join("artmd.ini"),
        b"[GI]\nSequence=ReadySeq\nXDrawOffset=3\nYDrawOffset=4\n[ReadySeq]\nReady=0,1,1\n",
    )
    .unwrap();
    // Eight differently indexed cropped frames on a 4x4 logical canvas.
    let mut shp = Vec::new();
    for v in [0u16, 4, 4, 8] {
        shp.extend(v.to_le_bytes());
    }
    for frame in 0..8u32 {
        for v in [1u16, 2, 2, 1] {
            shp.extend(v.to_le_bytes());
        }
        shp.extend([0; 12]);
        shp.extend((8 + 8 * 24 + frame * 2).to_le_bytes());
    }
    for _ in 0..8 {
        shp.extend([1, 0]);
    }
    std::fs::write(fixture.0.join("gi.shp"), shp).unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let sprites = crate::sprites::SpritePalette::load(&install, &catalog);
    let object = &sprites.objects[&("Infantry".into(), "GI".into())];
    assert_eq!(object.anchor, [3., 4.]);
    for (facing, frame) in [(0, 7), (64, 5), (128, 3), (192, 1), (224, 0), (236, 7)] {
        let image = object.image(facing);
        assert_eq!(image.info.index, frame);
        assert_eq!(image.info.offset, [-1, 0]);
        assert_eq!((image.info.width, image.info.height), (2, 1));
        assert_eq!(image.rgba[7], 0);
    }
    assert!(std::sync::Arc::ptr_eq(object.image(0), object.image(8)));
}

#[test]
fn map_building_attached_art_uses_parent_palette_and_tree_uses_theater_palette() {
    let fixture = Fixture::new();
    std::fs::copy(fixture.0.join("gi.shp"), fixture.0.join("glow.shp")).unwrap();
    std::fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[BuildingTypes]\n0=GI\n[TerrainTypes]\n0=TREE\n[TREE]\nImage=GI\n[Animations]\n0=GLOW\n",
    )
    .unwrap();
    std::fs::write(
        fixture.0.join("artmd.ini"),
        b"[GI]\nActiveAnim=GLOW\n[GLOW]\nLoopStart=0\nLoopEnd=1\n",
    )
    .unwrap();
    let mut anim = vec![0; 768];
    anim[3..6].copy_from_slice(&[63, 0, 63]);
    std::fs::write(fixture.0.join("anim.pal"), anim).unwrap();
    let mut terrain = vec![0; 768];
    terrain[3..6].copy_from_slice(&[1, 2, 30]);
    std::fs::write(fixture.0.join("isotem.pal"), terrain).unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let sprites = crate::sprites::SpritePalette::load(&install, &catalog);
    let building = &sprites.objects[&("Structures".into(), "GI".into())];
    assert_eq!(&building.image(0).rgba[..4], &[60, 80, 100, 255]);
    assert_eq!(building.anchor, [0., -15.]);
    assert_eq!(
        &sprites
            .get("Animations", "GLOW")
            .unwrap()
            .image
            .as_ref()
            .unwrap()
            .rgba[..4],
        &[252, 0, 252, 255]
    );
    let tree = &sprites.objects[&("Terrain".into(), "TREE".into())];
    assert_eq!(&tree.image(0).rgba[..4], &[4, 8, 120, 255]);
}

#[test]
fn library_includes_variants_environment_objects_and_map_local_types() {
    let fixture = Fixture::new();
    std::fs::copy(
        fixture.0.join("clear01.tem"),
        fixture.0.join("clear01a.tem"),
    )
    .unwrap();
    std::fs::write(fixture.0.join("rulesmd.ini"), b"[BuildingTypes]\n0=BASE\n[InfantryTypes]\n0=GI\n[TerrainTypes]\n0=TREE\n[SmudgeTypes]\n0=CR1\n[OverlayTypes]\n0=WALL\n1=TIB01\n[Animations]\n0=ANIM\n[TREE]\nImage=GI\n[CR1]\nImage=GI\n[WALL]\nImage=GI\nWall=yes\n").unwrap();
    let install = fixture.install();
    let catalog = crate::catalog::Catalog::load(&install.manager, &install.theater);
    let palette = catalog.terrain_palette(&install.manager, &install.theater);
    assert!(palette.brushes.iter().any(|b| b.key.variant == 0));
    assert!(palette.brushes.iter().any(|b| b.key.variant == 1));
    let patched = catalog
        .with_map(b"[BuildingTypes]\n0=CUSTOM\n[CUSTOM]\nImage=GI\n[OverlayTypes]\n0=NEWOVERLAY\n");
    assert_eq!(patched.object_types["Structures"], ["BASE", "CUSTOM"]);
    assert_eq!(patched.overlays, ["WALL", "TIB01", "NEWOVERLAY"]);
    assert_eq!(patched.ore, Some(1));
    let sprites = crate::sprites::SpritePalette::load(&install, &patched);
    let image = sprites
        .get("Infantry", "GI")
        .unwrap()
        .image
        .as_ref()
        .unwrap();
    assert_eq!(image.rgba, [60, 80, 100, 255, 0, 0, 0, 0]);
    assert_eq!(image.info.offset, [-1, 0]);
    assert!(sprites.get("Terrain", "TREE").unwrap().image.is_some());
    assert!(sprites.get("Smudge", "CR1").unwrap().image.is_some());
    assert!(sprites.get("Overlay", "WALL").unwrap().image.is_some());
    assert!(sprites.get("Structures", "BASE").unwrap().image.is_none());
    assert!(sprites.get("Structures", "CUSTOM").unwrap().image.is_some());
    assert_eq!(
        &sprites.overlay_frames[&("WALL".into(), 0)].rgba[..4],
        &[60, 80, 100, 255]
    );
    assert!(std::sync::Arc::ptr_eq(
        &catalog,
        &catalog.with_map(b"[Map]\nTheater=TEMPERATE\n")
    ));
}

#[test]
fn adapter_keeps_palette_transparency_and_crop_semantics() {
    let fixture = Fixture::new();
    let install = fixture.install();
    let decoded = install.decode(&AssetSelection::default()).unwrap();
    assert_eq!(&decoded.tile.rgba[..8], &[4, 8, 12, 255, 12, 20, 28, 255]);
    assert_eq!(
        decoded.tile.rgba[4 * 4 + 3],
        0,
        "outside TMP diamond stays transparent"
    );
    assert_eq!(decoded.sprite.rgba, [60, 80, 100, 255, 0, 0, 0, 0]);
    assert_eq!(decoded.sprite.info.offset, [1, 2]);
    assert_eq!(
        (decoded.sprite.info.width, decoded.sprite.info.height),
        (2, 1)
    );
    assert_eq!(decoded.tile.info.palette, "isotem.pal");
    assert_eq!(decoded.sprite.info.palette, "unittem.pal");
    assert!(!decoded.tile.info.source.is_empty());
    assert!(!decoded.sprite.info.palette_source.is_empty());

    let mut selection = AssetSelection {
        frame: 1,
        ..Default::default()
    };
    assert!(
        install
            .decode(&selection)
            .err()
            .unwrap()
            .to_string()
            .contains("越界")
    );
    selection.frame = 0;
    selection.sub_tile = 1;
    assert!(
        install
            .decode(&selection)
            .err()
            .unwrap()
            .to_string()
            .contains("不存在或为空")
    );
}

#[test]
fn loader_returns_missing_directory_error_with_request_identity() {
    let fixture = Fixture::new();
    let loader = AssetLoader::new(egui::Context::default()).unwrap();
    loader
        .request(
            42,
            AssetSelection {
                directory: fixture.0.join("missing"),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let response = loader
        .results
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(response.generation, 42);
    assert!(
        response
            .result
            .err()
            .unwrap()
            .to_string()
            .contains("游戏目录不存在")
    );
}
