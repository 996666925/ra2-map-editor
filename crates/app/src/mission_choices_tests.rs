use super::*;
use ra2_map_document::test_support::document;

#[test]
fn candidate_cache_tracks_pending_history_rollback_and_different_documents() {
    let mut doc = document();
    let scene = crate::editing::tests::terrain_scene();
    let ctx = egui::Context::default();
    let scripts =
        |doc: &MapDocument| get(&ctx, ParameterKind::Reference(Kind::Script), doc, &scene);
    let initial = scripts(&doc);
    assert!(Arc::ptr_eq(&initial, &scripts(&doc)));
    let state = doc.state_id;
    doc.set_ini("ScriptTypes", "0", Some(b"SCRIPT".to_vec()))
        .unwrap();
    doc.set_ini("SCRIPT", "0", Some(b"3,0".to_vec())).unwrap();
    assert_eq!(doc.state_id, state);
    let pending = scripts(&doc);
    assert!(pending.iter().any(|(id, _)| id == "SCRIPT"));
    assert!(!Arc::ptr_eq(&initial, &pending));
    assert!(
        Arc::ptr_eq(
            &pending,
            &references(&ctx, Kind::Script, &doc, &scene.mission_definitions)
        ),
        "CSV and parameter controls share candidates"
    );
    let checkpoint = doc.edit_checkpoint();
    doc.set_ini("ScriptTypes", "1", Some(b"OTHER".to_vec()))
        .unwrap();
    assert!(scripts(&doc).iter().any(|(id, _)| id == "OTHER"));
    doc.restore_edit(&checkpoint).unwrap();
    assert!(!scripts(&doc).iter().any(|(id, _)| id == "OTHER"));
    let result: anyhow::Result<()> = doc.atomic(|doc| {
        doc.set_ini("ScriptTypes", "1", Some(b"TEMP".to_vec()))?;
        assert!(scripts(doc).iter().any(|(id, _)| id == "TEMP"));
        anyhow::bail!("rollback")
    });
    assert!(result.is_err());
    assert!(!scripts(&doc).iter().any(|(id, _)| id == "TEMP"));
    doc.commit("script");
    let bytes = doc.snapshot().bytes().unwrap();
    doc.undo();
    assert!(!scripts(&doc).iter().any(|(id, _)| id == "SCRIPT"));
    doc.redo();
    assert!(scripts(&doc).iter().any(|(id, _)| id == "SCRIPT"));
    let other = document();
    assert!(!scripts(&other).iter().any(|(id, _)| id == "SCRIPT"));
    let fork = doc.fork();
    assert!(scripts(&fork).iter().any(|(id, _)| id == "SCRIPT"));
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    doc.set_ini("ScriptTypes", "1", Some(b"CANCEL".to_vec()))
        .unwrap();
    assert!(scripts(&doc).iter().any(|(id, _)| id == "CANCEL"));
    doc.cancel();
    assert!(!scripts(&doc).iter().any(|(id, _)| id == "CANCEL"));
    doc.replace_bytes(ra2_map_document::test_support::fixture(), "replace")
        .unwrap();
    assert!(!scripts(&doc).iter().any(|(id, _)| id == "SCRIPT"));
    doc.undo();
    assert!(scripts(&doc).iter().any(|(id, _)| id == "SCRIPT"));
}

#[test]
fn waypoint_candidates_and_validation_follow_pending_additions_and_deletions() {
    let mut doc = document();
    let scene = crate::editing::tests::terrain_scene();
    let ctx = egui::Context::default();
    let choices =
        |letters, doc: &MapDocument| get(&ctx, ParameterKind::Waypoint { letters }, doc, &scene);
    assert_eq!(choices(false, &doc)[0].0, "0");
    let original = choices(true, &doc);
    assert_eq!(original[0].0, "A");
    doc.waypoint(26, [3, 2]).unwrap();
    assert!(choices(true, &doc).iter().any(|(id, _)| id == "AA"));
    assert!(choices(false, &doc).iter().any(|(id, _)| id == "26"));
    assert!(
        mission::schema::parameter_error(&doc, ParameterKind::Waypoint { letters: true }, "AA")
            .is_none()
    );
    let waypoint = doc
        .record_iter()
        .find(|(_, r)| r.section == "Waypoints" && r.key == "26")
        .unwrap()
        .0;
    doc.set(ra2_map_document::document::Target::Record(waypoint), None)
        .unwrap();
    assert!(!choices(true, &doc).iter().any(|(id, _)| id == "AA"));
    assert!(
        mission::schema::parameter_error(&doc, ParameterKind::Waypoint { letters: true }, "AA")
            .is_some()
    );
    doc.cancel();
    assert_eq!(choices(true, &doc).as_ref(), original.as_ref());
}

#[test]
fn candidate_cache_tracks_language_catalog_and_fixed_definitions() {
    let mut doc = document();
    doc.set_ini("ScriptTypes", "0", Some(b"SCRIPT".to_vec()))
        .unwrap();
    let mut scene = crate::editing::tests::terrain_scene();
    Arc::make_mut(&mut scene.catalog)
        .object_types
        .insert("Units".into(), vec!["HTNK".into()]);
    Arc::make_mut(&mut scene.catalog)
        .object_types
        .insert("Structures".into(), vec!["BUILDING".into()]);
    assert!(
        !get(
            &egui::Context::default(),
            ParameterKind::Type("Foot"),
            &doc,
            &scene
        )
        .iter()
        .any(|(id, _)| id == "BUILDING")
    );
    let ctx = egui::Context::default();
    let _chinese = i18n::enter(i18n::Language::Chinese);
    let types = get(&ctx, ParameterKind::Type(""), &doc, &scene);
    let refs = get(&ctx, ParameterKind::Reference(Kind::Script), &doc, &scene);
    assert!(refs.iter().any(|(_, label)| label == "SCRIPT · 地图"));
    let countries = get(&ctx, ParameterKind::Country, &doc, &scene);
    doc.set_ini("EditorGlobalNames", "0", Some(b"first".to_vec()))
        .unwrap();
    assert!(
        Arc::ptr_eq(&types, &get(&ctx, ParameterKind::Type(""), &doc, &scene)),
        "static choices survive document edits"
    );
    assert_eq!(
        get(&ctx, ParameterKind::Reference(Kind::Global), &doc, &scene)[0].1,
        "0 · first"
    );
    doc.set_ini("EditorGlobalNames", "0", Some(b"second".to_vec()))
        .unwrap();
    assert_eq!(
        get(&ctx, ParameterKind::Reference(Kind::Global), &doc, &scene)[0].1,
        "0 · second"
    );
    {
        let _english = i18n::enter(i18n::Language::English);
        let translated = get(&ctx, ParameterKind::Reference(Kind::Script), &doc, &scene);
        assert!(translated.iter().any(|(_, label)| label == "SCRIPT · Map"));
        assert_ne!(
            countries.as_ref(),
            get(&ctx, ParameterKind::Country, &doc, &scene).as_ref()
        );
    }
    assert!(
        get(&ctx, ParameterKind::Reference(Kind::Script), &doc, &scene)
            .iter()
            .any(|(_, label)| label == "SCRIPT · 地图")
    );
    Arc::make_mut(&mut scene.catalog)
        .object_types
        .get_mut("Units")
        .unwrap()
        .push("MOD".into());
    assert!(
        get(&ctx, ParameterKind::Type(""), &doc, &scene)
            .iter()
            .any(|(id, _)| id == "MOD")
    );
    let mut definitions = (*scene.mission_definitions).clone();
    definitions.scripts.insert("EXTERNAL".into());
    definitions.source = "fixed.ini".into();
    scene.mission_definitions = Arc::new(definitions);
    assert!(
        get(&ctx, ParameterKind::Reference(Kind::Script), &doc, &scene)
            .iter()
            .any(|(id, label)| id == "EXTERNAL" && label == "EXTERNAL · fixed.ini")
    );
    let separate = egui::Context::default();
    assert!(!Arc::ptr_eq(
        &get(&separate, ParameterKind::Type(""), &doc, &scene),
        &get(&ctx, ParameterKind::Type(""), &doc, &scene)
    ));
}

#[test]
#[ignore = "opt-in synthetic CPU performance audit; writes target/step5-caches.json"]
fn synthetic_candidate_and_occupancy_profile() {
    use ra2_map_document::{document::Baseline, new_map::NewMapOptions};
    use ra2_map_scene::placement;
    use std::{collections::BTreeSet, hint::black_box, time::Instant};
    let mut reports = Vec::new();
    for count in [1000, 3000] {
        let mut scene = crate::editing::tests::terrain_scene();
        Arc::make_mut(&mut scene.catalog).object_types.insert(
            "Units".into(),
            (0..count).map(|index| format!("TYPE{index}")).collect(),
        );
        let mut bytes = NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap();
        bytes.extend_from_slice(b"\r\n[Units]\r\n");
        for index in 0..count {
            bytes.extend_from_slice(
                format!("{index}=Neutral,HTNK,256,20,20,0,Guard,None,0,-1,0,-1,1,1\r\n").as_bytes(),
            );
        }
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
        let proposed = doc
            .record_iter()
            .find(|(_, record)| record.section == "Units")
            .unwrap()
            .1
            .moved([22, 20])
            .unwrap();
        let ignored = BTreeSet::new();
        let terrain = BTreeMap::new();
        let ctx = egui::Context::default();
        let occupancy = placement::Occupancy::new(&doc, &scene, &ignored);
        black_box(get(&ctx, ParameterKind::Type(""), &doc, &scene));
        let measure = |mut operation: Box<dyn FnMut() + '_>| {
            let mut samples = Vec::new();
            for _ in 0..7 {
                let started = Instant::now();
                for _ in 0..20 {
                    operation();
                }
                samples.push(started.elapsed().as_secs_f64() * 1000. / 20.);
            }
            samples.sort_by(f64::total_cmp);
            samples[3]
        };
        let candidates_rebuilt_ms = measure(Box::new(|| {
            black_box(Cache::default().get(
                Family::Type(""),
                &doc,
                &scene.catalog,
                &scene.mission_definitions,
            ));
        }));
        let candidates_cached_ms = measure(Box::new(|| {
            black_box(get(&ctx, ParameterKind::Type(""), &doc, &scene));
        }));
        let occupancy_rebuilt_ms = measure(Box::new(|| {
            placement::validate(
                &doc,
                &scene,
                std::slice::from_ref(&proposed),
                &ignored,
                false,
                &terrain,
            )
            .unwrap();
        }));
        let occupancy_cached_ms = measure(Box::new(|| {
            occupancy
                .validate(
                    &doc,
                    &scene,
                    std::slice::from_ref(&proposed),
                    false,
                    &terrain,
                )
                .unwrap();
        }));
        reports.push(serde_json::json!({"records_and_candidates": count, "candidates_rebuilt_ms": candidates_rebuilt_ms, "candidates_cached_ms": candidates_cached_ms, "occupancy_rebuilt_ms": occupancy_rebuilt_ms, "occupancy_cached_ms": occupancy_cached_ms}));
    }
    let report = serde_json::json!({"scope": "synthetic CPU candidate lookup and single-placement validation; setup and document writes excluded", "samples": 7, "operations_per_sample": 20, "reports": reports});
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/step5-caches.json");
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{report}");
}
