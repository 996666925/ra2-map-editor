//! Editor-only layout travels inside the playable composite, including underlays.
use super::*;
use crate::raw_ini::{RawIni, base64, pack_lines};
use anyhow::Context;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub(super) const SECTION: &str = "YRMapEditorLayersPack";
const MAX_LAYOUT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Layout<C = Clipboard> {
    version: u32,
    checksum: u64,
    baseline: String,
    shift: [i32; 2],
    pieces: Vec<SavedPiece<C>>,
}
#[derive(Serialize, Deserialize)]
struct SavedPiece<C> {
    name: String,
    theater: String,
    clip: C,
    offset: [i32; 2],
    extent: [[i32; 2]; 2],
}

pub(super) fn without_layout(raw: &RawIni) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in &raw.lines {
        if line.section != SECTION {
            bytes.extend_from_slice(&raw.bytes[line.bytes.clone()]);
        }
    }
    bytes
}

// Publication regenerates previews and may add empty lines. Everything authored
// in the actual composite remains part of this consistency check.
fn checksum(raw: &RawIni) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for line in &raw.lines {
        if matches!(
            line.section.as_str(),
            SECTION | "Preview" | "PreviewPack" | "Digest"
        ) {
            continue;
        }
        let bytes = raw.bytes[line.bytes.clone()].trim_ascii_end();
        if bytes.is_empty() {
            continue;
        }
        for byte in bytes.iter().chain(std::iter::once(&b'\n')) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

impl Workspace {
    pub(super) fn persist(&self, bytes: Vec<u8>, shift: [i32; 2]) -> Result<Vec<u8>> {
        let raw = RawIni::parse(bytes.into())?;
        let layout = Layout {
            version: 1,
            checksum: checksum(&raw),
            baseline: base64(&without_layout(&RawIni::parse(
                self.baseline.snapshot().bytes()?.into(),
            )?)),
            shift,
            pieces: self
                .pieces
                .iter()
                .map(|p| SavedPiece {
                    name: p.name.clone(),
                    theater: p.theater.clone(),
                    clip: p.clip.as_ref(),
                    offset: p.offset,
                    extent: p.extent,
                })
                .collect(),
        };
        let json = serde_json::to_vec(&layout)?;
        ensure!(
            json.len() <= MAX_LAYOUT_BYTES,
            "独立地图图层数据超过大小限制"
        );
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&json)?;
        let mut output = without_layout(&raw);
        output.extend_from_slice(b"\r\n[YRMapEditorLayersPack]\r\n");
        output.extend_from_slice(&pack_lines(&encoder.finish()?, b"\r\n"));
        ensure!(
            output.len() <= crate::source::MAX_SOURCE_BYTES,
            "保存多地图后的文件超过地图大小限制"
        );
        Ok(output)
    }

    /// None means ordinary edits changed the composite since this layout was
    /// applied. Do not replace those edits with an older independent underlay.
    pub fn restore(doc: &MapDocument) -> Result<Option<(Self, [i32; 2])>> {
        let raw = RawIni::parse(doc.snapshot().bytes()?.into())?;
        if raw.entries(SECTION).next().is_none() {
            return Ok(None);
        }
        let packed = raw.packed(SECTION).context("读取独立地图图层失败")?;
        let mut json = Vec::new();
        ZlibDecoder::new(packed.as_slice())
            .take(MAX_LAYOUT_BYTES as u64 + 1)
            .read_to_end(&mut json)?;
        ensure!(
            json.len() <= MAX_LAYOUT_BYTES,
            "独立地图图层数据超过大小限制"
        );
        let layout: Layout = serde_json::from_slice(&json).context("独立地图图层数据无效")?;
        ensure!(layout.version == 1, "不支持的多地图布局版本");
        if layout.checksum != checksum(&raw) {
            return Ok(None);
        }
        ensure!(!layout.pieces.is_empty(), "独立地图图层列表为空");
        let baseline = vera20k_core::util::base64::base64_decode(&layout.baseline)
            .map_err(anyhow::Error::msg)?;
        let (source, _) = crate::source::SourceMap::parse_with_ares(
            "layout.yrm".into(),
            "layout".into(),
            baseline,
            doc.base.reads_ares,
        )?;
        let mut baseline = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
        baseline.resource_profile = doc.resource_profile.clone();
        let mut pieces = Vec::new();
        for saved in layout.pieces {
            let [[u0, v0], [u1, v1]] = saved.extent;
            let width = (i64::from(u1) - i64::from(u0)) / 2;
            let height = (i64::from(v1) - i64::from(v0)) / 2;
            ensure!(
                (1..MAX_MAP_SPAN as i64).contains(&width)
                    && (1..MAX_MAP_SPAN as i64).contains(&height)
                    && width + height <= MAX_MAP_SPAN as i64
                    && saved.extent
                        == [
                            [1 - width as i32, width as i32 + 1],
                            [width as i32 + 1, width as i32 + 2 * height as i32 + 1]
                        ],
                "独立地图尺寸无效"
            );
            let mut clip = saved.clip;
            clip.resource_profile = doc.resource_profile.clone();
            ensure!(
                clip.theater.eq_ignore_ascii_case(&doc.base.header.theater),
                "独立地图目标环境无效"
            );
            let cells: BTreeSet<_> = clip.cells.iter().map(|c| c.at).collect();
            ensure!(
                cells.len() == ((2 * width - 1) * height) as usize
                    && cells.len() == clip.cells.len()
                    && cells.iter().all(|at| {
                        let x = i64::from(at[0]);
                        let y = i64::from(at[1]);
                        x < 1024
                            && y < 1024
                            && (1 - width..width).contains(&(x - y))
                            && (width + 1..=width + 2 * height).contains(&(x + y))
                    }),
                "独立地图格网无效"
            );
            ensure!(
                clip.records
                    .iter()
                    .all(|(_, r)| r.cell().is_some_and(|at| cells.contains(&at))),
                "独立地图对象坐标无效"
            );
            let max_level = clip
                .cells
                .iter()
                .filter_map(|c| c.terrain.map(|v| v.level))
                .max()
                .unwrap_or(0);
            pieces.push(Piece {
                name: saved.name,
                theater: saved.theater,
                clip: Arc::new(clip),
                offset: saved.offset,
                max_level,
                extent: saved.extent,
            });
        }
        let workspace = Self {
            baseline,
            identity: (doc.id, doc.state_id, doc.edit_revision()),
            pieces,
        };
        ensure!(
            workspace.geometry()?.1 == layout.shift,
            "独立地图坐标偏移无效"
        );
        Ok(Some((workspace, layout.shift)))
    }
}
