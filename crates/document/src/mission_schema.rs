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
        19,
        "最近建成建筑",
        0,
        [Parameter {
            index: 2,
            label: "建筑类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的 last-built 建筑序号，运行时创建/发现也可更新；匹配后锁存。"
    ),
    opcode!(
        20,
        "最近建成车辆",
        0,
        [Parameter {
            index: 2,
            label: "车辆类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的 last-built 原生数组序号。"
    ),
    opcode!(
        21,
        "最近建成步兵",
        0,
        [Parameter {
            index: 2,
            label: "步兵类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的 last-built 原生数组序号。"
    ),
    opcode!(
        22,
        "最近建成飞机",
        0,
        [Parameter {
            index: 2,
            label: "飞机类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的 last-built 原生数组序号。"
    ),
    opcode!(
        32,
        "建筑类型存在",
        0,
        [Parameter {
            index: 2,
            label: "建筑类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的当前 active count 非零；不锁存曾经存在的结果。"
    ),
    opcode!(
        54,
        "间谍伪装步兵",
        0,
        [Parameter {
            index: 2,
            label: "步兵类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "Agent 进入 CellTag，强制伪装的 InfantryType 原生序号。"
    ),
    opcode!(
        57,
        "建筑类型不存在",
        0,
        [Parameter {
            index: 2,
            label: "建筑类型序号",
            kind: ParameterKind::Integer {
                min: 0,
                max: i32::MAX,
                unit: "原生类型序号"
            }
        }],
        "incoming House 的当前 active count 为零；不锁存曾经不存在的结果。"
    ),
    opcode!(
        4,
        "对象发现通知",
        0,
        [],
        "需对象 Tag；原生击杀路径也可能发布同编号。"
    ),
    opcode!(
        5,
        "国家已发现",
        0,
        [COUNTRY],
        "指定 Country 的首个 House，不是规则 owner。"
    ),
    opcode!(
        6,
        "遭攻击",
        0,
        [],
        "对象或 House 攻击通知，来源不保证为敌方。"
    ),
    opcode!(
        9,
        "国家单位消灭",
        0,
        [COUNTRY],
        "首个 House 在场 Unit/Infantry 计数分别 <= 0；不含 Aircraft 和 limbo 乘客。"
    ),
    opcode!(10, "国家建筑消灭", 0, [COUNTRY], "跟踪建筑数 <= 0。"),
    opcode!(
        11,
        "国家全部消灭",
        0,
        [COUNTRY],
        "首个 House 跟踪建筑数 <= 0 且在场 Unit/Infantry 清空；不含 Aircraft 和 limbo 乘客。"
    ),
    opcode!(
        12,
        "资金不少于",
        0,
        [int(2, "资金", 0, i32::MAX, "credits")],
        "读取规则 incoming House 的可用资金。"
    ),
    opcode!(
        15,
        "建筑损失不少于",
        0,
        [int(2, "数量", 0, i32::MAX, "座")],
        "读取规则 incoming House 的累计损失。"
    ),
    opcode!(
        16,
        "单位损失不少于",
        0,
        [int(2, "数量", 0, i32::MAX, "个")],
        "读取规则 incoming House 的累计损失。"
    ),
    opcode!(
        17,
        "没有工厂",
        0,
        [],
        "规则 incoming House 没有非 limbo 工厂建筑。"
    ),
    opcode!(
        23,
        "小队离场",
        1,
        [TEAM],
        "指定 TeamType 的空离场队伍；离场入口通知。"
    ),
    opcode!(
        25,
        "经过水平线",
        0,
        [COUNTRY],
        "绑定格子；同 Tag 同时有垂直线时水平优先。"
    ),
    opcode!(26, "经过垂直线", 0, [COUNTRY], "绑定格子的原生垂直线。"),
    opcode!(29, "对象任意销毁", 0, [], "包含无攻击来源的原生销毁路径。"),
    opcode!(
        30,
        "国家缺电",
        0,
        [COUNTRY],
        "Country 首个 House 的电力状态。"
    ),
    opcode!(33, "对象选择", 0, [], "单人模式选择入口；联机不发布。"),
    opcode!(35, "探照灯发现", 0, [], "由探照灯照射通知其附着对象。"),
    opcode!(
        38,
        "有来源的首次受伤",
        0,
        [],
        "从满血第一次受伤；来源无敌对限制。"
    ),
    opcode!(
        39,
        "有来源的黄血",
        0,
        [],
        "跨过原生黄血阈值，不是持续血量状态。"
    ),
    opcode!(40, "有来源的红血", 0, [], "跨过 ConditionRed 阈值。"),
    opcode!(41, "首次受伤", 0, [], "从满血第一次受伤。"),
    opcode!(42, "跨过黄血", 0, [], "跨过原生黄血阈值。"),
    opcode!(43, "跨过红血", 0, [], "跨过 ConditionRed 阈值。"),
    opcode!(
        45,
        "环境亮度不高于",
        0,
        [int(2, "亮度", i32::MIN, i32::MAX, "原始整数")],
        "读取 current ambient，不是百分比。"
    ),
    opcode!(
        46,
        "环境亮度不低于",
        0,
        [int(2, "亮度", i32::MIN, i32::MAX, "原始整数")],
        "读取 current ambient，不是目标亮度。"
    ),
    opcode!(
        48,
        "销毁或移除",
        0,
        [],
        "销毁、进入建筑等路径发布；不同类别遵循原生入口。"
    ),
    opcode!(
        49,
        "对象拾取木箱",
        0,
        [],
        "绑定对象拾取带 crate_trigger 的木箱。"
    ),
    opcode!(
        50,
        "任意木箱拾取",
        0,
        [],
        "带 crate_trigger 的木箱拾取后发布全局通知。"
    ),
    opcode!(
        51,
        "随机经过时间",
        0,
        [int(2, "时间参数", 0, 95_443_717, "游戏秒")],
        "等待 floor(n/2)+RNG[0,n] 秒；与事件 13 共用计时器。"
    ),
    opcode!(
        52,
        "资金不多于",
        0,
        [int(2, "资金", 0, i32::MAX, "credits")],
        "规则 incoming House 的可用资金。"
    ),
    opcode!(
        53,
        "间谍伪装国家",
        0,
        [COUNTRY],
        "Agent 进入绑定格子；比较其显示所属方。"
    ),
    opcode!(
        55,
        "国家海军消灭",
        0,
        [COUNTRY],
        "首个 House 跟踪 Naval=yes 且 Passengers=0 的 Unit 数 <= 0；不含海军运输工具。"
    ),
    opcode!(
        56,
        "国家陆军消灭",
        0,
        [COUNTRY],
        "首个 House 的跟踪车辆减海军数及跟踪步兵数均 <= 0；海军运输工具和普通运输中的步兵仍计入。"
    ),
    opcode!(
        58,
        "国家电力充足",
        0,
        [COUNTRY],
        "Country 首个 House 的电力状态。"
    ),
    opcode!(
        59,
        "进入或飞越格子",
        0,
        [COUNTRY],
        "高空对象每 16 帧发布通知。"
    ),
    opcode!(
        7,
        "附着对象被摧毁",
        0,
        [],
        "对象销毁事件入口；需要给目标对象绑定 Tag。"
    ),
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
        44,
        "指定 House 来源伤害",
        0,
        [int(
            2,
            "来源 House 序号",
            0,
            i32::MAX,
            "原生 House 数组索引"
        )],
        "对象 Tag 伤害通知；参数不是 Country。SDK 仅单人模式投影最终 Houses 注册顺序。"
    ),
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
        43,
        "航点体素动画",
        0,
        [
            int(2, "体素动画序号", 0, i32::MAX, "VoxelAnimType 原生数组"),
            wp(7, true)
        ],
        "读取 VoxelAnims 注册序号，不是 Animations；在航点生成体素碎片动画。"
    ),
    opcode!(
        52,
        "附着探照灯模式",
        0,
        [int(2, "模式", 0, 3, "原生枚举")],
        "仅当前活动 Trigger Tag 关联的有效探照灯建筑。"
    ),
    opcode!(
        41,
        "航点动画",
        0,
        [
            int(2, "动画序号", 0, i32::MAX, "AnimType 原生数组"),
            wp(7, true)
        ],
        "包含隐式注册的动画顺序。"
    ),
    opcode!(
        42,
        "航点爆炸",
        0,
        [
            int(2, "武器序号", 0, i32::MAX, "WeaponType 原生数组"),
            wp(7, true)
        ],
        "读取武器的 Damage 和 Warhead，直接引爆；不发射弹体。序号包含隐式注册的武器。"
    ),
    opcode!(
        88,
        "航点粒子系统",
        0,
        [
            int(2, "系统序号", 0, i32::MAX, "ParticleSystemType 原生数组"),
            wp(7, true)
        ],
        "原生粒子系统构造入口。"
    ),
    opcode!(
        9,
        "House 紧急策略",
        0,
        [COUNTRY],
        "仅设置状态 4，由普通 AI 帧决定出售/猎杀。"
    ),
    opcode!(
        12,
        "删除触发器实例",
        2,
        [TRIGGER],
        "延迟删除目标定义的活动实例。"
    ),
    opcode!(
        13,
        "启用自动组队",
        0,
        [COUNTRY],
        "与生产/AI 触发选择开关独立。"
    ),
    opcode!(
        30,
        "自动基地建设",
        0,
        [int(2, "启用", 0, 1, "布尔")],
        "incoming House 基地建设开关。"
    ),
    opcode!(
        35,
        "超武目标类别",
        0,
        [int(2, "类别", i32::MIN, i32::MAX, "原生 Quarry/策略")],
        "incoming House；1 特指最佳集结目标。"
    ),
    opcode!(
        37,
        "双方同盟",
        0,
        [COUNTRY],
        "incoming House 与所选 House 互设同盟。"
    ),
    opcode!(
        38,
        "双方敌对",
        0,
        [COUNTRY],
        "incoming House 与所选 House 互设敌对。"
    ),
    opcode!(
        58,
        "陨石雨",
        0,
        [int(2, "强度", 0, 3, "原生档位"), wp(7, true)],
        "四档数量基数为 1、5、9、15，实际数量还受场景随机数影响；需要活动资源中的陨石动画。"
    ),
    opcode!(59, "减少矿物", 0, [wp(2, false)], "共享矿物减少入口。"),
    opcode!(63, "航点桥梁伤害", 0, [wp(2, false)], "共享桥梁伤害入口。"),
    opcode!(
        64,
        "小型战斗光照",
        0,
        [wp(2, false)],
        "C4 impact damage 50，仅光照请求。"
    ),
    opcode!(
        65,
        "中型战斗光照",
        0,
        [wp(2, false)],
        "C4 impact damage 100，仅光照请求。"
    ),
    opcode!(
        66,
        "大型战斗光照",
        0,
        [wp(2, false)],
        "C4 impact damage 300，仅光照请求。"
    ),
    opcode!(69, "结束场景", 0, [], "单人结果/退出入口。"),
    opcode!(
        70,
        "删除 Tag 实例",
        3,
        [reference(2, "Tag", Kind::Tag)],
        "即时删除 Tag，关联 Trigger 延迟清理。"
    ),
    opcode!(
        71,
        "环境变化步长",
        0,
        [int(2, "binary32 位", i32::MIN, i32::MAX, "浮点位编码")],
        "把整数位解释为 binary32，再转 binary64。"
    ),
    opcode!(
        72,
        "环境变化率",
        0,
        [int(2, "binary32 位", i32::MIN, i32::MAX, "浮点位编码")],
        "不是整数比例；同原生浮点读取。"
    ),
    opcode!(
        73,
        "环境亮度",
        0,
        [int(2, "亮度", i32::MIN, i32::MAX, "百分数")],
        "普通 ambient 和非风暴目标。"
    ),
    opcode!(
        76,
        "AI 触发组队比例",
        0,
        [int(2, "比例", i32::MIN, i32::MAX, "原生整数")],
        "incoming House；不重启计时。"
    ),
    opcode!(
        77,
        "飞机组队比例",
        0,
        [int(2, "比例", i32::MIN, i32::MAX, "原生整数")],
        "incoming House。"
    ),
    opcode!(
        78,
        "步兵组队比例",
        0,
        [int(2, "比例", i32::MIN, i32::MAX, "原生整数")],
        "incoming House。"
    ),
    opcode!(
        79,
        "车辆组队比例",
        0,
        [int(2, "比例", i32::MIN, i32::MAX, "原生整数")],
        "incoming House。"
    ),
    opcode!(
        85,
        "藤蔓生长",
        0,
        [int(2, "启用", 0, 1, "布尔")],
        "修改场景开关。"
    ),
    opcode!(
        86,
        "矿物生长",
        0,
        [int(2, "启用", 0, 1, "布尔")],
        "修改矿物 grower 开关。"
    ),
    opcode!(
        87,
        "冰生长",
        0,
        [int(2, "启用", 0, 1, "布尔")],
        "仅场景冰开关，不调用矿物 grower。"
    ),
    opcode!(
        89,
        "结束航点粒子系统",
        0,
        [wp(7, true)],
        "标记完成，由原生命周期清理。"
    ),
    opcode!(94, "离子爆炸", 0, [wp(7, true)], "共享效果入口。"),
    opcode!(95, "核打击", 0, [wp(7, true)], "共享核弹入口。"),
    opcode!(96, "化学打击", 0, [wp(7, true)], "共享化学打击入口。"),
    opcode!(97, "切换火车木箱", 0, [], "反转场景 TrainCrate。"),
    opcode!(
        106,
        "类型科技等级",
        9,
        [
            Parameter {
                index: 2,
                label: "类型",
                kind: ParameterKind::Type("")
            },
            int(7, "科技", i32::MIN, i32::MAX, "等级")
        ],
        "运行时科技类型精确名称。"
    ),
    opcode!(
        113,
        "House 欢呼",
        0,
        [COUNTRY],
        "成员是否可欢呼由原生对象判断。"
    ),
    opcode!(118, "清除全部污迹", 0, [], "清除 smudge 身份并请求重绘。"),
    opcode!(
        142,
        "光照红色",
        0,
        [int(2, "强度", i32::MIN, i32::MAX, "百分数")],
        "修改普通 light 组件并发布 normal palette。"
    ),
    opcode!(
        143,
        "光照绿色",
        0,
        [int(2, "强度", i32::MIN, i32::MAX, "百分数")],
        "修改普通 light 组件。"
    ),
    opcode!(
        144,
        "光照蓝色",
        0,
        [int(2, "强度", i32::MIN, i32::MAX, "百分数")],
        "修改普通 light 组件。"
    ),
    opcode!(
        10,
        "全屏影片",
        0,
        [int(2, "影片序号", 0, i32::MAX, "Movie 注册序号")],
        "仅单人影片服务。"
    ),
    opcode!(
        100,
        "雷达影片",
        0,
        [int(2, "影片序号", 0, i32::MAX, "Movie 注册序号")],
        "仅战役，不暂停场景。"
    ),
    opcode!(
        117,
        "暂停雷达影片",
        0,
        [int(2, "影片序号", 0, i32::MAX, "Movie 注册序号")],
        "仅战役，播放结束释放暂停。"
    ),
    opcode!(
        98,
        "随机航点声音",
        7,
        [Parameter {
            index: 2,
            label: "声音",
            kind: ParameterKind::Sound
        }],
        "引擎 RNG 选取已有航点。"
    ),
    opcode!(
        99,
        "航点声音",
        7,
        [
            Parameter {
                index: 2,
                label: "声音",
                kind: ParameterKind::Sound
            },
            wp(7, true)
        ],
        "原生位置声音入口。"
    ),
    opcode!(
        116,
        "停止航点声音",
        0,
        [wp(7, true)],
        "停止自定义声源或匹配位置请求。"
    ),
    opcode!(46, "锁定输入", 0, [], "场景输入锁。"),
    opcode!(47, "解除输入锁", 0, [], "场景输入锁。"),
    opcode!(
        110,
        "场景暂停",
        0,
        [int(2, "持续时间", 1, 35_791_394, "60 个 16ms 桶")],
        "定时到期宿主恢复；0 为无脚本恢复入口的无限期暂停，SDK 拒绝。"
    ),
    opcode!(
        127,
        "超时空画面",
        0,
        [int(2, "阶段时间", 0, i32::MAX, "服务帧")],
        "两段时间及中间 45 帧，恢复输入与暂停。"
    ),
    opcode!(
        105,
        "对话气泡",
        5,
        [TEAM, int(7, "气泡", 0, 3, "图形编号")],
        "首个 Team 首成员；-1 Team 清除气泡。"
    ),
    opcode!(
        32,
        "销毁附着对象",
        0,
        [],
        "当前活动 Trigger 的 Tag 对象集合；C4 接收链。"
    ),
    opcode!(
        60,
        "出售附着建筑",
        0,
        [],
        "当前活动 Trigger 的 Tag 建筑集合。"
    ),
    opcode!(61, "附着建筑离线", 0, [], "仅作用于当前 Tag 的建筑。"),
    opcode!(62, "附着建筑在线", 0, [], "仅作用于当前 Tag 的建筑。"),
    opcode!(91, "附着步兵 Psycho", 0, [], "仅作用于当前 Tag 的步兵。"),
    opcode!(81, "唤醒附着对象", 0, [], "附着 Sleep/Harmless 移动对象。"),
    opcode!(
        82,
        "唤醒其他 House",
        0,
        [],
        "非 incoming House 的 Sleep 移动对象，无敌对过滤。"
    ),
    opcode!(
        83,
        "唤醒本 House",
        0,
        [],
        "incoming House 的 Harmless 移动对象。"
    ),
    opcode!(
        84,
        "唤醒 Group",
        0,
        [int(2, "Group", i32::MIN, i32::MAX, "编号")],
        "不按 House 过滤。"
    ),
    opcode!(
        125,
        "创建建筑",
        10,
        [
            Parameter {
                index: 2,
                label: "建筑",
                kind: ParameterKind::Type("BuildingTypes")
            },
            wp(7, true)
        ],
        "incoming House；原生清理地基/放置准入。"
    ),
    opcode!(111, "释放驻军", 0, [], "首个符合条件的附着建筑。"),
    opcode!(
        119,
        "摧毁 House 全部对象",
        0,
        [COUNTRY],
        "原生 HouseBlowup All 集合。"
    ),
    opcode!(
        120,
        "摧毁 House 建筑",
        0,
        [COUNTRY],
        "原生 HouseBlowup Buildings 集合。"
    ),
    opcode!(
        121,
        "摧毁 House 陆军",
        0,
        [COUNTRY],
        "按类型 Naval=no 选择非建筑对象，包含 Aircraft；跳过 limbo。"
    ),
    opcode!(
        122,
        "摧毁 House 海军",
        0,
        [COUNTRY],
        "按类型 Naval=yes 选择非建筑对象，包含海军运输工具；跳过 limbo。"
    ),
    opcode!(
        123,
        "控制 House 建筑",
        0,
        [COUNTRY],
        "incoming House 夺取所选 House 建筑，并记录原属。"
    ),
    opcode!(
        124,
        "归还 House 建筑",
        0,
        [COUNTRY],
        "归还此前由 incoming House 控制且记录原属为所选 House 的建筑。"
    ),
    opcode!(
        126,
        "恢复初始对象",
        0,
        [COUNTRY],
        "从引擎已保存开局历史重建四类对象。"
    ),
    opcode!(
        130,
        "恢复初始建筑",
        0,
        [COUNTRY],
        "从开局历史修复或补建建筑。"
    ),
    opcode!(
        36,
        "转交 House 全部对象",
        0,
        [COUNTRY],
        "incoming House 的对象转交所选 House。"
    ),
    opcode!(
        131,
        "闪烁本 House 建筑",
        9,
        [
            Parameter {
                index: 2,
                label: "建筑",
                kind: ParameterKind::Type("BuildingTypes")
            },
            int(7, "时间", i32::MIN, i32::MAX, "逻辑帧")
        ],
        "精确类型 ID。"
    ),
    opcode!(
        33,
        "授予一次超武",
        0,
        [int(2, "超武序号", 0, i32::MAX, "SuperWeaponTypes 序号")],
        "incoming House 的超武实例。"
    ),
    opcode!(
        34,
        "授予重复超武",
        0,
        [int(2, "超武序号", 0, i32::MAX, "SuperWeaponTypes 序号")],
        "不绕过目标与发射准入。"
    ),
    opcode!(
        129,
        "设置超武充能",
        11,
        [
            int(2, "超武序号", 0, i32::MAX, "序号"),
            int(7, "充能", 0, 100, "百分比")
        ],
        "需要已授予；尾参数不是航点。"
    ),
    opcode!(
        132,
        "覆盖超武充能时间",
        11,
        [
            int(2, "超武序号", 0, i32::MAX, "序号"),
            int(7, "时间", 0, i32::MAX, "逻辑帧")
        ],
        "不重启当前计时。"
    ),
    opcode!(
        133,
        "恢复超武充能时间",
        0,
        [int(2, "超武序号", 0, i32::MAX, "序号")],
        "恢复类型时间，不重启当前计时。"
    ),
    opcode!(
        134,
        "重置超武",
        0,
        [int(2, "超武序号", 0, i32::MAX, "序号")],
        "共享 SuperClass Reset 入口。"
    ),
    opcode!(
        135,
        "超武首选目标",
        0,
        [wp(7, true)],
        "incoming House；不立即发射。"
    ),
    opcode!(136, "清除超武目标", 0, [], "incoming House。"),
    opcode!(
        140,
        "超武防御位置",
        0,
        [wp(7, true)],
        "设置 incoming House 防御位置和当前帧戳。"
    ),
    opcode!(141, "清除超武防御", 0, [], "清除位置并恢复原生帧戳。"),
    opcode!(
        90,
        "闪电云与闪电",
        0,
        [wp(7, true)],
        "CreateCloudBolt 原生入口。"
    ),
    opcode!(
        44,
        "全局闪电风暴",
        0,
        [int(2, "持续时间", 1, i32::MAX, "逻辑帧")],
        "无 House，Rules 延迟。"
    ),
    opcode!(
        45,
        "停止闪电风暴",
        0,
        [],
        "请求已活动风暴结束，不立即删除云。"
    ),
    opcode!(
        102,
        "航点闪电风暴",
        0,
        [wp(7, true)],
        "incoming House，Rules 时间/延迟。"
    ),
    opcode!(
        128,
        "House 超时空",
        0,
        [wp(7, true)],
        "incoming House 的可移动对象。"
    ),
    opcode!(109, "航点铁幕", 0, [wp(7, true)], "原生铁幕范围效果。"),
    opcode!(
        17,
        "揭示航点周围",
        0,
        [wp(2, false)],
        "当前玩家；原生固定范围。"
    ),
    opcode!(
        18,
        "揭示航点区域",
        0,
        [wp(2, false)],
        "原生 zone，不是自定义半径。"
    ),
    opcode!(31, "生长迷雾", 0, [], "原生全地图迷雾生长服务。"),
    opcode!(
        40,
        "修改显示范围",
        0,
        [
            int(3, "X", 0, 65535, "格"),
            int(4, "Y", 0, 65535, "格"),
            int(5, "宽", 1, 65535, "格"),
            int(6, "高", 1, 65535, "格")
        ],
        "四个坐标字段，不使用主整数和尾航点。"
    ),
    opcode!(
        48,
        "平移镜头",
        0,
        [int(2, "速度", 0, 4, "原生速度档位"), wp(7, true)],
        "客户端镜头请求。"
    ),
    opcode!(51, "重新遮蔽地图", 0, [], "作用于规则 incoming House。"),
    opcode!(
        55,
        "雷达提示",
        0,
        [int(2, "种类", 0, 16, "枚举"), wp(7, true)],
        "客户端准入与去重。"
    ),
    opcode!(
        101,
        "遮蔽航点周围",
        0,
        [wp(2, false)],
        "当前玩家的原生固定范围。"
    ),
    opcode!(112, "跳转镜头", 0, [wp(7, true)], "立即跳至航点。"),
    opcode!(
        114,
        "侧栏页",
        0,
        [int(2, "页", 0, 3, "枚举")],
        "空页由客户端拒绝。"
    ),
    opcode!(
        115,
        "闪烁侧栏图标",
        9,
        [
            Parameter {
                index: 2,
                label: "科技类型",
                kind: ParameterKind::Type("")
            },
            int(7, "时间", i32::MIN, i32::MAX, "逻辑帧")
        ],
        "类型 9：名称位于字段 2，时间位于字段 7。"
    ),
    opcode!(
        139,
        "雷达中断",
        0,
        [int(2, "时间", 0, i32::MAX, "逻辑帧")],
        "作用于规则 incoming House。"
    ),
    opcode!(145, "回基地镜头", 0, [], "本地玩家 Home 入口。"),
    opcode!(
        1,
        "指定国家胜利",
        0,
        [COUNTRY],
        "Country 索引：与当前玩家 Country 相同则胜、不同则负；不作用于触发器 owner。"
    ),
    opcode!(
        2,
        "指定国家失败",
        0,
        [COUNTRY],
        "与当前玩家 Country 相同则负、不同则胜；不选择触发器 House。"
    ),
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
        14,
        "改变附着对象所属方",
        0,
        [COUNTRY],
        "改变挂接此触发器的全部有效对象；4475～4504 分别为航点 0～29 的玩家（8 以上为 VERA 扩展），8997 为保留 House。"
    ),
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
        "设置剩余时间并启动，同时清空标题；自定义标题须在此动作之后设置。"
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
        [TEAM, int(7, "闪烁时间", i32::MIN, i32::MAX, "带符号逻辑帧")],
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
    opcode!(
        137,
        "设置备用基地",
        0,
        [wp(7, true)],
        "修改活动触发器所属 House 的备用基地格子；不移动建筑。"
    ),
    opcode!(
        138,
        "清除备用基地",
        0,
        [],
        "清除活动触发器所属 House 的备用基地格子。"
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
