use crate::editor_io::*;
use ra2_map_document::test_support::{document, fixture};
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "yr-p2-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}
#[test]
fn atomic_save_replaces_on_windows_backs_up_original_and_rejects_external_changes() {
    let root = root();
    let path = root.join("中文地图.map");
    let original = fixture();
    let stamp = replace_map(&path, &original, None).unwrap();
    let mut doc = document();
    doc.waypoint(1, [3, 2]).unwrap();
    doc.commit("start");
    let edited = doc.snapshot().bytes().unwrap();
    let new = replace_map(&path, &edited, Some(stamp)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), edited);
    assert_ne!(new, stamp);
    let backups: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "bak"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path()).unwrap(), original);
    fs::write(&path, b"external edit").unwrap();
    assert!(replace_map(&path, &edited, Some(new)).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"external edit");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn failed_validation_preserves_original_and_complete_temp() {
    let root = root();
    let path = root.join("original.map");
    let original = fixture();
    let stamp = replace_map(&path, &original, None).unwrap();
    assert!(replace_map(&path, b"[Map]\nSize=0,0,512,512\n", Some(stamp)).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(
        fs::read_dir(&root)
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|e| e == "tmp"))
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn recovery_roundtrip_is_independent_and_truncation_is_rejected() {
    let root = root();
    let path = root.join("recovery.yrrec");
    let original = root.join("original.map");
    let bytes = fixture();
    fs::write(&original, &bytes).unwrap();
    let meta = RecoveryMeta {
        version: 1,
        name: "中文地图.map".into(),
        directory: root.clone(),
        target: Some(original.clone()),
        expected: Some(Stamp::bytes(&bytes)),
        resource_profile: Some("schema:resources".into()),
    };
    write_recovery(&path, &meta, &bytes).unwrap();
    let (restored, again) = read_recovery(&path).unwrap();
    assert_eq!(restored.target, meta.target);
    assert_eq!(restored.resource_profile, meta.resource_profile);
    assert_eq!(again, bytes);
    assert_eq!(fs::read(&original).unwrap(), bytes);
    let mut bad = fs::read(&path).unwrap();
    bad.truncate(14);
    fs::write(&path, bad).unwrap();
    assert!(read_recovery(&path).is_err());
    fs::remove_dir_all(root).unwrap();
}
