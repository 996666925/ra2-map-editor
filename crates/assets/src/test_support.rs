use super::assets::*;
use std::path::PathBuf;

pub struct Fixture(pub PathBuf);

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "yr-editor-资源-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let mut iso = vec![0u8; 768];
        iso[..6].copy_from_slice(&[1, 2, 3, 3, 5, 7]);
        let mut unit = vec![0u8; 768];
        unit[3..6].copy_from_slice(&[15, 20, 25]);
        std::fs::write(path.join("isotem.pal"), &iso).unwrap();
        std::fs::write(path.join("unittem.pal"), &unit).unwrap();
        std::fs::write(path.join("temperat.pal"), &iso).unwrap();
        std::fs::write(
            path.join("temperatmd.ini"),
            b"[General]\nClearTile=0\n[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=1\n",
        )
        .unwrap();

        // A sparse-free 1x1 TMP template, with a 4x2 diamond (four pixels
        // in the first row, no coverage in the second), no extra/depth plane.
        let mut tmp = Vec::new();
        for value in [1u32, 1, 4, 2, 20] {
            tmp.extend(value.to_le_bytes());
        }
        tmp.resize(20 + 52, 0);
        tmp.extend([0, 1, 1, 0]);
        std::fs::write(path.join("clear01.tem"), tmp).unwrap();

        // One cropped 2x1 SHP frame at (1,2) in a declared 4x4 canvas.
        let mut shp = Vec::new();
        for value in [0u16, 4, 4, 1, 1, 2, 2, 1] {
            shp.extend(value.to_le_bytes());
        }
        shp.resize(28, 0);
        shp.extend(32u32.to_le_bytes());
        shp.extend([1, 0]);
        std::fs::write(path.join("gi.shp"), shp).unwrap();
        Self(path)
    }

    pub fn install(&self) -> Install {
        Install::from_probe_root(&self.0).unwrap()
    }

    /// Small TMP images with stock CliffSet topology for authoring regressions.
    pub fn with_cliffs() -> Self {
        let fixture = Self::new();
        std::fs::write(fixture.0.join("temperatmd.ini"),b"[General]\nClearTile=0\nCliffSet=1\n[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=1\nMorphable=yes\n[TileSet0001]\nSetName=Cliff Set\nFileName=cliff\nTilesInSet=40\n").unwrap();
        for piece in 1..=40 {
            let (width, cells): (u32, Vec<Option<u8>>) = match piece {
                1 | 3 | 4 => (2, vec![Some(4), None, Some(0), Some(4), None, Some(0)]),
                2 | 8 => (1, vec![Some(4), Some(0)]),
                5..=7 => (2, vec![Some(4), Some(4), Some(0), Some(0)]),
                9..=11 => (2, vec![Some(4), Some(0), Some(0), None]),
                12..=14 => (1, vec![Some(0)]),
                15..=17 => (2, vec![Some(4), Some(0), Some(4), Some(0)]),
                18 | 22 => (2, vec![Some(4), Some(0)]),
                19..=21 => (3, vec![Some(4), Some(0), None, None, Some(4), Some(0)]),
                23..=25 => (2, vec![Some(4), Some(4)]),
                27 | 31 => (2, vec![Some(4), Some(4), Some(4), None]),
                28 | 32 => (2, vec![None, Some(4), Some(4), Some(4)]),
                35..=37 => (1, vec![Some(4), Some(4)]),
                _ => (1, vec![Some(4)]),
            };
            let mut bytes = Vec::new();
            for n in [width, cells.len() as u32 / width, 4, 2] {
                bytes.extend(n.to_le_bytes());
            }
            bytes.resize(16 + cells.len() * 4, 0);
            for (sub, cell) in cells.iter().enumerate() {
                let Some(height) = cell else {
                    continue;
                };
                let offset = bytes.len();
                bytes[16 + sub * 4..20 + sub * 4].copy_from_slice(&(offset as u32).to_le_bytes());
                bytes.resize(offset + 52, 0);
                bytes[offset + 40] = *height;
                bytes.extend([0, 1, 1, 0]);
            }
            std::fs::write(fixture.0.join(format!("cliff{piece:02}.tem")), bytes).unwrap();
        }
        fixture
    }

    /// Shore/road geometry and water cliff counterparts with tiny test images.
    pub fn with_joining() -> Self {
        let f = Self::with_authoring();
        let ini=std::fs::read_to_string(f.0.join("temperatmd.ini")).unwrap().replace("ClearTile=0", "ClearTile=0\nWaterSet=5\nShorePieces=6\nPavedRoads=7\nPavedRoadEnds=8\nWaterCliffs=9");
        std::fs::write(f.0.join("temperatmd.ini"),format!("{ini}\n[TileSet0005]\nFileName=water\nTilesInSet=14\n[TileSet0006]\nFileName=shore\nTilesInSet=40\n[TileSet0007]\nFileName=road\nTilesInSet=15\n[TileSet0008]\nFileName=end\nTilesInSet=4\n[TileSet0009]\nFileName=wcliff\nTilesInSet=22\n")).unwrap();
        for n in 1..=14 {
            write_surface(
                &f.0.join(format!("water{n:02}.tem")),
                if n <= 8 { 2 } else { 1 },
                &vec![9; if n <= 8 { 4 } else { 1 }],
            );
        }
        for n in 0..40 {
            let (w, pattern): (u32, Vec<u8>) = match n {
                0..=2 => (2, vec![14, 14, 10, 10]),
                3 => (1, vec![14, 10]),
                4 => (2, vec![14, 14, 10, 14, 10, 10]),
                5 => (2, vec![14, 14, 14, 10, 10, 10]),
                6..=7 => (2, vec![14, 10, 10, 10]),
                8..=10 => (2, vec![14, 10, 14, 10]),
                11 => (2, vec![14, 10]),
                12 => (3, vec![14, 14, 10, 14, 10, 10]),
                13 => (3, vec![14, 10, 10, 14, 14, 10]),
                14..=15 => (2, vec![10, 10, 14, 10]),
                16..=18 => (2, vec![10, 10, 14, 14]),
                19 => (1, vec![10, 14]),
                20 => (2, vec![10, 10, 10, 14, 14, 14]),
                21 => (2, vec![10, 10, 14, 10, 14, 14]),
                22..=23 => (2, vec![10, 10, 10, 14]),
                24..=26 => (2, vec![10, 14, 10, 14]),
                27 => (2, vec![10, 14]),
                28 => (3, vec![10, 14, 14, 10, 10, 14]),
                29 => (3, vec![10, 10, 14, 10, 14, 14]),
                30..=31 => (2, vec![10, 14, 10, 10]),
                32..=33 => (2, vec![14, 14, 14, 10]),
                34..=35 => (2, vec![14, 10, 14, 14]),
                36..=37 => (2, vec![10, 14, 14, 14]),
                _ => (2, vec![14, 14, 10, 14]),
            };
            write_surface(&f.0.join(format!("shore{:02}.tem", n + 1)), w, &pattern);
        }
        for n in 0..15 {
            let (w, h) = match n {
                0 => (1, 3),
                1 => (3, 1),
                9..=11 => (4, 3),
                12..=14 => (3, 4),
                _ => (3, 3),
            };
            write_surface(
                &f.0.join(format!("road{:02}.tem", n + 1)),
                w,
                &vec![11; (w * h) as usize],
            );
        }
        for n in 0..4 {
            write_surface(
                &f.0.join(format!("end{:02}.tem", n + 1)),
                if n % 2 == 0 { 1 } else { 3 },
                &[14; 3],
            );
        }
        for n in 1..=22 {
            std::fs::copy(
                f.0.join(format!("cliff{n:02}.tem")),
                f.0.join(format!("wcliff{n:02}.tem")),
            )
            .unwrap();
        }
        f
    }

    /// Stock straight ramp ends and bridgehead geometry with tiny test images.
    pub fn with_authoring() -> Self {
        let f = Self::with_cliffs();
        std::fs::write(f.0.join("temperatmd.ini"),b"[General]\nClearTile=0\nCliffSet=1\nCliffRamps=2\nSlopeSetPieces=2\nBridgeSet=3\nWoodBridgeSet=4\nBridgeTopLeft1=1\nBridgeTopLeft2=2\nBridgeBottomRight1=3\nBridgeBottomRight2=3\nBridgeTopRight1=4\nBridgeTopRight2=5\nBridgeBottomLeft1=6\nBridgeBottomLeft2=6\nBridgeMiddle1=7\nBridgeMiddle2=12\n[TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=1\nMorphable=yes\n[TileSet0001]\nSetName=Cliff Set\nFileName=cliff\nTilesInSet=40\n[TileSet0002]\nSetName=Cliff Ramps\nFileName=ramp\nTilesInSet=10\n[TileSet0003]\nSetName=Bridge\nFileName=bridge\nTilesInSet=16\n[TileSet0004]\nSetName=Wood Bridge\nFileName=wood\nTilesInSet=16\n").unwrap();
        type RampCells<'a> = (u32, &'a [Option<(u8, u8)>]);
        let ramps: &[RampCells<'_>] = &[
            (
                3,
                &[
                    None,
                    Some((3, 11)),
                    Some((3, 4)),
                    Some((4, 0)),
                    Some((3, 3)),
                    Some((2, 4)),
                    Some((0, 0)),
                    Some((0, 1)),
                    Some((1, 4)),
                    None,
                    Some((0, 8)),
                    Some((0, 4)),
                ],
            ),
            (
                3,
                &[
                    Some((3, 4)),
                    Some((3, 12)),
                    None,
                    Some((2, 4)),
                    Some((3, 1)),
                    Some((4, 0)),
                    Some((1, 4)),
                    Some((0, 3)),
                    Some((0, 0)),
                    Some((0, 4)),
                    Some((0, 7)),
                    None,
                ],
            ),
            (
                4,
                &[
                    Some((3, 3)),
                    Some((2, 3)),
                    Some((1, 3)),
                    Some((0, 3)),
                    Some((3, 10)),
                    Some((3, 2)),
                    Some((0, 4)),
                    Some((0, 7)),
                    None,
                    Some((4, 0)),
                    Some((0, 0)),
                    None,
                ],
            ),
            (
                4,
                &[
                    None,
                    Some((4, 0)),
                    Some((0, 0)),
                    None,
                    Some((3, 11)),
                    Some((3, 4)),
                    Some((0, 2)),
                    Some((0, 6)),
                    Some((3, 3)),
                    Some((2, 3)),
                    Some((1, 3)),
                    Some((0, 3)),
                ],
            ),
            (
                3,
                &[
                    None,
                    Some((0, 5)),
                    Some((0, 2)),
                    Some((0, 0)),
                    Some((0, 0)),
                    Some((1, 2)),
                    Some((4, 0)),
                    Some((3, 3)),
                    Some((2, 2)),
                    None,
                    Some((3, 10)),
                    Some((3, 2)),
                ],
            ),
            (
                3,
                &[
                    Some((0, 2)),
                    Some((0, 0)),
                    None,
                    Some((1, 2)),
                    Some((0, 3)),
                    Some((0, 0)),
                    Some((2, 2)),
                    None,
                    None,
                    Some((3, 2)),
                    None,
                    None,
                ],
            ),
            (2, &[Some((3, 1)), Some((4, 0)), Some((3, 9)), None]),
            (
                4,
                &[
                    None,
                    Some((0, 0)),
                    Some((4, 0)),
                    None,
                    Some((0, 5)),
                    Some((0, 0)),
                    Some((3, 4)),
                    Some((3, 12)),
                    Some((0, 1)),
                    Some((1, 1)),
                    Some((2, 1)),
                    Some((3, 1)),
                ],
            ),
            (
                4,
                &[
                    Some((0, 1)),
                    Some((1, 1)),
                    Some((2, 1)),
                    Some((3, 1)),
                    Some((0, 8)),
                    Some((0, 0)),
                    None,
                    None,
                    None,
                    Some((0, 0)),
                    None,
                    None,
                ],
            ),
            (2, &[Some((3, 2)), Some((3, 9)), Some((4, 0)), None]),
        ];
        for (i, (w, cells)) in ramps.iter().enumerate() {
            write_tmp(&f.0.join(format!("ramp{:02}.tem", i + 1)), *w, cells);
        }
        for prefix in ["bridge", "wood"] {
            for i in 0..16 {
                let (w, heights): (u32, Vec<u8>) = match i {
                    0 | 1 => (3, vec![4, 0, 0, 4, 4, 0, 4, 4, 0, 4, 4, 0, 4, 0, 0]),
                    2 => (2, vec![0, 4, 4, 4, 4, 4, 4, 4, 0, 4]),
                    3 | 4 => (5, vec![4, 4, 4, 4, 4, 0, 4, 4, 4, 0, 0, 0, 0, 0, 0]),
                    5 => (5, vec![0, 4, 4, 4, 0, 4, 4, 4, 4, 4]),
                    6..=10 => (2, vec![0, 0, 4, 0, 4, 0, 4, 0, 0, 0]),
                    _ => (5, vec![0, 4, 4, 4, 0, 0, 0, 0, 0, 0]),
                };
                write_tmp(
                    &f.0.join(format!("{prefix}{:02}.tem", i + 1)),
                    w,
                    &heights
                        .into_iter()
                        .map(|h| Some((h, 0)))
                        .collect::<Vec<_>>(),
                );
            }
        }
        let mut rules = String::from("[OverlayTypes]\n");
        for id in 0..255 {
            let name = match id {
                24 => "BRIDGE1".into(),
                25 => "BRIDGE2".into(),
                237 => "BRIDGEB1".into(),
                238 => "BRIDGEB2".into(),
                74..=101 => format!("LOBRDG{:02}", id - 73),
                205..=232 => format!("LOBRDB{:02}", id - 204),
                _ => format!("OV{id}"),
            };
            rules.push_str(&format!("{id}={name}\n"));
            if matches!(id,24|25|237|238|74..=101|205..=232) {
                write_shp(
                    &f.0.join(format!("{name}.tem")),
                    if matches!(id, 24 | 25 | 237 | 238) {
                        18
                    } else {
                        6
                    },
                );
            }
        }
        std::fs::write(f.0.join("rulesmd.ini"), rules).unwrap();
        f
    }
}

fn write_tmp(path: &std::path::Path, width: u32, cells: &[Option<(u8, u8)>]) {
    let mut bytes = Vec::new();
    for n in [width, cells.len() as u32 / width, 4, 2] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.resize(16 + cells.len() * 4, 0);
    for (sub, c) in cells.iter().enumerate() {
        if let Some((height, slope)) = c {
            let offset = bytes.len();
            bytes[16 + sub * 4..20 + sub * 4].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes.resize(offset + 52, 0);
            bytes[offset + 40] = *height;
            bytes[offset + 42] = *slope;
            bytes.extend([0, 1, 1, 0]);
        }
    }
    std::fs::write(path, bytes).unwrap();
}
fn write_surface(path: &std::path::Path, width: u32, terrain: &[u8]) {
    write_tmp(path, width, &vec![Some((0, 0)); terrain.len()]);
    let mut b = std::fs::read(path).unwrap();
    for (sub, &terrain) in terrain.iter().enumerate() {
        let offset = u32::from_le_bytes(b[16 + sub * 4..20 + sub * 4].try_into().unwrap()) as usize;
        b[offset + 41] = terrain;
    }
    std::fs::write(path, b).unwrap();
}
fn write_shp(path: &std::path::Path, count: u16) {
    let mut bytes = Vec::new();
    for n in [0u16, 4, 4, count] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.resize(8 + usize::from(count) * 24, 0);
    for frame in 0..usize::from(count) {
        let start = 8 + frame * 24;
        for (n, v) in [1u16, 2, 2, 1].into_iter().enumerate() {
            bytes[start + n * 2..start + n * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let offset = bytes.len() as u32;
        bytes[start + 20..start + 24].copy_from_slice(&offset.to_le_bytes());
        bytes.extend([1, 1]);
    }
    std::fs::write(path, bytes).unwrap();
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
