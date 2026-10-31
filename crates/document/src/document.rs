//! Authoring state and sparse, reversible transactions. No simulation state is serialized.
use crate::raw_ini::{RawIni, lcw_encode, pack_lines};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use vera20k_core::{
    map::map_file::{MapFile, MapHeader},
    util::{lcw, lzo},
};

pub type Cell = [u16; 2];
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainValue {
    pub tile: i32,
    pub sub: u8,
    pub level: u8,
    pub ice: u8,
}
impl TerrainValue {
    fn bytes(self) -> [u8; 7] {
        let mut out = [0; 7];
        out[..4].copy_from_slice(&self.tile.to_le_bytes());
        out[4] = self.sub;
        out[5] = self.level;
        out[6] = self.ice;
        out
    }
    fn read(data: &[u8]) -> Self {
        Self {
            tile: i32::from_le_bytes(data[..4].try_into().unwrap()),
            sub: data[4],
            level: data[5],
            ice: data[6],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub section: String,
    pub key: String,
    pub value: Vec<u8>,
    #[serde(default = "default_radix")]
    pub coordinate_radix: u16,
}
fn default_radix() -> u16 {
    1000
}
impl Record {
    /// Patch only the requested CSV columns; opaque/old-encoded tails survive.
    pub fn patched(&self, edits: &[(usize, String)]) -> Result<Self> {
        let mut fields: Vec<Vec<u8>> = self
            .value
            .split(|b| *b == b',')
            .map(<[u8]>::to_vec)
            .collect();
        for (index, value) in edits {
            ensure!(*index < fields.len(), "记录缺少第 {} 个字段", index + 1);
            ensure!(
                !value.is_empty()
                    && value.is_ascii()
                    && !value
                        .bytes()
                        .any(|b| matches!(b, b',' | b';' | b'\r' | b'\n' | 0)),
                "字段须为非空 ASCII 值，不能含逗号、分号或换行"
            );
            fields[*index] = value.as_bytes().to_vec();
        }
        Ok(Self {
            value: fields.join(&b','),
            ..self.clone()
        })
    }
    pub fn fields(&self) -> Vec<String> {
        self.value
            .split(|b| *b == b',')
            .map(|p| String::from_utf8_lossy(p).trim().to_owned())
            .collect()
    }
    pub fn cell(&self) -> Option<Cell> {
        let f = self.fields();
        if self.section == "Terrain" {
            let n = self.key.parse::<u32>().ok()?;
            let radix = u32::from(self.coordinate_radix);
            if radix == 0 {
                return None;
            }
            return Some([(n % radix) as u16, (n / radix) as u16]);
        }
        if self.section == "Smudge" {
            return Some([f.get(1)?.parse().ok()?, f.get(2)?.parse().ok()?]);
        }
        if self.section == "Waypoints" {
            let n = f.first()?.parse::<u32>().ok()?;
            return Some([(n % 1000) as u16, (n / 1000) as u16]);
        }
        Some([f.get(3)?.parse().ok()?, f.get(4)?.parse().ok()?])
    }
    pub fn type_id(&self) -> String {
        if self.section == "Waypoints" {
            format!("WP {}", self.key)
        } else if matches!(self.section.as_str(), "Terrain" | "Smudge") {
            self.fields().first().cloned().unwrap_or_default()
        } else {
            self.fields().get(1).cloned().unwrap_or_default()
        }
    }
    pub fn moved(&self, cell: Cell) -> Result<Self> {
        let mut record = self.clone();
        if self.section == "Terrain" {
            ensure!(
                cell[0] < self.coordinate_radix,
                "地形对象横坐标须小于 {}",
                self.coordinate_radix
            );
            record.key = (u32::from(cell[1]) * u32::from(self.coordinate_radix)
                + u32::from(cell[0]))
            .to_string();
        } else if self.section == "Waypoints" {
            ensure!(cell[0] < 1000, "航点横坐标须小于 1000");
            record.value =
                format!("{}", u32::from(cell[1]) * 1000 + u32::from(cell[0])).into_bytes();
        } else {
            let mut fields: Vec<_> = self
                .value
                .split(|b| *b == b',')
                .map(<[u8]>::to_vec)
                .collect();
            let offset = if self.section == "Smudge" { 1 } else { 3 };
            ensure!(fields.len() > offset + 1, "对象记录缺少坐标字段");
            fields[offset] = cell[0].to_string().into_bytes();
            fields[offset + 1] = cell[1].to_string().into_bytes();
            record.value = fields.join(&b',');
        }
        Ok(record)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    Terrain(Cell),
    Overlay(Cell),
    Record(u64),
    IniLine(usize),
    IniNew(String, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    Terrain(TerrainValue),
    Overlay(u8, u8),
    Record(Record),
    Ini(Vec<u8>),
}

pub struct Baseline {
    pub raw: RawIni,
    pub header: MapHeader,
    pub records: BTreeMap<u64, Record>,
    terrain: Vec<u8>,
    offsets: BTreeMap<Cell, usize>,
    end: usize,
    overlay: Vec<u8>,
    frames: Vec<u8>,
}
impl Baseline {
    pub fn parse(bytes: Arc<[u8]>, map: &MapFile) -> Result<Arc<Self>> {
        let raw = RawIni::parse(bytes)?;
        ensure!(
            raw.lines.iter().any(|line| line.section == "Map"),
            "P2 暂不编辑 MIX 包装地图，请先提取 INI"
        );
        let grid = map.header.cell_grid();
        let side = usize::from(grid.side());
        let terrain = lzo::decompress_chunks(&raw.packed("IsoMapPack5")?, side * side * 11 + 4)?;
        let mut offsets = BTreeMap::new();
        let mut end = 0;
        while end + 4 <= terrain.len() {
            let x = i16::from_le_bytes(terrain[end..end + 2].try_into().unwrap());
            let y = u16::from_le_bytes(terrain[end + 2..end + 4].try_into().unwrap());
            if x == 0 && y == 0 {
                break;
            }
            ensure!(end + 11 <= terrain.len(), "Pack5 记录截断");
            // Pack5 sign-extends X but reads Y as unsigned, like the core decoder.
            let stride = i32::from(grid.side());
            let linear = i32::from(y) * stride + i32::from(x);
            if (0..stride * stride).contains(&linear) {
                offsets.insert([(linear % stride) as u16, (linear / stride) as u16], end);
            }
            end += 11;
        }
        ensure!(
            end == terrain.len() || end + 4 <= terrain.len(),
            "Pack5 尾部截断"
        );
        let decode = |name| -> Result<Vec<u8>> {
            let packed = raw.packed(name)?;
            if packed.is_empty() {
                Ok(vec![])
            } else {
                Ok(lcw::decompress_chunks(&packed, side * side)?)
            }
        };
        let overlay = decode("OverlayPack").context("OverlayPack 不可编辑")?;
        let frames = decode("OverlayDataPack").context("OverlayDataPack 不可编辑")?;
        let mut records = BTreeMap::new();
        let mut seen = HashSet::new();
        for (id, line) in raw.lines.iter().enumerate() {
            if matches!(
                line.section.as_str(),
                "Units"
                    | "Infantry"
                    | "Structures"
                    | "Aircraft"
                    | "Waypoints"
                    | "Terrain"
                    | "Smudge"
            ) && let Some(key) = &line.key
            {
                // First physical duplicate is the core's effective record. Keep
                // all other duplicates byte-exact, and never target them by key.
                if seen.insert((line.section.clone(), key.clone())) {
                    records.insert(
                        id as u64,
                        Record {
                            section: line.section.clone(),
                            key: key.clone(),
                            value: raw.value(id).to_vec(),
                            coordinate_radix: if map.basic.new_ini_format.unwrap_or(0) < 4 {
                                128
                            } else {
                                1000
                            },
                        },
                    );
                }
            }
        }
        Ok(Arc::new(Self {
            raw,
            header: map.header.clone(),
            records,
            terrain,
            offsets,
            end,
            overlay,
            frames,
        }))
    }
    fn value(&self, target: &Target) -> Option<Value> {
        match target {
            Target::Terrain(at) => self.offsets.get(at).map(|offset| {
                Value::Terrain(TerrainValue::read(&self.terrain[offset + 4..offset + 11]))
            }),
            Target::Overlay(at) => {
                let index =
                    at[1] as usize * usize::from(self.header.cell_grid().side()) + at[0] as usize;
                Some(Value::Overlay(
                    *self.overlay.get(index).unwrap_or(&255),
                    *self.frames.get(index).unwrap_or(&0),
                ))
            }
            Target::Record(id) => self.records.get(id).cloned().map(Value::Record),
            Target::IniLine(line) => self
                .raw
                .lines
                .get(*line)
                .filter(|r| r.key.is_some())
                .map(|_| Value::Ini(self.raw.value(*line).to_vec())),
            Target::IniNew(_, _) => None,
        }
    }
}
#[derive(Clone)]
pub struct Snapshot {
    pub resource_profile: Option<String>,
    pub base: Arc<Baseline>,
    pub changes: BTreeMap<Target, Option<Value>>,
    pub document_id: u64,
    pub state_id: u64,
}
impl Snapshot {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        if self.changes.is_empty() {
            return Ok(self.base.raw.bytes.to_vec());
        }
        let base = &self.base;
        let mut records = BTreeMap::new();
        let mut packs = BTreeMap::new();
        let mut append = BTreeMap::<String, Vec<(String, Vec<u8>)>>::new();
        let mut terrain = None;
        let mut additions = Vec::new();
        let mut overlay = None;
        let mut frames = None;
        for (target, value) in &self.changes {
            match target {
                Target::IniLine(line) => {
                    let bytes = match value {
                        Some(Value::Ini(bytes)) => Some(bytes.clone()),
                        None => None,
                        _ => bail!("INI 值类型不匹配"),
                    };
                    records.insert(*line, bytes);
                }
                Target::IniNew(section, key) => {
                    if let Some(Value::Ini(bytes)) = value {
                        append
                            .entry(section.clone())
                            .or_default()
                            .push((key.clone(), bytes.clone()));
                    }
                }
                Target::Terrain(at) => {
                    let Value::Terrain(value) = value.as_ref().context("地形记录不可删除")?
                    else {
                        bail!("地形值类型不匹配")
                    };
                    let data = terrain.get_or_insert_with(|| base.terrain[..base.end].to_vec());
                    if let Some(offset) = base.offsets.get(at) {
                        data[offset + 4..offset + 11].copy_from_slice(&value.bytes());
                    } else {
                        additions.extend_from_slice(&(at[0] as i16).to_le_bytes());
                        additions.extend_from_slice(&at[1].to_le_bytes());
                        additions.extend_from_slice(&value.bytes());
                    }
                }
                Target::Overlay(at) => {
                    let Some(Value::Overlay(id, frame)) = value else {
                        bail!("overlay 值类型不匹配")
                    };
                    let data = overlay.get_or_insert_with(|| {
                        let mut v = base.overlay.clone();
                        let side = usize::from(base.header.cell_grid().side());
                        v.resize(side * side, 255);
                        v
                    });
                    let values = frames.get_or_insert_with(|| {
                        let mut v = base.frames.clone();
                        let side = usize::from(base.header.cell_grid().side());
                        v.resize(side * side, 0);
                        v
                    });
                    let i = at[1] as usize * usize::from(base.header.cell_grid().side())
                        + at[0] as usize;
                    data[i] = *id;
                    values[i] = *frame;
                }
                Target::Record(id) => {
                    if let Some(original) = base.records.get(id) {
                        match value {
                            Some(Value::Record(record)) => {
                                ensure!(
                                    record.section == original.section
                                        && (record.key == original.key
                                            || record.section == "Terrain"),
                                    "不能改变原记录身份"
                                );
                                if record.key == original.key {
                                    records.insert(*id as usize, Some(record.value.clone()));
                                } else {
                                    for (line, _) in base
                                        .raw
                                        .entries(&original.section)
                                        .filter(|(_, row)| row.key.as_ref() == Some(&original.key))
                                    {
                                        records.insert(line, None);
                                    }
                                    append
                                        .entry(record.section.clone())
                                        .or_default()
                                        .push((record.key.clone(), record.value.clone()));
                                }
                            }
                            None => {
                                // Deleting an effective duplicate must remove its shadow
                                // copies too, otherwise native reload resurrects it.
                                for (line, row) in base
                                    .raw
                                    .entries(&original.section)
                                    .filter(|(_, row)| row.key.as_ref() == Some(&original.key))
                                {
                                    let _ = row;
                                    records.insert(line, None);
                                }
                            }
                            _ => bail!("记录值类型不匹配"),
                        }
                    } else if let Some(Value::Record(record)) = value {
                        append
                            .entry(record.section.clone())
                            .or_default()
                            .push((record.key.clone(), record.value.clone()));
                    }
                }
            }
        }
        if let Some(mut data) = terrain {
            data.extend_from_slice(&additions);
            data.extend_from_slice(&base.terrain[base.end..]);
            packs.insert(
                "IsoMapPack5".into(),
                pack_lines(&lzo::compress_chunks(&data, 8192), &base.raw.newline),
            );
        }
        if let Some(data) = overlay {
            packs.insert(
                "OverlayPack".into(),
                pack_lines(&lcw_encode(&data), &base.raw.newline),
            );
        }
        if let Some(data) = frames {
            packs.insert(
                "OverlayDataPack".into(),
                pack_lines(&lcw_encode(&data), &base.raw.newline),
            );
        }
        Ok(base.raw.patch(&records, &packs, &append, true))
    }
}
#[derive(Clone)]
struct Change {
    before: Option<Value>,
    after: Option<Value>,
}
struct Transaction {
    name: String,
    changes: BTreeMap<Target, Change>,
    before: u64,
    after: u64,
    cost: usize,
    replacement: Option<(DocumentState, DocumentState)>,
}
#[derive(Clone)]
struct DocumentState {
    base: Arc<Baseline>,
    changes: BTreeMap<Target, Option<Value>>,
    next_record: u64,
}
pub struct MapDocument {
    pub resource_profile: Option<String>,
    pub id: u64,
    pub state_id: u64,
    pub saved_state: Option<u64>,
    pub base: Arc<Baseline>,
    pub(crate) changes: BTreeMap<Target, Option<Value>>,
    pending: BTreeMap<Target, Change>,
    undo: VecDeque<Transaction>,
    redo: Vec<Transaction>,
    next_state: u64,
    next_record: u64,
    history_bytes: usize,
}
static NEXT_DOC: AtomicU64 = AtomicU64::new(1);
impl MapDocument {
    /// Structural edits retain the old baseline and history as one undo step.
    /// Parse everything before publishing; callers cannot replace a live brush.
    pub fn replace_bytes(&mut self, bytes: Vec<u8>, name: &str) -> Result<bool> {
        ensure!(self.pending.is_empty(), "请先完成或取消当前编辑事务");
        if self.snapshot().bytes()? == bytes {
            return Ok(false);
        }
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
        let base = Baseline::parse(bytes.into(), &map)?;
        let before = DocumentState {
            base: self.base.clone(),
            changes: self.changes.clone(),
            next_record: self.next_record,
        };
        let after = DocumentState {
            next_record: base.raw.lines.len() as u64,
            base,
            changes: BTreeMap::new(),
        };
        let cost = before.base.raw.bytes.len()
            + after.base.raw.bytes.len()
            + before.changes.values().map(value_size).sum::<usize>();
        self.history_bytes -= self.redo.iter().map(|tx| tx.cost).sum::<usize>();
        self.redo.clear();
        self.restore_state(&after);
        self.undo.push_back(Transaction {
            name: name.into(),
            changes: BTreeMap::new(),
            before: self.state_id,
            after: self.next_state,
            cost,
            replacement: Some((before, after)),
        });
        self.history_bytes += cost;
        self.state_id = self.next_state;
        self.next_state += 1;
        self.trim_history();
        Ok(true)
    }
    fn restore_state(&mut self, state: &DocumentState) {
        self.base = state.base.clone();
        self.changes = state.changes.clone();
        self.next_record = state.next_record;
    }
    fn trim_history(&mut self) {
        while self.history_bytes > 64 * 1024 * 1024 && self.undo.len() > 1 {
            self.history_bytes -= self.undo.pop_front().unwrap().cost;
        }
    }
    /// Independent scratch document at the current visible state, without
    /// copying undo history or committing/cancelling the active transaction.
    pub fn fork(&self) -> Self {
        let mut fork = Self::new(self.base.clone());
        fork.resource_profile = self.resource_profile.clone();
        fork.changes = self.changes.clone();
        fork.next_record = self.next_record;
        fork
    }
    /// A command-level savepoint, including an already pending brush transaction.
    /// This does not commit: callers can include scene-dependent neighbour edits.
    pub fn atomic<T>(&mut self, command: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let saved = (self.changes.clone(), self.pending.clone(), self.next_record);
        match command(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                (self.changes, self.pending, self.next_record) = saved;
                Err(error)
            }
        }
    }
    pub fn insert_record(&mut self, mut record: Record) -> Result<u64> {
        let current = self.records();
        if record.section != "Terrain" {
            let reserved: HashSet<String> = self
                .base
                .raw
                .entries(&record.section)
                .filter_map(|(_, line)| line.key.clone())
                .chain(
                    current
                        .values()
                        .filter(|r| r.section == record.section)
                        .map(|r| r.key.clone()),
                )
                .collect();
            let start = if record.section == "Waypoints" { 30 } else { 0 };
            let end = if record.section == "Waypoints" {
                702
            } else {
                u32::MAX
            };
            record.key = (start..end)
                .find(|n| !reserved.contains(&n.to_string()))
                .context("没有可用的记录编号")?
                .to_string();
        }
        let id = self.next_record;
        self.set(Target::Record(id), Some(Value::Record(record)))?;
        self.next_record += 1;
        Ok(id)
    }
    pub fn new(base: Arc<Baseline>) -> Self {
        let next_record = base.raw.lines.len() as u64;
        Self {
            resource_profile: None,
            id: NEXT_DOC.fetch_add(1, Ordering::Relaxed),
            state_id: 0,
            saved_state: Some(0),
            base,
            changes: BTreeMap::new(),
            pending: BTreeMap::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            next_state: 1,
            next_record,
            history_bytes: 0,
        }
    }
    pub fn value(&self, target: &Target) -> Option<Value> {
        self.changes
            .get(target)
            .cloned()
            .unwrap_or_else(|| self.base.value(target))
    }
    pub fn terrain(&self, at: Cell) -> Option<TerrainValue> {
        match self.value(&Target::Terrain(at)) {
            Some(Value::Terrain(v)) => Some(v),
            _ => None,
        }
    }
    pub fn overlay(&self, at: Cell) -> (u8, u8) {
        match self.value(&Target::Overlay(at)) {
            Some(Value::Overlay(id, frame)) => (id, frame),
            _ => (255, 0),
        }
    }
    fn assign(&mut self, target: Target, value: Option<Value>) {
        if value == self.base.value(&target) {
            self.changes.remove(&target);
        } else {
            self.changes.insert(target, value);
        }
    }
    pub fn set(&mut self, target: Target, value: Option<Value>) -> Result<()> {
        match (&target, &value) {
            (Target::IniLine(_) | Target::IniNew(_, _), None | Some(Value::Ini(_))) => {
                let (section, key) = match &target {
                    Target::IniLine(line) => {
                        let r = self.base.raw.lines.get(*line).context("INI 行不存在")?;
                        (&r.section, r.key.as_ref().context("该行不是 INI 字段")?)
                    }
                    Target::IniNew(section, key) => (section, key),
                    _ => unreachable!(),
                };
                crate::ini_edit::validate_target(section, key)?;
                if let Some(Value::Ini(bytes)) = &value {
                    ensure!(
                        bytes.len() <= 480
                            && !bytes.iter().any(|b| matches!(b, b'\r' | b'\n' | 0 | b';')),
                        "INI 值最多 480 字节，不能含换行、NUL 或分号"
                    );
                }
            }
            (Target::Terrain(_), Some(Value::Terrain(_)))
            | (Target::Overlay(_), Some(Value::Overlay(_, _)))
            | (Target::Record(_), None) => {}
            (Target::Record(id), Some(Value::Record(record))) => {
                ensure!(
                    !record.value.iter().any(|b| matches!(b, b'\r' | b'\n' | 0)),
                    "记录不能包含换行或 NUL"
                );
                self.check_cell(record.cell().context("记录坐标无效")?)?;
                if record.section == "Terrain" {
                    ensure!(
                        !self.records().iter().any(|(other, r)| other != id
                            && r.section == "Terrain"
                            && r.cell() == record.cell()),
                        "该格子已有地形对象"
                    );
                }
            }
            _ => bail!("编辑值与目标不匹配"),
        }
        if let Target::Terrain(at) | Target::Overlay(at) = &target {
            self.check_cell(*at)?;
        }
        let before = self.value(&target);
        if before == value {
            return Ok(());
        }
        self.pending
            .entry(target.clone())
            .and_modify(|c| c.after = value.clone())
            .or_insert(Change {
                before,
                after: value.clone(),
            });
        self.assign(target, value);
        Ok(())
    }
    pub fn check_cell(&self, at: Cell) -> Result<()> {
        let side = self.base.header.cell_grid().side();
        ensure!(at[0] < side && at[1] < side, "坐标超过地图格网");
        ensure!(
            vera20k_core::map::playfield::size_diamond_contains(
                self.base.header.width as i32,
                self.base.header.height as i32,
                (at[0] as i16, at[1] as i16)
            ),
            "格子在 Size 之外"
        );
        Ok(())
    }
    pub fn commit(&mut self, name: impl Into<String>) -> bool {
        self.pending.retain(|_, c| c.before != c.after);
        if self.pending.is_empty() {
            return false;
        }
        let changes = std::mem::take(&mut self.pending);
        let cost = changes
            .values()
            .map(|c| 128 + value_size(&c.before) + value_size(&c.after))
            .sum();
        self.history_bytes += cost;
        self.history_bytes -= self.redo.iter().map(|tx| tx.cost).sum::<usize>();
        self.redo.clear();
        self.undo.push_back(Transaction {
            name: name.into(),
            changes,
            before: self.state_id,
            after: self.next_state,
            cost,
            replacement: None,
        });
        self.state_id = self.next_state;
        self.next_state += 1;
        self.trim_history();
        true
    }
    pub fn cancel(&mut self) {
        for (target, c) in std::mem::take(&mut self.pending) {
            self.assign(target, c.before);
        }
    }
    pub fn undo(&mut self) -> bool {
        self.cancel();
        let Some(tx) = self.undo.pop_back() else {
            return false;
        };
        if let Some((before, _)) = &tx.replacement {
            self.restore_state(before);
        }
        for (t, c) in &tx.changes {
            self.assign(t.clone(), c.before.clone());
        }
        self.state_id = tx.before;
        self.redo.push(tx);
        true
    }
    pub fn redo(&mut self) -> bool {
        self.cancel();
        let Some(tx) = self.redo.pop() else {
            return false;
        };
        if let Some((_, after)) = &tx.replacement {
            self.restore_state(after);
        }
        for (t, c) in &tx.changes {
            self.assign(t.clone(), c.after.clone());
        }
        self.state_id = tx.after;
        self.undo.push_back(tx);
        true
    }
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.back().map(|t| t.name.as_str())
    }
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|t| t.name.as_str())
    }
    pub fn dirty(&self) -> bool {
        self.saved_state != Some(self.state_id) || !self.pending.is_empty()
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            resource_profile: self.resource_profile.clone(),
            base: self.base.clone(),
            changes: self.changes.clone(),
            document_id: self.id,
            state_id: self.state_id,
        }
    }
    pub fn records(&self) -> BTreeMap<u64, Record> {
        let mut records = self.base.records.clone();
        for (target, value) in &self.changes {
            if let Target::Record(id) = target {
                if let Some(Value::Record(record)) = value {
                    records.insert(*id, record.clone());
                } else {
                    records.remove(id);
                }
            }
        }
        records
    }
    pub fn add_record(
        &mut self,
        section: &str,
        cell: Cell,
        owner: &str,
        type_id: &str,
    ) -> Result<u64> {
        ensure!([owner,type_id].iter().all(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_')),"对象类型与所属方必须是 ASCII ID");
        ensure!(
            matches!(
                section,
                "Structures" | "Units" | "Infantry" | "Aircraft" | "Terrain" | "Smudge"
            ),
            "不支持该对象分类"
        );
        let records = self.records();
        if section == "Terrain" {
            let coordinate_radix = if self
                .base
                .raw
                .lines
                .iter()
                .enumerate()
                .find(|(_, r)| r.section == "Basic" && r.key.as_deref() == Some("NewINIFormat"))
                .and_then(|(i, _)| {
                    std::str::from_utf8(self.base.raw.value(i))
                        .ok()?
                        .parse::<i32>()
                        .ok()
                })
                .unwrap_or(0)
                < 4
            {
                128
            } else {
                1000
            };
            ensure!(
                cell[0] < coordinate_radix,
                "地形对象横坐标须小于 {coordinate_radix}；旧格式地图请使用 NewINIFormat=4"
            );
            let existing = records
                .iter()
                .find(|(_, r)| r.section == section && r.cell() == Some(cell))
                .map(|(&id, _)| id);
            let id = existing.unwrap_or(self.next_record);
            if existing.is_none() {
                self.next_record += 1;
            }
            self.set(
                Target::Record(id),
                Some(Value::Record(Record {
                    section: section.into(),
                    key: (u32::from(cell[1]) * u32::from(coordinate_radix) + u32::from(cell[0]))
                        .to_string(),
                    value: type_id.as_bytes().to_vec(),
                    coordinate_radix,
                })),
            )?;
            return Ok(id);
        }
        let mut index = 0;
        while records
            .values()
            .any(|r| r.section == section && r.key == index.to_string())
        {
            index += 1;
        }
        let prefix = format!("{owner},{type_id},256,{},{}", cell[0], cell[1]);
        let value = match section {
            "Infantry" => format!("{prefix},0,Guard,0,None,0,-1,0,1,1"),
            "Structures" => format!("{prefix},0,None,0,0,1,0,None,None,None,0,0,0"),
            "Aircraft" => format!("{prefix},0,Guard,None,0,-1,1,1"),
            "Smudge" => format!("{type_id},{},{},0", cell[0], cell[1]),
            _ => format!("{prefix},0,Guard,None,0,-1,0,-1,1,1"),
        };
        let id = self.next_record;
        self.next_record += 1;
        self.set(
            Target::Record(id),
            Some(Value::Record(Record {
                section: section.into(),
                key: index.to_string(),
                value: value.into_bytes(),
                coordinate_radix: 1000,
            })),
        )?;
        Ok(id)
    }
    pub fn waypoint(&mut self, index: u32, cell: Cell) -> Result<()> {
        ensure!(index <= 701, "航点索引应为 0..701");
        ensure!(cell[0] < 1000, "航点横坐标须小于 1000");
        let key = index.to_string();
        let records = self.records();
        let existing = records
            .iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == key)
            .map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = self.next_record;
            self.next_record += 1;
            id
        });
        self.set(
            Target::Record(id),
            Some(Value::Record(Record {
                section: "Waypoints".into(),
                key,
                value: format!("{}", u32::from(cell[1]) * 1000 + u32::from(cell[0])).into_bytes(),
                coordinate_radix: 1000,
            })),
        )
    }
}
fn value_size(value: &Option<Value>) -> usize {
    match value {
        Some(Value::Record(r)) => r.section.len() + r.key.len() + r.value.len(),
        Some(Value::Ini(bytes)) => bytes.len(),
        _ => 16,
    }
}
pub fn cell_line(a: Cell, b: Cell) -> Vec<Cell> {
    let steps = a[0].abs_diff(b[0]).max(a[1].abs_diff(b[1]));
    if steps == 0 {
        return vec![a];
    }
    (0..=steps)
        .map(|i| {
            [0, 1].map(|axis| {
                ((i32::from(a[axis]) * (i32::from(steps) - i32::from(i))
                    + i32::from(b[axis]) * i32::from(i)
                    + i32::from(steps) / 2)
                    / i32::from(steps)) as u16
            })
        })
        .collect()
}
