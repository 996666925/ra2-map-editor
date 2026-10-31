//! Structural map edits: plan against a snapshot, then replace it as one history step.
use crate::{
    document::{Cell, MapDocument},
    mission::{self, Kind},
    new_map::{MAX_MAP_SPAN, NewMapOptions},
    raw_ini::{RawIni, lcw_encode, pack_lines},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use vera20k_core::{
    map::{
        cell_index::CellGrid,
        map_file,
        playfield::{PlayfieldBounds, size_diamond_contains},
    },
    util::{lcw, lzo},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub width: u16,
    pub height: u16,
    /// Placement of the old map's projected left/top in the new map.
    pub offset: [i32; 2],
    pub local: [i32; 4],
    pub fill_level: u8,
    pub delete_outside: bool,
}
impl Options {
    pub fn from_document(doc: &MapDocument) -> Self {
        let h = &doc.base.header;
        Self {
            width: h.width as u16,
            height: h.height as u16,
            offset: [0, 0],
            local: [
                h.local_left as i32,
                h.local_top as i32,
                h.local_width as i32,
                h.local_height as i32,
            ],
            fill_level: h.level as u8,
            delete_outside: false,
        }
    }
    pub fn validate(&self) -> Result<()> {
        NewMapOptions {
            width: self.width,
            height: self.height,
            level: self.fill_level,
            ..Default::default()
        }
        .validate()?;
        ensure!(
            self.offset
                .iter()
                .all(|v| (-i32::from(MAX_MAP_SPAN)..=i32::from(MAX_MAP_SPAN)).contains(v)),
            "平移偏移超出范围"
        );
        let [l, t, w, h] = self.local;
        ensure!(
            l >= 2
                && t >= 2
                && w > 0
                && h > 0
                && l + w <= i32::from(self.width) - 2
                && t + h <= i32::from(self.height) - 6,
            "LocalSize 须位于引擎有效边距内：左/上至少 2，右至少 2，下至少 6"
        );
        Ok(())
    }
    pub fn delta(&self, old_width: u32) -> [i32; 2] {
        [
            self.offset[0] + self.offset[1],
            i32::from(self.width) - old_width as i32 + self.offset[1] - self.offset[0],
        ]
    }
    pub fn translated(&self, at: Cell, delta: [i32; 2]) -> Option<Cell> {
        let x = i32::from(at[0]) + delta[0];
        let y = i32::from(at[1]) + delta[1];
        let side = i32::from(CellGrid::for_map_size(self.width.into(), self.height.into()).side());
        ((0..side).contains(&x)
            && (0..side).contains(&y)
            && size_diamond_contains(
                i32::from(self.width),
                i32::from(self.height),
                (x as i16, y as i16),
            ))
        .then_some([x as u16, y as u16])
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Affected {
    pub source: String,
    pub cell: Cell,
    pub blocking: bool,
}
#[derive(Default, Clone, Debug, Serialize)]
pub struct Summary {
    pub terrain_removed: usize,
    pub terrain_added: usize,
    pub overlays_removed: usize,
    pub records_moved: usize,
    pub references_moved: usize,
    pub affected: Vec<Affected>,
}
pub type Footprints = BTreeMap<String, Vec<(i16, i16)>>;
pub struct Plan {
    pub summary: Summary,
    pub bytes: Vec<u8>,
    pub options: Options,
    identity: (u64, u64),
    source: Vec<u8>,
}
impl Plan {
    pub fn can_apply(&self) -> bool {
        self.summary
            .affected
            .iter()
            .all(|a| !a.blocking && self.options.delete_outside)
    }
    pub fn apply(&self, doc: &mut MapDocument) -> Result<bool> {
        ensure!(self.can_apply(), "越界项尚未处理或未选择删除越界内容");
        ensure!(
            self.identity == (doc.id, doc.state_id) && self.source == doc.snapshot().bytes()?,
            "地图已改变，请重新预览迁移"
        );
        doc.replace_bytes(self.bytes.clone(), "地图尺寸与可玩区域")
    }
}
struct Patch<'a> {
    raw: &'a RawIni,
    lines: BTreeMap<usize, Option<Vec<u8>>>,
    append: BTreeMap<String, Vec<(String, Vec<u8>)>>,
}
impl Patch<'_> {
    fn set(&mut self, section: &str, key: &str, value: Option<Vec<u8>>) -> Result<()> {
        let rows: Vec<_> = self
            .raw
            .entries(section)
            .filter(|(_, l)| l.key.as_deref() == Some(key))
            .collect();
        ensure!(rows.len() <= 1, "重复空间字段 {section}/{key}，请先消歧");
        if let Some((line, _)) = rows.first() {
            self.lines.insert(*line, value);
        } else if let Some(value) = value {
            self.append
                .entry(section.into())
                .or_default()
                .push((key.into(), value));
        }
        Ok(())
    }
}
fn number(bytes: &[u8]) -> Result<i32> {
    Ok(std::str::from_utf8(bytes)?.trim().parse()?)
}
fn patch_columns(bytes: &[u8], edits: &[(usize, i32)]) -> Result<Vec<u8>> {
    let mut f: Vec<_> = bytes.split(|b| *b == b',').map(<[u8]>::to_vec).collect();
    for (i, v) in edits {
        ensure!(*i < f.len(), "空间记录字段不足");
        f[*i] = v.to_string().into_bytes();
    }
    Ok(f.join(&b','))
}
fn lost(summary: &mut Summary, source: String, cell: Cell, blocking: bool) {
    summary.affected.push(Affected {
        source,
        cell,
        blocking,
    });
}
/// Footprints come from the active installation's ART foundation registry.
pub fn plan(doc: &MapDocument, options: Options, footprints: &Footprints) -> Result<Plan> {
    options.validate()?;
    let source = doc.snapshot().bytes()?;
    let map = map_file::load_from_bytes(source.clone())?;
    let raw = RawIni::parse(source.clone().into())?;
    let scratch = MapDocument::new(crate::document::Baseline::parse(
        source.clone().into(),
        &map,
    )?);
    let delta = options.delta(map.header.width);
    let structural = options.width as u32 != map.header.width
        || options.height as u32 != map.header.height
        || delta != [0, 0];
    let mut patch = Patch {
        raw: &raw,
        lines: BTreeMap::new(),
        append: BTreeMap::new(),
    };
    let mut summary = Summary::default();
    let mut packs = BTreeMap::new();
    if options.width as u32 != map.header.width || options.height as u32 != map.header.height {
        patch.set(
            "Map",
            "Size",
            Some(format!("0,0,{},{}", options.width, options.height).into_bytes()),
        )?;
    }
    patch.set(
        "Map",
        "LocalSize",
        Some(options.local.map(|n| n.to_string()).join(",").into_bytes()),
    )?;
    if structural {
        let old_side = usize::from(map.header.cell_grid().side());
        let new_side =
            usize::from(CellGrid::for_map_size(options.width.into(), options.height.into()).side());
        // A repeated section has CRC-reader semantics that a migration cannot infer.
        let spatial_sections: BTreeSet<_> = [
            "Map",
            "Units",
            "Infantry",
            "Structures",
            "Aircraft",
            "Terrain",
            "Smudge",
            "Waypoints",
            "CellTags",
            "Tubes",
            "Actions",
            "ScriptTypes",
            "Houses",
        ]
        .into_iter()
        .chain(
            mission::ids(&scratch, Kind::Script)
                .iter()
                .map(String::as_str),
        )
        .chain(
            mission::ids(&scratch, Kind::House)
                .iter()
                .map(String::as_str),
        )
        .map(str::to_owned)
        .collect();
        let mut sections = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for line in &raw.lines {
            if !spatial_sections.contains(&line.section) {
                continue;
            }
            if raw.bytes[line.bytes.clone()]
                .trim_ascii_start()
                .starts_with(b"[")
            {
                ensure!(
                    sections.insert(line.section.clone()),
                    "重复空间 section {}",
                    line.section
                );
            }
            if let Some(key) = &line.key {
                ensure!(
                    keys.insert((line.section.clone(), key.clone())),
                    "重复空间字段 {}/{key}",
                    line.section
                );
            }
        }
        let terrain =
            lzo::decompress_chunks(&raw.packed("IsoMapPack5")?, old_side * old_side * 11 + 4)?;
        let mut values = BTreeMap::new();
        let mut cursor = 0;
        while cursor + 4 <= terrain.len() {
            let x = i16::from_le_bytes(terrain[cursor..cursor + 2].try_into()?);
            let y = u16::from_le_bytes(terrain[cursor + 2..cursor + 4].try_into()?);
            if x == 0 && y == 0 {
                break;
            }
            ensure!(cursor + 11 <= terrain.len(), "地形 Pack 截断");
            ensure!(
                (0..old_side as i16).contains(&x) && usize::from(y) < old_side,
                "Pack 含网格别名坐标，请先消歧"
            );
            let at = [x as u16, y];
            if let Some(next) = options.translated(at, delta) {
                ensure!(
                    values
                        .insert(next, terrain[cursor + 4..cursor + 11].to_vec())
                        .is_none(),
                    "地形 Pack 含重复格子"
                );
            } else {
                summary.terrain_removed += 1;
            }
            cursor += 11;
        }
        let mut encoded = Vec::new();
        for sum in
            u32::from(options.width) + 1..=u32::from(options.width) + 2 * u32::from(options.height)
        {
            for x in 0..=sum {
                let y = sum - x;
                if x.abs_diff(y) >= u32::from(options.width) {
                    continue;
                }
                let at = [x as u16, y as u16];
                encoded.extend_from_slice(&at[0].to_le_bytes());
                encoded.extend_from_slice(&at[1].to_le_bytes());
                if let Some(value) = values.remove(&at) {
                    encoded.extend(value);
                } else {
                    encoded.extend(0i32.to_le_bytes());
                    encoded.extend([0, options.fill_level, 0]);
                    summary.terrain_added += 1;
                }
            }
        }
        encoded.extend([0; 4]);
        packs.insert(
            "IsoMapPack5".into(),
            pack_lines(&lzo::compress_chunks(&encoded, 8192), &raw.newline),
        );
        let decode = |section: &str, default: u8| -> Result<Vec<u8>> {
            let data = raw.packed(section)?;
            let mut bytes = if data.is_empty() {
                vec![]
            } else {
                lcw::decompress_chunks(&data, old_side * old_side)?
            };
            bytes.resize(old_side * old_side, default);
            Ok(bytes)
        };
        let overlays = decode("OverlayPack", 255)?;
        let frames = decode("OverlayDataPack", 0)?;
        let mut target = vec![255; new_side * new_side];
        let mut data = vec![0; new_side * new_side];
        for (i, &id) in overlays.iter().enumerate().filter(|(_, id)| **id != 255) {
            let at = [(i % old_side) as u16, (i / old_side) as u16];
            if let Some(next) = options.translated(at, delta) {
                let n = usize::from(next[1]) * new_side + usize::from(next[0]);
                target[n] = id;
                data[n] = frames[i];
            } else {
                summary.overlays_removed += 1;
                lost(
                    &mut summary,
                    format!("Overlay {id}"),
                    at,
                    vera20k_core::rules::overlay_types::is_high_bridge_index(id),
                );
            }
        }
        for (section, bytes) in [("OverlayPack", target), ("OverlayDataPack", data)] {
            packs.insert(
                section.into(),
                pack_lines(&lcw_encode(&bytes), &raw.newline),
            );
        }
        let mut removed_units = BTreeSet::new();
        let mut moved = Vec::new();
        for record in scratch.records().values() {
            let at = record
                .cell()
                .with_context(|| format!("{}/{} 坐标无效", record.section, record.key))?;
            let next = options.translated(at, delta);
            let fits = next.is_some_and(|at| {
                record.section != "Structures"
                    || footprints
                        .get(&record.type_id())
                        .into_iter()
                        .flatten()
                        .all(|&(x, y)| {
                            let x = i32::from(at[0]) + i32::from(x);
                            let y = i32::from(at[1]) + i32::from(y);
                            (0..new_side as i32).contains(&x)
                                && (0..new_side as i32).contains(&y)
                                && size_diamond_contains(
                                    i32::from(options.width),
                                    i32::from(options.height),
                                    (x as i16, y as i16),
                                )
                        })
            });
            if !fits {
                lost(
                    &mut summary,
                    format!("{}/{}", record.section, record.key),
                    at,
                    record.section == "Waypoints",
                );
                if record.section == "Units" {
                    removed_units.insert(record.key.clone());
                }
                patch.set(&record.section, &record.key, None)?;
            } else {
                moved.push(record.moved(next.unwrap())?);
                summary.records_moved += 1;
            }
        }
        // Remove every old tree key before adding translated keys, including overlaps.
        for (line, _) in raw.entries("Terrain") {
            patch.lines.insert(line, None);
        }
        for mut record in moved {
            if record.section == "Units"
                && record
                    .fields()
                    .get(11)
                    .is_some_and(|s| removed_units.contains(s))
            {
                record = record.patched(&[(11, "-1".into())])?;
            }
            if record.section == "Terrain" {
                patch
                    .append
                    .entry(record.section.clone())
                    .or_default()
                    .push((record.key, record.value));
            } else {
                patch.set(&record.section, &record.key, Some(record.value))?;
            }
        }
        let radix = mission::cell_stride(&scratch);
        let mut tags = BTreeMap::new();
        for (_, line) in raw.entries("CellTags") {
            let key = line.key.as_ref().unwrap();
            let n = key.parse::<u32>().context("CellTags 坐标无效")?;
            let at = [(n % radix) as u16, (n / radix) as u16];
            if let Some(next) = options.translated(at, delta) {
                ensure!(u32::from(next[0]) < radix, "旧 CellTag 横坐标超过 127");
                let key = (u32::from(next[1]) * radix + u32::from(next[0])).to_string();
                ensure!(
                    tags.insert(key, raw.bytes[line.value.clone()].to_vec())
                        .is_none(),
                    "CellTag 格子别名冲突"
                );
                summary.references_moved += 1;
            } else {
                lost(&mut summary, format!("CellTags/{key}"), at, false);
            }
        }
        for (line, _) in raw.entries("CellTags") {
            patch.lines.insert(line, None);
        }
        patch
            .append
            .insert("CellTags".into(), tags.into_iter().collect());
        for house in mission::ids(&scratch, Kind::House) {
            let nodes = crate::base_nodes::read(&scratch, &house)?;
            let mut retained = Vec::new();
            for node in &nodes {
                if node.cell == [0, 0] || matches!(node.building.as_str(), "-2" | "-3") {
                    retained.push(node.clone());
                    continue;
                }
                if let Some(next) = options.translated(node.cell, delta) {
                    retained.push(node.moved(next)?);
                    summary.references_moved += 1;
                } else {
                    lost(
                        &mut summary,
                        format!("{house}/基地节点 {}", node.building),
                        node.cell,
                        false,
                    );
                }
            }
            for i in 0..nodes.len() {
                patch.set(&house, &format!("{i:03}"), None)?;
            }
            for (i, node) in retained.iter().enumerate() {
                patch.set(&house, &format!("{i:03}"), Some(node.raw.clone()))?;
            }
            if !nodes.is_empty() {
                patch.set(
                    &house,
                    "NodeCount",
                    Some(retained.len().to_string().into_bytes()),
                )?;
            }
        }
        for (line, row) in raw.entries("Tubes") {
            let bytes = raw.value(line);
            let f: Vec<_> = bytes.split(|b| *b == b',').collect();
            ensure!(f.len() >= 6, "Tubes 记录字段不足");
            let mut points = vec![
                [u16::try_from(number(f[0])?)?, u16::try_from(number(f[1])?)?],
                [u16::try_from(number(f[3])?)?, u16::try_from(number(f[4])?)?],
            ];
            let mut at = points[0];
            let mut terminated = false;
            for step in f.iter().skip(5).take(100) {
                let n = number(step)?;
                if n == -1 {
                    terminated = true;
                    break;
                }
                ensure!((0..8).contains(&n), "Tubes 路径方向无效");
                let (dx, dy) = [
                    (0, -1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                    (0, 1),
                    (-1, 1),
                    (-1, 0),
                    (-1, -1),
                ][n as usize];
                at = [
                    u16::try_from(i32::from(at[0]) + dx)?,
                    u16::try_from(i32::from(at[1]) + dy)?,
                ];
                points.push(at);
            }
            ensure!(terminated, "Tubes 路径须以 -1 结束");
            if points
                .iter()
                .all(|&at| options.translated(at, delta).is_some())
            {
                let entry = options.translated(points[0], delta).unwrap();
                let exit = options.translated(points[1], delta).unwrap();
                patch.lines.insert(
                    line,
                    Some(patch_columns(
                        bytes,
                        &[
                            (0, i32::from(entry[0])),
                            (1, i32::from(entry[1])),
                            (3, i32::from(exit[0])),
                            (4, i32::from(exit[1])),
                        ],
                    )?),
                );
                summary.references_moved += 1;
            } else {
                patch.lines.insert(line, None);
                lost(
                    &mut summary,
                    format!("Tubes/{}", row.key.as_ref().unwrap()),
                    points[0],
                    false,
                );
            }
        }
        for script in mission::ids(&scratch, Kind::Script) {
            for (line, row) in raw
                .entries(&script)
                .filter(|(_, l)| l.key.as_ref().is_some_and(|k| k.parse::<u32>().is_ok()))
            {
                let f: Vec<_> = raw.value(line).split(|b| *b == b',').collect();
                ensure!(f.len() == 2, "脚本 {script} 格式不明确");
                if number(f[0])? != 4 {
                    continue;
                }
                let n = u32::try_from(number(f[1])?)?;
                let at = [(n % 128) as u16, (n / 128) as u16];
                if let Some(next) = options.translated(at, delta).filter(|a| a[0] < 128) {
                    patch.lines.insert(
                        line,
                        Some(patch_columns(
                            raw.value(line),
                            &[(1, i32::from(next[1]) * 128 + i32::from(next[0]))],
                        )?),
                    );
                    summary.references_moved += 1;
                } else {
                    lost(
                        &mut summary,
                        format!("{script}/{}：脚本 4 旧 128 坐标", row.key.as_ref().unwrap()),
                        at,
                        true,
                    );
                }
            }
        }
        for (line, row) in raw.entries("Actions") {
            let fields: Vec<_> = raw.value(line).split(|b| *b == b',').collect();
            let count = usize::try_from(number(fields[0])?)?;
            ensure!(
                count <= 40
                    && fields.len() == 1 + 8 * count
                    && fields.iter().all(|f| !f.trim_ascii().is_empty()),
                "Actions 列表截断或含歧义字段"
            );
            let mut edits = Vec::new();
            let mut changed = false;
            for (index, action) in fields[1..].as_chunks::<8>().0.iter().enumerate() {
                number(action[1])?;
                if number(action[0])? != 40 {
                    continue;
                }
                let mut rect = [0; 4];
                for i in 0..4 {
                    rect[i] = number(action[i + 3])?;
                }
                rect[0] += options.offset[0];
                rect[1] += options.offset[1];
                if rect[0] < 0
                    || rect[1] < 0
                    || rect[2] <= 0
                    || rect[3] <= 0
                    || rect[0] + rect[2] > i32::from(options.width)
                    || rect[1] + rect[3] > i32::from(options.height)
                {
                    lost(
                        &mut summary,
                        format!("Actions/{}：动作 40 可视矩形", row.key.as_ref().unwrap()),
                        [0, 0],
                        true,
                    );
                }
                for (i, value) in rect.iter().enumerate() {
                    edits.push((1 + index * 8 + i + 3, *value));
                }
                changed = true;
                summary.references_moved += 1;
            }
            if changed {
                patch
                    .lines
                    .insert(line, Some(patch_columns(raw.value(line), &edits)?));
            }
        }
    }
    let unchanged = !structural && options.local == Options::from_document(doc).local;
    let bytes = if unchanged {
        source.clone()
    } else {
        raw.patch(&patch.lines, &packs, &patch.append, true)
    };
    map_file::load_from_bytes(bytes.clone()).context("迁移后地图未通过生产解析")?;
    Ok(Plan {
        summary,
        bytes,
        options,
        identity: (doc.id, doc.state_id),
        source,
    })
}

/// Effective native clipping, shown alongside the raw LocalSize form.
pub fn bounds(doc: &MapDocument) -> PlayfieldBounds {
    PlayfieldBounds::from_map_header(&doc.base.header)
}

#[cfg(test)]
#[path = "map_settings_tests.rs"]
mod tests;
