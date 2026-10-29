//! Authoring schemas audited against the pinned trigger dispatcher and TeamClass VM.
//! Only named handlers are offered. Other opcodes keep their original payload.
use super::{Kind, MapDocument};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    Integer {
        min: i32,
        max: i32,
        unit: &'static str,
    },
    Reference(Kind),
    Waypoint {
        letters: bool,
    },
    Country,
    Type(&'static str),
    Csf,
    Sound,
    Eva,
    Theme,
    Cell128,
}
#[derive(Clone, Copy, Debug)]
pub struct Parameter {
    pub index: usize,
    pub label: &'static str,
    pub kind: ParameterKind,
}
#[derive(Clone, Copy, Debug)]
pub struct Opcode {
    pub id: i32,
    pub name: &'static str,
    pub parameter_type: i32,
    pub parameters: &'static [Parameter],
    pub hint: &'static str,
}
const fn int(
    index: usize,
    label: &'static str,
    min: i32,
    max: i32,
    unit: &'static str,
) -> Parameter {
    Parameter {
        index,
        label,
        kind: ParameterKind::Integer { min, max, unit },
    }
}
const fn reference(index: usize, label: &'static str, kind: Kind) -> Parameter {
    Parameter {
        index,
        label,
        kind: ParameterKind::Reference(kind),
    }
}
const fn wp(index: usize, letters: bool) -> Parameter {
    Parameter {
        index,
        label: "航点",
        kind: ParameterKind::Waypoint { letters },
    }
}
const COUNTRY: Parameter = Parameter {
    index: 2,
    label: "国家索引（动作 8997 为保留 House）",
    kind: ParameterKind::Country,
};
const LOCAL: Parameter = reference(2, "局部变量", Kind::Variable);
const GLOBAL: Parameter = reference(2, "全局变量", Kind::Global);
const TEAM: Parameter = reference(2, "小队", Kind::Team);
const TRIGGER: Parameter = reference(2, "触发器", Kind::Trigger);
const SECONDS: Parameter = int(2, "时间", 0, i32::MAX / 15, "秒（15 逻辑帧/秒）");
const CSF: Parameter = Parameter {
    index: 2,
    label: "字符串标签",
    kind: ParameterKind::Csf,
};
macro_rules! opcode {
    ($id:literal,$name:literal,$ty:literal,[$($p:expr),*],$hint:literal) => {
        Opcode { id:$id,name:$name,parameter_type:$ty,parameters:&[$($p),*],hint:$hint }
    };
}
pub const EVENTS: &[Opcode] = &[
    opcode!(
        1,
        "进入附着格 / 对象",
        0,
        [COUNTRY],
        "将 Tag 绑定格子或对象；国家索引不是 Houses 的序号。"
    ),
    opcode!(
        8,
        "任意事件 / 始终满足",
        0,
        [],
        "由可达的 Tag 事件入口评估，不是每帧无条件执行。"
    ),
    opcode!(
        13,
        "经过时间",
        0,
        [SECONDS],
        "相对触发器计时；重新启用或重置会影响计时。"
    ),
    opcode!(14, "倒计时结束", 0, [], "需要先用倒计时动作设置并启动。"),
    opcode!(
        34,
        "接近航点",
        0,
        [wp(2, false)],
        "附着移动对象距离航点不超过 1280 leptons（5 格）时判断。"
    ),
    opcode!(
        24,
        "进入相连区域",
        0,
        [COUNTRY],
        "判断附着格所属移动 zone；不等同于只进入这个格子。"
    ),
    opcode!(
        27,
        "全局变量已设置",
        0,
        [GLOBAL],
        "跨场景全局变量；管理面板属于后续战役批次。"
    ),
    opcode!(
        28,
        "全局变量已清除",
        0,
        [GLOBAL],
        "全局编号范围以当前引擎 SCENARIO_GLOBAL_COUNT 为准。"
    ),
    opcode!(31, "桥梁被摧毁", 0, [], "桥梁回调触发附着 Tag。"),
    opcode!(
        36,
        "局部变量已设置",
        0,
        [LOCAL],
        "0～99，引用 VariableNames。"
    ),
    opcode!(
        37,
        "局部变量已清除",
        0,
        [LOCAL],
        "0～99，引用 VariableNames。"
    ),
    opcode!(
        60,
        "科技类型存在",
        2,
        [Parameter {
            index: 3,
            label: "科技类型",
            kind: ParameterKind::Type("")
        }],
        "类型名称在第 3 字段，0 作为原生整数占位。"
    ),
    opcode!(
        61,
        "科技类型不存在",
        2,
        [Parameter {
            index: 3,
            label: "科技类型",
            kind: ParameterKind::Type("")
        }],
        "在活动规则目录中选择类型。"
    ),
    opcode!(
        47,
        "场景经过时间",
        0,
        [SECONDS],
        "从场景逻辑帧起计时，不是该 Trigger 的独立倒计时。"
    ),
];
pub const ACTIONS: &[Opcode] = &[
    opcode!(
        1,
        "指定国家胜利",
        0,
        [COUNTRY],
        "国家索引：8997 为触发器保留 House；-1 不选择任何 House。"
    ),
    opcode!(2, "指定国家失败", 0, [COUNTRY], "通过国家索引选择 House。"),
    opcode!(3, "生产开始", 0, [COUNTRY], "激活所选 House 的生产行为。"),
    opcode!(
        74,
        "启用 House AI 触发器",
        0,
        [COUNTRY],
        "启用 House 的 AI 选择；AITriggerTypesEnable、小队/工厂及条件仍分别校验。"
    ),
    opcode!(
        75,
        "禁用 House AI 触发器",
        0,
        [COUNTRY],
        "只关闭 House 的 AI 选择，不销毁已出队的小队。"
    ),
    opcode!(
        4,
        "创建小队（招募）",
        1,
        [TEAM],
        "从现有对象招募，不凭空生成成员。"
    ),
    opcode!(5, "删除小队", 1, [TEAM], "销毁所选类型的活动 Team。"),
    opcode!(6, "全部猎杀", 0, [COUNTRY], "所选 House 的 AllToHunt。"),
    opcode!(
        7,
        "小队增援",
        1,
        [TEAM],
        "使用 Team.Waypoint；本动作尾部的航点不会覆盖它。"
    ),
    opcode!(
        8,
        "下达投放区信标",
        0,
        [wp(7, true)],
        "字母编码航点；是否显示由游戏呈现层决定。"
    ),
    opcode!(
        11,
        "显示文本",
        4,
        [CSF],
        "使用 CSF 标签，最多 31 字节；不自动添加外部字符串文件。"
    ),
    opcode!(16, "显示整张地图", 0, [], "解除地图遮蔽。"),
    opcode!(
        19,
        "播放声音",
        7,
        [Parameter {
            index: 2,
            label: "声音",
            kind: ParameterKind::Sound
        }],
        "使用活动 SOUND 根的声音 ID。"
    ),
    opcode!(
        20,
        "播放音乐",
        8,
        [Parameter {
            index: 2,
            label: "音乐",
            kind: ParameterKind::Theme
        }],
        "使用活动 THEME 根的曲目 ID。"
    ),
    opcode!(
        21,
        "播放 EVA",
        6,
        [Parameter {
            index: 2,
            label: "EVA 事件",
            kind: ParameterKind::Eva
        }],
        "使用活动 EVA 根的事件 ID。"
    ),
    opcode!(22, "强制触发", 2, [TRIGGER], "通过 Trigger 定义引用执行。"),
    opcode!(23, "启动倒计时", 0, [], "继续当前场景倒计时。"),
    opcode!(24, "停止倒计时", 0, [], "停止但保留剩余时间。"),
    opcode!(25, "延长倒计时", 0, [SECONDS], "原生秒数转换为逻辑帧。"),
    opcode!(26, "缩短倒计时", 0, [SECONDS], "原生秒数转换为逻辑帧。"),
    opcode!(
        27,
        "设置倒计时",
        0,
        [SECONDS],
        "设置剩余时间；另用动作 23 启动。"
    ),
    opcode!(28, "设置全局变量", 0, [GLOBAL], "改变共享场景全局变量。"),
    opcode!(29, "清除全局变量", 0, [GLOBAL], "改变共享场景全局变量。"),
    opcode!(53, "启用触发器", 2, [TRIGGER], "启用定义对应的活动触发器。"),
    opcode!(54, "禁用触发器", 2, [TRIGGER], "禁用定义对应的活动触发器。"),
    opcode!(
        56,
        "设置局部变量",
        0,
        [LOCAL],
        "改变 VariableNames 的对应编号。"
    ),
    opcode!(
        57,
        "清除局部变量",
        0,
        [LOCAL],
        "改变 VariableNames 的对应编号。"
    ),
    opcode!(67, "宣告胜利", 0, [], "通过当前玩家的任务胜利入口。"),
    opcode!(68, "宣告失败", 0, [], "通过当前玩家的任务失败入口。"),
    opcode!(
        80,
        "小队在指定航点增援",
        1,
        [TEAM, wp(7, true)],
        "覆盖 Team.Waypoint；字段 7 为 A/AA 等字母编码。"
    ),
    opcode!(103, "设置倒计时标题", 4, [CSF], "引用 CSF 标签。"),
    opcode!(
        104,
        "闪烁小队成员",
        5,
        [TEAM, int(7, "闪烁时间", 0, i32::MAX, "逻辑帧")],
        "类型 5 在尾字段保存整数；该字段不是航点。"
    ),
    opcode!(
        107,
        "超时空小队增援",
        1,
        [TEAM, wp(7, true)],
        "通过超时空增援路径投放。"
    ),
    opcode!(
        108,
        "创建木箱",
        0,
        [int(2, "木箱种类", 0, 20, "20 表示随机"), wp(7, true)],
        "使用活动覆盖物/木箱规则。"
    ),
];
pub fn opcode(event: bool, id: i32) -> Option<&'static Opcode> {
    (if event { EVENTS } else { ACTIONS })
        .iter()
        .find(|s| s.id == id)
}
pub fn row(event: bool, id: i32) -> Option<Vec<String>> {
    let schema = opcode(event, id)?;
    let mut row = if event {
        if schema.parameter_type == 2 {
            vec![id.to_string(), "2".into(), "0".into(), "E1".into()]
        } else {
            vec![
                id.to_string(),
                schema.parameter_type.to_string(),
                "0".into(),
            ]
        }
    } else {
        vec![
            id.to_string(),
            schema.parameter_type.to_string(),
            "0".into(),
            "0".into(),
            "0".into(),
            "0".into(),
            "0".into(),
            "A".into(),
        ]
    };
    for parameter in schema.parameters {
        row[parameter.index] = match parameter.kind {
            ParameterKind::Reference(_) => "<none>".into(),
            ParameterKind::Waypoint { letters: true } => "A".into(),
            ParameterKind::Type(_) => "E1".into(),
            ParameterKind::Integer { min, .. } if min > 0 => min.to_string(),
            _ => "0".into(),
        };
    }
    Some(row)
}
pub fn waypoint_token(index: u32) -> Result<String> {
    ensure!(index < 702, "航点须在 0～701 之间");
    Ok(if index < 26 {
        ((b'A' + index as u8) as char).to_string()
    } else {
        format!(
            "{}{}",
            (b'A' + (index / 26 - 1) as u8) as char,
            (b'A' + (index % 26) as u8) as char
        )
    })
}
pub fn waypoint_index(value: &str, letters: bool) -> Option<u32> {
    if letters {
        vera20k_core::map::actions::decode_waypoint_token(value)
    } else {
        value.parse().ok()
    }
}
pub fn parameter_error(doc: &MapDocument, kind: ParameterKind, value: &str) -> Option<String> {
    match kind {
        ParameterKind::Integer { min, max, unit } => {
            if value.parse::<i32>().ok().is_none_or(|n| n < min || n > max) {
                return Some(format!("须为 {min}～{max} 的整数（{unit}）"));
            }
        }
        ParameterKind::Reference(Kind::Variable) => {
            if value.parse::<u32>().ok().is_none_or(|n| n >= 100) {
                return Some("局部变量须为 0～99".into());
            }
        }
        ParameterKind::Reference(Kind::Global) => {
            if value
                .parse::<usize>()
                .ok()
                .is_none_or(|n| n >= crate::campaign::GLOBAL_COUNT)
            {
                return Some("全局变量须为 0～49".into());
            }
        }
        ParameterKind::Waypoint { letters } => {
            let id = waypoint_index(value, letters).filter(|id| *id < 702);
            if id.is_none_or(|id| {
                !doc.records()
                    .values()
                    .any(|r| r.section == "Waypoints" && r.key.parse::<u32>().ok() == Some(id))
            }) {
                return Some("航点不存在；字母编码字段请使用 A/B/AA 等".into());
            }
        }
        ParameterKind::Country => {
            if value.parse::<i32>().ok().is_none_or(|n| n < -1) {
                return Some("国家索引须为 -1 或非负整数".into());
            }
        }
        ParameterKind::Cell128 => {
            if value.parse::<i32>().ok().is_none_or(|n| {
                n < 0
                    || n / 128 > i32::from(u16::MAX)
                    || doc
                        .check_cell([(n % 128) as u16, (n / 128) as u16])
                        .is_err()
            }) {
                return Some("坐标须为 y × 128 + x，且在地图内".into());
            }
        }
        ParameterKind::Type(_) if value.is_empty() || value.len() > 24 => {
            return Some("类型 ID 须为 1～24 字节".into());
        }
        ParameterKind::Csf if value.is_empty() || value.len() > 31 => {
            return Some("CSF 标签须为 1～31 字节".into());
        }
        _ => {}
    }
    None
}

#[path = "mission_scripts.rs"]
pub mod scripts;
