use super::*;
#[test]
fn csf_overrides_preserve_base_records_and_unicode_and_reject_truncation() {
    let mut base = Vec::new();
    for word in [0x43534620u32, 3, 1, 1, 0, 9, 0x4c424c20, 1, 4] {
        base.extend(word.to_le_bytes());
    }
    base.extend(b"BASE");
    for word in [0x53545257u32, 1] {
        base.extend(word.to_le_bytes());
    }
    base.extend((!65u16).to_le_bytes());
    base.extend(3u32.to_le_bytes());
    base.extend(b"xyz");
    let strings = BTreeMap::from([
        ("BASE".into(), "改写😀".into()),
        ("NEW".into(), "新增\n第二行".into()),
    ]);
    let out = merge_csf(&base, &strings).unwrap();
    assert_eq!(&out[24..base.len()], &base[24..]);
    let table = CsfFile::from_bytes(&out).unwrap();
    assert_eq!(table.language, 9);
    assert_eq!(table.get("base"), Some("改写😀"));
    assert_eq!(table.get("NEW"), Some("新增\n第二行"));
    assert!(merge_csf(&base[..base.len() - 1], &strings).is_err());
}
#[test]
fn ini_metadata_preserves_unrelated_bytes_and_refuses_duplicate_target() {
    let original = b";old\r\n[OTHER]\r\nName=\xD6\xD0\r\n[A.MAP]\r\nUIName=OLD ; note\r\n";
    let out = patch_ini(original, &[("A.MAP".into(), "UIName".into(), "NEW".into())]).unwrap();
    assert!(out.starts_with(b";old\r\n[OTHER]\r\nName=\xD6\xD0\r\n"));
    assert!(out.ends_with(b"UIName=NEW ; note\r\n"));
    assert!(patch_ini(b"[A]\nx=1\nx=2\n", &[("A".into(), "x".into(), "3".into())]).is_err());
}
#[test]
fn export_refuses_missing_dependencies_or_existing_destination() {
    let p = std::env::temp_dir().join(format!("yr-campaign-refuse-{}", std::process::id()));
    let report = Report {
        passed: false,
        entry: "A.MAP".into(),
        shell_campaign: "all1".into(),
        dependencies: vec![],
        warnings: vec![],
        files: vec![],
    };
    assert!(
        export(
            Prepared {
                payload: BTreeMap::new(),
                report
            },
            &p
        )
        .is_err()
    );
    assert!(!p.exists());
    let report = Report {
        passed: true,
        entry: "A.MAP".into(),
        shell_campaign: "all1".into(),
        dependencies: vec![],
        warnings: vec![],
        files: vec![],
    };
    assert!(
        export(
            Prepared {
                payload: BTreeMap::new(),
                report
            },
            &std::env::temp_dir()
        )
        .is_err()
    );
}
