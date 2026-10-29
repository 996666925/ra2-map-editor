//! Ordered House BaseClass nodes. Numeric keys use native `%03d` formatting.
use crate::{
    document::{Cell, MapDocument, Target, Value},
    mission::{self, Kind},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

pub const MAX_NODES: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// Preserve extension columns and old encoding when moving or reordering.
    pub raw: Vec<u8>,
    pub building: String,
    pub cell: Cell,
}
impl Node {
    pub fn parse(raw: Vec<u8>) -> Result<Self> {
        let f: Vec<_> = raw.split(|b| *b == b',').collect();
        ensure!(f.len() >= 3, "节点须为 类型,X,Y");
        let building = std::str::from_utf8(f[0])?.trim().to_owned();
        ensure!(!building.is_empty() && building.is_ascii(), "节点类型无效");
        // Negative controls are planner instructions, never building IDs.
        if building.starts_with('-') {
            building.parse::<i32>().context("控制节点无效")?;
        }
        let x = std::str::from_utf8(f[1])?
            .trim()
            .parse::<u16>()
            .context("节点 X 无效")?;
        let y = std::str::from_utf8(f[2])?
            .trim()
            .parse::<u16>()
            .context("节点 Y 无效")?;
        ensure!(
            x <= i16::MAX as u16 && y <= i16::MAX as u16,
            "节点坐标超出有符号 16 位范围"
        );
        Ok(Self {
            raw,
            building,
            cell: [x, y],
        })
    }
    pub fn new(building: &str, cell: Cell) -> Result<Self> {
        mission::identifier(building)?;
        Self::parse(format!("{building},{},{}", cell[0], cell[1]).into_bytes())
    }
    pub fn moved(&self, cell: Cell) -> Result<Self> {
        let mut f: Vec<_> = self.raw.split(|b| *b == b',').map(<[u8]>::to_vec).collect();
        f[1] = cell[0].to_string().into_bytes();
        f[2] = cell[1].to_string().into_bytes();
        Self::parse(f.join(&b','))
    }
    pub fn control(&self) -> bool {
        self.building.starts_with('-')
    }
}

pub fn read(doc: &MapDocument, house: &str) -> Result<Vec<Node>> {
    ensure!(
        mission::ids(doc, Kind::House).iter().any(|id| id == house),
        "House 未注册"
    );
    let fields = doc.ini_entries(house);
    let keys: BTreeSet<_> = fields.iter().map(|(_, k, _)| k).collect();
    ensure!(keys.len() == fields.len(), "House 含重复字段，请先消歧");
    let bodies = doc
        .base
        .raw
        .lines
        .iter()
        .filter(|line| {
            line.section == house
                && doc.base.raw.bytes[line.bytes.clone()]
                    .trim_ascii_start()
                    .starts_with(b"[")
        })
        .count();
    ensure!(bodies <= 1, "House 含重复 section，请先消歧");
    let count = doc
        .ini_value(house, "NodeCount")
        .map(|v| {
            std::str::from_utf8(&v)?
                .trim()
                .parse::<usize>()
                .context("NodeCount 无效")
        })
        .transpose()?
        .unwrap_or(0);
    ensure!(count <= MAX_NODES, "节点数量超过编辑上限 {MAX_NODES}");
    (0..count)
        .map(|i| {
            Node::parse(
                doc.ini_value(house, &format!("{i:03}"))
                    .with_context(|| format!("缺少节点 {i:03}"))?,
            )
            .with_context(|| format!("节点 {i:03}"))
        })
        .collect()
}

/// Does not commit history; callers combine node and structure changes in one command.
pub fn write(doc: &mut MapDocument, house: &str, nodes: &[Node]) -> Result<()> {
    let old = read(doc, house)?;
    ensure!(nodes.len() <= MAX_NODES, "节点数量超过编辑上限");
    for i in old.len()..nodes.len() {
        ensure!(
            doc.ini_value(house, &format!("{i:03}")).is_none(),
            "{i:03} 位于 NodeCount 之外，不能覆盖孤立节点，请先消歧"
        );
    }
    doc.atomic(|doc| {
        for (i, node) in nodes.iter().enumerate() {
            Node::parse(node.raw.clone())?;
            doc.set_ini(house, &format!("{i:03}"), Some(node.raw.clone()))?;
        }
        for i in nodes.len()..old.len() {
            doc.set_ini(house, &format!("{i:03}"), None)?;
        }
        doc.set_ini(
            house,
            "NodeCount",
            Some(nodes.len().to_string().into_bytes()),
        )
    })
}

pub fn append(doc: &mut MapDocument, house: &str, building: &str, at: Cell) -> Result<usize> {
    ensure!(doc.terrain(at).is_some(), "节点在地图格网之外");
    let mut nodes = read(doc, house)?;
    nodes.push(Node::new(building, at)?);
    write(doc, house, &nodes)?;
    Ok(nodes.len() - 1)
}
pub fn move_node(doc: &mut MapDocument, house: &str, index: usize, at: Cell) -> Result<()> {
    ensure!(doc.terrain(at).is_some(), "节点在地图格网之外");
    let mut nodes = read(doc, house)?;
    let node = nodes.get_mut(index).context("节点已不存在")?;
    *node = node.moved(at)?;
    write(doc, house, &nodes)
}
pub fn remove(doc: &mut MapDocument, house: &str, index: usize) -> Result<()> {
    let mut nodes = read(doc, house)?;
    ensure!(index < nodes.len(), "节点已不存在");
    nodes.remove(index);
    write(doc, house, &nodes)
}
pub fn reorder(doc: &mut MapDocument, house: &str, from: usize, to: usize) -> Result<()> {
    let mut nodes = read(doc, house)?;
    ensure!(from < nodes.len() && to < nodes.len(), "节点已不存在");
    let node = nodes.remove(from);
    nodes.insert(to, node);
    write(doc, house, &nodes)
}

pub fn matching_structures(doc: &MapDocument, house: &str, node: &Node) -> Vec<u64> {
    doc.records()
        .iter()
        .filter_map(|(&id, r)| {
            (r.section == "Structures"
                && r.cell() == Some(node.cell)
                && r.type_id().eq_ignore_ascii_case(&node.building)
                && r.fields().first().is_some_and(|owner| owner == house))
            .then_some(id)
        })
        .collect()
}
/// Only the selected, matching placed building is changed. Node order is independent.
pub fn set_rebuild(doc: &mut MapDocument, house: &str, index: usize, rebuild: bool) -> Result<()> {
    let nodes = read(doc, house)?;
    let node = nodes.get(index).context("节点已不存在")?;
    let ids = matching_structures(doc, house, node);
    ensure!(
        ids.len() == 1,
        "此节点须对应唯一的同 House、同类型、同坐标建筑"
    );
    let record = doc.records()[&ids[0]].patched(&[(8, u8::from(rebuild).to_string())])?;
    doc.set(Target::Record(ids[0]), Some(Value::Record(record)))
}

/// Add missing plan sites without changing existing plan order or control nodes.
pub fn import_structures(doc: &mut MapDocument, house: &str) -> Result<usize> {
    let mut nodes = read(doc, house)?;
    let before = nodes.len();
    for r in doc.records().values().filter(|r| {
        r.section == "Structures" && r.fields().first().is_some_and(|owner| owner == house)
    }) {
        let cell = r.cell().context("建筑坐标无效")?;
        if !nodes
            .iter()
            .any(|n| n.cell == cell && n.building.eq_ignore_ascii_case(&r.type_id()))
        {
            ensure!(doc.terrain(cell).is_some(), "建筑位于地图之外");
            nodes.push(Node::new(&r.type_id(), cell)?);
        }
    }
    write(doc, house, &nodes)?;
    Ok(nodes.len() - before)
}

#[cfg(test)]
#[path = "base_nodes_tests.rs"]
mod tests;
