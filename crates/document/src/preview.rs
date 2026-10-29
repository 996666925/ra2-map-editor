//! Regenerate engine-readable RGB/LZO previews without touching authored sections.
use crate::raw_ini::{RawIni, pack_lines};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, sync::Arc};
pub fn embed(bytes: &[u8], width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        width > 0
            && height > 0
            && width <= 2048
            && height <= 2048
            && rgba.len() == (width * height * 4) as usize,
        "地图预览尺寸或像素数无效"
    );
    let raw = RawIni::parse(Arc::from(bytes))?;
    let mut out = raw.patch(&BTreeMap::new(), &BTreeMap::new(), &BTreeMap::new(), true);
    let nl = &raw.newline;
    if !out.ends_with(b"\n") {
        out.extend_from_slice(nl)
    }
    out.extend_from_slice(b"[Preview]");
    out.extend_from_slice(nl);
    out.extend_from_slice(format!("Size=0,0,{width},{height}").as_bytes());
    out.extend_from_slice(nl);
    out.extend_from_slice(b"[PreviewPack]");
    out.extend_from_slice(nl);
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| p[..3].iter().copied())
        .collect();
    out.extend_from_slice(&pack_lines(
        &vera20k_core::util::lzo::compress_chunks(&rgb, 8192),
        nl,
    ));
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_roundtrip_replaces_old_preview_and_retains_opaque_map_bytes() {
        let source=b"[Unknown]\r\nopaque=\xd6\xd0; comment\r\n[Preview]\r\nSize=0,0,9,9\r\n[PreviewPack]\r\n1=old\r\n[Digest]\r\n1=obsolete\r\n";
        let rgba = [1, 2, 3, 255, 4, 5, 6, 255];
        let bytes = embed(source, 2, 1, &rgba).unwrap();
        assert!(bytes.starts_with(b"[Unknown]\r\nopaque=\xd6\xd0; comment\r\n"));
        let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&bytes).unwrap();
        let decoded = vera20k_core::map::preview::decode_preview_image_from_ini(&ini)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.rgba, rgba);
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert!(ini.section("Digest").is_none());
        assert!(embed(source, 0, 1, &[]).is_err());
    }
}
