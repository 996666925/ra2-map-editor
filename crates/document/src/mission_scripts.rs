//! ScriptClass operands differ from TAction operands: waypoints are integers.
use super::{Opcode, ParameterKind as P, int, wp};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
pub const OPCODES: &[Opcode] = &[
    opcode!(
        0,
        "攻击目标类别",
        0,
        [int(1, "目标类别", 0, 11, "Quarry")],
        "原生 Quarry 枚举。"
    ),
    opcode!(1, "攻击航点", 0, [wp(1, false)], "整数航点编号。"),
    opcode!(2, "永久等待", 0, [], "不自动进入下一行。"),
    opcode!(3, "移动到航点", 0, [wp(1, false)], "全体成员抵达后推进。"),
    opcode!(
        4,
        "移动到格子",
        0,
        [super::Parameter {
            index: 1,
            label: "格子",
            kind: P::Cell128
        }],
        "旧式 y × 128 + x 编码。"
    ),
    opcode!(
        5,
        "警戒等待",
        0,
        [int(1, "持续时间", 0, i32::MAX / 15, "秒")],
        "每秒 15 逻辑帧。"
    ),
    opcode!(
        6,
        "跳转到脚本行",
        0,
        [int(1, "行号", 1, 50, "从 1 起")],
        "插入、复制、删除和排序同步迁移行号。"
    ),
    opcode!(
        7,
        "任务胜利",
        0,
        [],
        "当前玩家胜利；不以 Team 所属 House 作为结果方。"
    ),
    opcode!(
        8,
        "卸载",
        0,
        [int(1, "卸载模式", 0, 3, "原生模式")],
        "保留原生卸载参数。"
    ),
    opcode!(9, "展开", 0, [], "展开成员。"),
    opcode!(
        11,
        "执行 Mission",
        0,
        [int(1, "Mission", 0, 32, "编号")],
        "原生任务枚举。"
    ),
    opcode!(
        12,
        "设置全局变量",
        0,
        [super::reference(1, "全局变量", super::Kind::Global)],
        "设置完成标记后可由 Trigger 接力。"
    ),
    opcode!(14, "装载运输工具", 0, [], "等待装载条件。"),
    opcode!(15, "间谍进入航点", 0, [wp(1, false)], "整数航点。"),
    opcode!(16, "巡逻到航点", 0, [wp(1, false)], "整数航点。"),
    opcode!(
        17,
        "切换脚本",
        0,
        [int(1, "脚本注册序号", 0, i32::MAX, "序号")],
        "这是注册序号，不是 Script ID；列表编辑不会修改它。"
    ),
    opcode!(19, "恐慌", 0, [], "设置成员恐慌。"),
    opcode!(
        20,
        "改变所属国家",
        0,
        [super::Parameter {
            index: 1,
            label: "国家索引",
            kind: P::Country
        }],
        "国家索引不是 Houses 序号。"
    ),
    opcode!(21, "散开", 0, [], "命令成员散开。"),
    opcode!(
        23,
        "任务失败",
        0,
        [],
        "当前玩家失败；不以 Team 所属 House 作为结果方。"
    ),
    opcode!(24, "前进到下一行", 0, [], "无额外参数。"),
    opcode!(
        25,
        "播放声音",
        0,
        [int(1, "SoundList 注册序号", 0, i32::MAX, "序号")],
        "使用进程 SOUNDMD 顺序；不是声音名称。"
    ),
    opcode!(
        26,
        "播放影片",
        0,
        [int(1, "Movies 注册序号", 0, i32::MAX, "序号")],
        "使用固定 ARTMD Movies 顺序。"
    ),
    opcode!(
        27,
        "播放音乐",
        0,
        [int(1, "Themes 注册序号", 0, i32::MAX, "序号")],
        "使用活动 ThemeCatalog 顺序。"
    ),
    opcode!(31, "杀死成员", 0, [], "经过伤害链销毁成员。"),
    opcode!(37, "移除成员", 0, [], "经过 Limbo / UnInit 移除。"),
    opcode!(
        38,
        "清除全局变量",
        0,
        [super::reference(1, "全局变量", super::Kind::Global)],
        "全局变量范围 0～49。"
    ),
    opcode!(
        39,
        "设置局部变量",
        0,
        [int(1, "局部变量", 0, 99, "编号")],
        "范围 0～99。"
    ),
    opcode!(
        40,
        "清除局部变量",
        0,
        [int(1, "局部变量", 0, 99, "编号")],
        "范围 0～99。"
    ),
    opcode!(41, "取消恐慌", 0, [], "清除成员恐慌。"),
    opcode!(
        42,
        "转向",
        0,
        [int(1, "朝向", 0, 7, "八方向")],
        "原生八方向。"
    ),
    opcode!(43, "等待装载完成", 0, [], "保留运输编组的等待条件。"),
    opcode!(
        49,
        "标记脚本成功",
        0,
        [],
        "置 succeeded 并推进；到列表末尾销毁 Team。"
    ),
    opcode!(
        50,
        "闪烁成员",
        0,
        [int(1, "时长", i32::MIN, i32::MAX, "逻辑帧")],
        "使用带符号帧数。"
    ),
];
pub fn opcode(id: i32) -> Option<&'static Opcode> {
    OPCODES.iter().find(|s| s.id == id)
}

#[derive(Clone, Copy)]
pub enum Edit {
    Insert(usize),
    Copy(usize),
    Remove(usize),
    Move(usize, usize),
}

/// Permute physical slots and adjust known one-based branch destinations.
/// Sparse or malformed input is rejected, leaving the caller's draft intact.
pub fn edit(fields: &mut BTreeMap<String, String>, script: bool, edit: Edit) -> Result<()> {
    let limit = if script { 50 } else { 6 };
    let mut rows: Vec<_> = fields
        .iter()
        .filter_map(|(k, v)| k.parse::<usize>().ok().map(|i| (i, v.clone())))
        .collect();
    rows.sort_by_key(|r| r.0);
    ensure!(
        rows.iter().enumerate().all(|(i, r)| r.0 == i),
        "列表不连续，请先在原文字段中消歧"
    );
    ensure!(
        rows.iter().all(|r| r.1.split(',').count() == 2),
        "存在扩展列表行；保留原文，不能自动重排"
    );
    let original_len = rows.len();
    match edit {
        Edit::Insert(i) => {
            ensure!(
                i <= rows.len() && rows.len() < limit,
                "插入位置无效或列表已满"
            );
            rows.insert(i, (usize::MAX, if script { "5,1" } else { "1,E1" }.into()));
        }
        Edit::Copy(i) => {
            ensure!(
                i < rows.len() && rows.len() < limit,
                "复制位置无效或列表已满"
            );
            let r = rows[i].clone();
            rows.insert(i + 1, (usize::MAX, r.1));
        }
        Edit::Remove(i) => {
            ensure!(i < rows.len(), "删除位置无效");
            rows.remove(i);
        }
        Edit::Move(a, b) => {
            ensure!(a < rows.len() && b < rows.len(), "排序位置无效");
            rows.swap(a, b);
        }
    }
    if script {
        let positions: BTreeMap<_, _> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.0 != usize::MAX)
            .map(|(i, r)| (r.0, i))
            .collect();
        for (_, value) in &mut rows {
            let f: Vec<_> = value.split(',').collect();
            if f[0].trim() == "6" {
                let old = f[1]
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| n.checked_sub(1));
                ensure!(
                    old.is_some_and(|i| i < original_len),
                    "跳转目标无效，拒绝重排"
                );
                let target = positions
                    .get(&old.unwrap())
                    .ok_or_else(|| anyhow::anyhow!("待删除行仍被跳转引用"))?;
                *value = format!("6,{}", target + 1);
            }
        }
    }
    fields.retain(|k, _| k.parse::<usize>().is_err());
    fields.extend(
        rows.into_iter()
            .enumerate()
            .map(|(i, (_, v))| (i.to_string(), v)),
    );
    Ok(())
}
