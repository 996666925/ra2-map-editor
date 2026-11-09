//! Ares authoring operands audited against the pinned trigger_runtime_ares owner.
//! Only builds using the engine's Ares parser expose these schemas in forms.
use super::*;

const HOUSE_INDEX: Parameter = int(2, "来源 House 数组索引", 0, i32::MAX, "原生 House 数组索引");
const TYPE_NAME: Parameter = Parameter {
    index: 3,
    label: "科技类型",
    kind: ParameterKind::Type("TechnoTypes"),
};
const SUPER_INDEX: Parameter = int(2, "超武类型序号", 0, i32::MAX, "原生类型序号");
const COUNT: Parameter = int(2, "数量", 0, i32::MAX - 1, "个");

pub const EVENTS: &[Opcode] = &[
    opcode!(
        62,
        "处于 EMP 状态",
        0,
        [],
        "Ares：附着对象收到 EMP 通知且当前 EMP 计时大于零。"
    ),
    opcode!(
        63,
        "被指定 House EMP",
        0,
        [HOUSE_INDEX],
        "Ares：附着对象收到指定来源 House 的 EMP 通知。"
    ),
    opcode!(
        64,
        "EMP 状态结束",
        0,
        [],
        "Ares：附着对象收到 EMP 结束通知。"
    ),
    opcode!(
        65,
        "指定 House 的 EMP 结束",
        0,
        [HOUSE_INDEX],
        "Ares：附着对象收到指定来源 House 的 EMP 结束通知。"
    ),
    opcode!(
        67,
        "驾驶员被击杀",
        0,
        [],
        "Ares：附着对象收到驾驶员击杀通知。"
    ),
    opcode!(
        68,
        "驾驶员被指定 House 击杀",
        0,
        [HOUSE_INDEX],
        "Ares：驾驶员击杀通知的来源 House 数组索引。"
    ),
    opcode!(69, "车辆被接管", 0, [], "Ares：附着对象收到车辆接管通知。"),
    opcode!(
        70,
        "车辆被指定 House 接管",
        0,
        [HOUSE_INDEX],
        "Ares：接管通知的来源 House 数组索引。"
    ),
    opcode!(71, "对象被劫持", 0, [], "Ares：附着对象收到被劫持通知。"),
    opcode!(
        72,
        "对象被指定 House 劫持",
        0,
        [HOUSE_INDEX],
        "Ares：劫持者所属 House 数组索引。"
    ),
    opcode!(
        73,
        "对象劫持其他对象",
        0,
        [],
        "Ares：附着对象收到劫持成功通知。"
    ),
    opcode!(
        74,
        "对象劫持指定 House 的对象",
        0,
        [HOUSE_INDEX],
        "Ares：被劫持者原所属 House 数组索引。"
    ),
    opcode!(
        75,
        "超武激活",
        0,
        [SUPER_INDEX],
        "Ares：所属 House 收到指定超武类型激活通知。"
    ),
    opcode!(
        76,
        "超武停用",
        0,
        [SUPER_INDEX],
        "Ares：所属 House 收到指定超武类型停用通知。"
    ),
    opcode!(
        77,
        "超武在航点附近发射",
        2,
        [
            wp(2, false),
            Parameter {
                index: 3,
                label: "超武类型",
                kind: ParameterKind::Type("SuperWeaponTypes")
            }
        ],
        "Ares：指定超武发射目标距离航点不超过五格。"
    ),
    opcode!(
        78,
        "已逆向工程科技类型",
        2,
        [TYPE_NAME],
        "Ares：所属 House 的逆向工程列表包含指定类型。"
    ),
    opcode!(
        79,
        "对象完成逆向工程",
        0,
        [],
        "Ares：附着对象收到逆向工程通知。"
    ),
    opcode!(
        80,
        "对象逆向工程指定类型",
        2,
        [TYPE_NAME],
        "Ares：附着对象收到指定科技类型的逆向工程通知。"
    ),
    opcode!(
        81,
        "所属 House 拥有至少指定数量",
        2,
        [COUNT, TYPE_NAME],
        "Ares：所属 House 当前拥有指定科技类型至少给定数量。"
    ),
    opcode!(
        82,
        "所属 House 拥有至多指定数量",
        2,
        [COUNT, TYPE_NAME],
        "Ares：所属 House 当前拥有指定科技类型至多给定数量。"
    ),
    opcode!(
        83,
        "对象被攻击或摧毁",
        0,
        [],
        "Ares：附着对象收到攻击或摧毁通知，不限制伤害来源。"
    ),
    opcode!(
        84,
        "对象被指定 House 攻击或摧毁",
        0,
        [HOUSE_INDEX],
        "Ares：攻击或摧毁通知的来源 House 数组索引。"
    ),
    opcode!(
        85,
        "所属 House 的对象被指定 House 摧毁",
        0,
        [HOUSE_INDEX],
        "Ares：所属 House 收到对象被指定 House 摧毁的通知。"
    ),
    opcode!(
        86,
        "全图科技类型至多指定数量",
        2,
        [COUNT, TYPE_NAME],
        "Ares：全图当前存在指定科技类型至多给定数量。"
    ),
    opcode!(
        87,
        "指定 House 无存续对象",
        0,
        [COUNTRY],
        "Ares：Country 或 Player@X 选择的 House 已无 KeepAlive 对象；8997 不适用。"
    ),
    opcode!(
        88,
        "指定 House 无存续建筑",
        0,
        [COUNTRY],
        "Ares：Country 或 Player@X 选择的 House 已无 KeepAlive 建筑；8997 不适用。"
    ),
];
pub const ACTIONS: &[Opcode] = &[
    opcode!(
        92,
        "启用火风暴",
        0,
        [],
        "Ares：启用触发器所属 House 的火风暴。"
    ),
    opcode!(
        93,
        "停用火风暴",
        0,
        [],
        "Ares：停用触发器所属 House 的火风暴。"
    ),
    opcode!(
        146,
        "增加辅助电力",
        11,
        [COUNTRY, int(7, "电力变化", i32::MIN, i32::MAX, "电力")],
        "Ares：为选定 House 增加带符号辅助电力；参数类型须为 11。"
    ),
    opcode!(
        147,
        "击杀附着对象驾驶员",
        0,
        [COUNTRY],
        "Ares：击杀本触发器附着 Foot 的驾驶员，转交给选定 House 或 Special。"
    ),
    opcode!(
        148,
        "选择 EVA 语音",
        0,
        [int(2, "语音序号", -1, i32::MAX, "编号")],
        "Ares：设置 EVA 语音序号；-1 禁用，客户端按实际语音数量检查。"
    ),
    opcode!(
        149,
        "设置附着对象分组",
        0,
        [int(2, "分组", i32::MIN, i32::MAX, "编号")],
        "Ares：设置触发本动作且仍存活对象的 Group。"
    ),
];
pub const ORDERS: &[Opcode] = &[
    opcode!(
        67,
        "进入可占据建筑（车辆优先）",
        0,
        [],
        "Ares：使用车辆搜寻方式寻找可占据建筑。"
    ),
    opcode!(
        68,
        "按 Convert.Script 转换类型",
        0,
        [],
        "Ares：将成员转换为其当前类型的 Convert.Script，参数忽略。"
    ),
];
