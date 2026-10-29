//! Synthetic map fixtures for cross-crate regression tests.
use crate::{document::*, raw_ini::pack_lines};
use std::sync::Arc;
use vera20k_core::{map::map_file, util::lzo};

pub fn fixture() -> Vec<u8> {
    let mut pack = Vec::new();
    // Duplicate coordinate: the final record is authoritative; the old one survives.
    for (tile, ice) in [(1i32, 6u8), (2, 7)] {
        pack.extend_from_slice(&2i16.to_le_bytes());
        pack.extend_from_slice(&2u16.to_le_bytes());
        pack.extend_from_slice(&tile.to_le_bytes());
        pack.extend_from_slice(&[0, 2, ice]);
    }
    pack.extend_from_slice(&[0, 0, 0, 0, 201, 202, 203]);
    let mut bytes=b"; old encoding \xD6\xD0\xCE\xC4\r\n[Map]\r\nTheater=TEMPERATE\r\nSize=0,0,3,3\r\nLocalSize=0,0,3,3\r\nLevel=0\r\n[IsoMapPack5]\r\n; packed comment\r\n".to_vec();
    bytes.extend(pack_lines(&lzo::compress_chunks(&pack, 8192), b"\r\n"));
    bytes.extend_from_slice(b"[Infantry]\r\n  0 = Neutral,E1,256,2,2,0,Guard,0,None,0,-1,0,1,1,\xD6\xD0 ; keep\r\n0=Neutral,SHADOW,256,3,3,0,Guard,0,None\r\n[Waypoints]\r\n0=2002\r\n[Unknown]\r\nA=first\r\nA=second\r\n[Digest]\r\n1=stale\r\n[Preview]\r\nSize=0,0,1,1\r\n[PreviewPack]\r\n1=stale\r\n; end");
    bytes
}
pub fn document() -> MapDocument {
    let bytes = fixture();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    MapDocument::new(Baseline::parse(Arc::from(bytes), &map).unwrap())
}
