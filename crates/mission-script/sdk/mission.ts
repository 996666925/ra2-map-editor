/** Mission SDK v1.
 * 任务脚本 SDK v1。
 *
 * 脚本在编辑器构建任务时执行，生成游戏原生的触发器、小队和行为脚本。
 * event、order、action 中的方法只创建描述数据；游戏中的效果发生在应用地图并运行任务后。
 * 支持同步函数、循环和项目内相对导入，不提供游戏逐帧回调、文件、网络或进程 API。
 *
 * @example
 * ```ts
 * import { defineMission, event, action } from './sdk';
 * import { map, House, Unit, Waypoint, MapObject, Text } from './map.generated';
 *
 * export default defineMission(map, { id: 'first_mission', player: House.Americans }, m => {
 *   m.rule('victory', {
 *     when: event.sinceScenarioStart(300),
 *     once: true,
 *     actions: [action.winPlayer()],
 *   });
 * });
 * ```
 */
export interface MapSchema {
  /** 当前地图可引用的 House ID，包含 Neutral 和 Special。 */
  readonly houses: readonly string[];
  /** 当前地图已经定义的航点编号。 */
  readonly waypoints: readonly number[];
  /** 当前资源配置可用的步兵、车辆和飞机类型 ID。 */
  readonly units: readonly string[];
  /** 支持 Tag 绑定的地图对象记录键，如 Structures:0、Units:3。 */
  readonly objects: readonly string[];
  /** 当前资源配置已经加载的 CSF 文本标签。 */
  readonly strings: readonly string[];
}

declare const referenceBrand: unique symbol;
/**
 * 带类别标记的只读引用。请通过 MissionBuilder 的方法取得，不要自行拼装或强制转换。
 * 不同类别不能互换，例如 House 不能传给需要 Waypoint 的方法。
 * 类型品牌仅供 TS 检查使用，不会写入构建结果；Rust 还会再次验证引用。
 */
export interface Ref<K extends string, I extends string | number = string> {
  /** 引用类别，由 SDK 设置。 */
  readonly kind: K;
  /** 逻辑名称、资源 ID、对象记录键或航点编号，具体含义由 kind 决定。 */
  readonly id: I;
  readonly [referenceBrand]: K;
}
/** 地图中的 House 引用，由 m.house() 取得。 */
export type House = Ref<"house">;
/** 占用指定玩家起点的人类或 AI，由多人任务中的 m.playerAt() 取得。 */
export type StartPlayer = Ref<"startPlayer", number>;
/** 地图运行模式，决定是否固定单人玩家及是否允许按起点选择玩家。 */
export enum MissionMode {
  /** 单人任务；必须指定 player，并写入 Basic/Player。默认模式，兼容已有脚本。 */
  SinglePlayer = "singlePlayer",
  /** 多人或遭遇战地图；玩家在开局分配，不指定固定 player。 */
  Multiplayer = "multiplayer",
}
/** 单人任务入口参数。 */
export interface SinglePlayerOptions<S extends MapSchema> {
  /** 任务逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。 */
  readonly id: string;
  /** 可省略，默认单人任务。 */
  readonly mode?: MissionMode.SinglePlayer;
  /** 当前地图中的玩家 House 枚举成员。 */
  readonly player: NoInfer<S["houses"][number]>;
}
/** 多人或遭遇战任务入口参数。 */
export interface MultiplayerOptions {
  /** 任务逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。 */
  readonly id: string;
  /** 写入 Basic/MultiplayerOnly=yes，按多人地图运行。 */
  readonly mode: MissionMode.Multiplayer;
  /** 多人地图不能指定固定单人玩家，请通过 m.playerAt() 选择起点玩家。 */
  readonly player?: never;
}
/** 地图中已存在的航点引用，由 m.waypoint() 取得。 */
export type Waypoint = Ref<"waypoint", number>;
/** 可加入小队的单位类型引用，由 m.unit() 取得。 */
export type UnitType = Ref<"unit">;
/** 地图中具体对象的引用，由 m.object() 取得，区别于单位类型。 */
export type MapObject = Ref<"object">;
/** 当前任务定义的小队引用，由 m.team() 返回。 */
export type Team = Ref<"team">;
/** 当前任务定义的布尔局部标记引用，由 m.flag() 返回。 */
export type Flag = Ref<"flag">;
/** 已加载的 CSF 标签引用，由 m.text() 取得，区别于直接显示的文本内容。 */
export type Text = Ref<"text">;

/** 单个规则的触发条件，使用 event 中的方法构造；v1 不支持条件组合。 */
export type Event =
  | { readonly kind: "scenarioTime"; readonly seconds: number }
  | { readonly kind: "enabledTime"; readonly seconds: number }
  | { readonly kind: "destroyed"; readonly object: MapObject }
  | { readonly kind: "flagSet" | "flagClear"; readonly flag: Flag };

/** 小队行为脚本的一条命令，使用 order 中的方法构造，按数组顺序交给游戏执行。 */
export type Order =
  | { readonly kind: "moveTo" | "attackWaypoint" | "patrol"; readonly waypoint: Waypoint }
  | { readonly kind: "guard"; readonly seconds: number }
  | { readonly kind: "hunt" };

/** 规则触发后执行的一个动作，使用 action 中的方法构造。 */
export type Action =
  | { readonly kind: "reinforce"; readonly team: Team; readonly at: Waypoint }
  | { readonly kind: "recruit"; readonly team: Team }
  | { readonly kind: "message"; readonly text: Text }
  | { readonly kind: "setFlag" | "clearFlag"; readonly flag: Flag }
  | { readonly kind: "winPlayer" | "losePlayer" };

/** 小队模板参数，定义归属、成员、集结航点和行为，不会立即在地图上生成单位。 */
export interface TeamOptions {
  /** 小队所属方；m.house() 指定固定 House，m.playerAt() 指定开局占用某起点的玩家。 */
  readonly house: House | StartPlayer;
  /** 1～6 种成员；type 不可重复，count 为 1～2147483647 的整数。 */
  readonly units: readonly { readonly type: UnitType; readonly count: number }[];
  /** 1～50 条行为命令，按顺序执行；创建模板时不会执行这些命令。 */
  readonly orders: readonly Order[];
  /** 小队模板的默认航点；指定航点增援时使用 action.reinforce() 的 at 参数。 */
  readonly waypoint: Waypoint;
}
/** 一条触发规则的参数。数量、数值范围和原生字段容量会在 Rust 构建阶段检查。 */
export interface RuleOptions {
  /** 一个触发条件，由 event 方法构造。 */
  readonly when: Event;
  /** 条件满足时按列表顺序执行的 1～64 个动作；过长的原生字段仍可能被拒绝。 */
  readonly actions: readonly Action[];
  /** 必须为 true；v1 每条规则最多触发一次，不支持循环规则。 */
  readonly once: true;
}
/** defineMission() 返回的可序列化任务描述，应作为 mission.ts 的默认导出。 */
export interface Mission {
  /** SDK 数据格式版本，当前固定为 1。 */
  readonly version: 1;
  /** 任务逻辑名称，参与生成稳定的原生记录 ID。 */
  readonly id: string;
  /** 单人或多人模式，决定原生地图运行方式。 */
  readonly mode: MissionMode;
  /** 单人玩家 House ID，写入 Basic/Player；多人模式为空字符串，不覆盖该字段。 */
  readonly player: string;
  /** 本任务声明的小队模板。 */
  readonly teams: readonly (TeamOptions & { readonly id: string })[];
  /** 本任务声明的局部标记及其游戏初始值。 */
  readonly flags: readonly { readonly id: string; readonly initial: boolean }[];
  /** 本任务声明的一次性规则。 */
  readonly rules: readonly (RuleOptions & { readonly id: string })[];
}

function ref<K extends string, I extends string | number>(kind: K, id: I): Ref<K, I> {
  // The nominal brand is deliberately absent from serialized data.
  return Object.freeze({ kind, id }) as Ref<K, I>;
}

/**
 * 同步任务构建器，通常通过 defineMission() 回调的 m 参数使用。
 *
 * 引用方法只查找现有地图或资源；flag、team、rule 方法登记任务定义。
 * 任务、小队、标记和规则的逻辑名称均须为 1～64 位 ASCII 字母、数字或下划线。
 * 小队、标记、规则各自在本任务内不得重名；更名会影响生成项的身份。
 */
export class MissionBuilder<S extends MapSchema, M extends MissionMode = MissionMode.SinglePlayer> {
  private readonly teams: (TeamOptions & { id: string })[] = [];
  private readonly flags: { id: string; initial: boolean }[] = [];
  private readonly rules: (RuleOptions & { id: string })[] = [];

  /**
   * 创建绑定到地图目录的构建器；一般由 defineMission() 自动调用。
   * @param schema 从 map.generated.ts 导入的 map，保留 as const 字面量类型以启用引用检查。
   * @param mode 由 defineMission() 传入的地图模式；默认单人模式。
   */
  constructor(private readonly schema: S, private readonly mode: M = MissionMode.SinglePlayer as M) {}

  /**
   * 取得已有 House 的类型安全引用，不会新建 House 或修改外交关系。
   * @param id 从 map.generated.ts 导入的 House 枚举成员，如 House.Americans。
   * @returns 可用于 TeamOptions.house 的 House 引用。
   * @throws 当 id 不在当前目录中时，在脚本执行阶段报错。
   * @example const enemy = m.house(House.Russians);
   */
  house(id: S["houses"][number]): House {
    this.member(this.schema.houses, id, "House");
    return ref("house", id);
  }
  /**
   * 取得开局占用指定玩家起点的实际所属方，包含人类和 AI，不绑定国家或玩家颜色。
   * @param id 当前地图 Waypoint 枚举中的 WP0～WP7，仅接受地图已有的这些起点。
   * @returns 可传入 TeamOptions.house 的 StartPlayer；生成原生 House=<Player @ A>～<Player @ H>。
   * @throws 单人模式、起点不存在或超出 0～7 时构建失败；Rust 会再次验证。
   * @remarks 仅在 MissionMode.Multiplayer 中可用。玩家随机起点时按实际占位解析；
   * 该起点无人占用时游戏无法创建此小队，不会自动送给中立方。
   * 这里只选择归属，不决定出兵位置；位置仍由小队 waypoint 和 action.reinforce() 指定。
   * 原生小队归属只识别 A～H，普通任务航点及扩展起点 8 以上不能用于此方法。
   * @example house: m.playerAt(Waypoint.WP0)
   */
  playerAt(this: MissionBuilder<S, MissionMode.Multiplayer>, id: Extract<S["waypoints"][number], 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7>): StartPlayer {
    if (this.mode !== MissionMode.Multiplayer) throw new Error("按起点选择玩家仅支持多人任务");
    this.member(this.schema.waypoints, id, "player start");
    if (!Number.isInteger(id) || id < 0 || id > 7) throw new Error("玩家起点仅支持 0～7");
    return ref("startPlayer", id);
  }
  /**
   * 取得地图中已有航点的引用，不会创建或移动航点。
   * @param id 从 map.generated.ts 导入的 Waypoint 枚举成员，如 Waypoint.WP20。
   * @returns 可用于小队、移动命令和增援位置的 Waypoint 引用。
   * @throws 当编号不在当前目录中时，在脚本执行阶段报错。
   * @example const entry = m.waypoint(Waypoint.WP20);
   */
  waypoint(id: S["waypoints"][number]): Waypoint {
    this.member(this.schema.waypoints, id, "waypoint");
    return ref("waypoint", id);
  }
  /**
   * 取得资源配置中的单位类型引用，用来声明小队成员，不会生成单位实例。
   * @param id 从 map.generated.ts 导入的 Unit 枚举成员，如 Unit.E1（美国大兵）、Unit.HTNK（犀牛坦克）。
   * @returns 可用于 TeamOptions.units[].type 的 UnitType 引用。
   * @throws 当类型不在当前目录中时，在脚本执行阶段报错。
   * @remarks 点击“刷新类型”后，输入 Unit. 可补全可用成员，悬停枚举成员即可查看中文注释；
   * 名称会跟随 Mod 和地图规则更新，不用中文显示名称替代内部 ID。
   * @example const infantry = m.unit(Unit.E1);
   */
  unit(id: S["units"][number]): UnitType {
    this.member(this.schema.units, id, "unit type");
    return ref("unit", id);
  }
  /**
   * 取得地图中一个具体对象的引用，用于对象销毁事件，不会生成或修改对象。
   * @param id 从 map.generated.ts 导入的 MapObject 枚举成员，如 MapObject.Structures_0；值为原生记录键。
   * @returns MapObject 引用；它与 m.unit() 返回的单位类型引用不同。
   * @throws 当键不在当前目录中时，在脚本执行阶段报错。
   * @remarks 记录键不是永久 UUID。外部工具重排记录后须重新确认目标。
   * 使用 event.destroyed() 时还会检查重复记录、已有 Tag 和重复绑定。
   * @example const target = m.object(MapObject.Structures_0);
   */
  object(id: S["objects"][number]): MapObject {
    this.member(this.schema.objects, id, "map object");
    return ref("object", id);
  }
  /**
   * 取得已经加载的 CSF 文本标签引用，不会创建标签或将参数当作原文显示。
   * @param id 从 map.generated.ts 导入的 Text 枚举成员，如 Text.MISSION_INTRO；标签值须为不超过 31 字节的 ASCII，
   * 且不能含逗号、分号或换行，Rust 构建阶段会校验。
   * @returns 可交给 action.message() 的 Text 引用。
   * @throws 当标签不在当前目录中时，在脚本执行阶段报错。
   * @example const intro = m.text(Text.MISSION_INTRO);
   */
  text(id: S["strings"][number]): Text {
    this.member(this.schema.strings, id, "CSF label");
    return ref("text", id);
  }
  /**
   * 声明一个布尔局部标记，供规则之间传递任务状态。
   * @param id 本任务内唯一的标记逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。
   * @param initial 游戏开始时的值，默认为 false；不是构建时可读取的游戏状态。
   * @returns 供 event.flagSet/flagClear 和 action.setFlag/clearFlag 使用的 Flag 引用。
   * @remarks 重复调用不会查找已有标记，而会重复声明并在 Rust 构建阶段报错。
   * 原生局部变量槽位为 0～99，与地图已有变量共享；槽位不足或存在无法安全分析的
   * 扩展操作码时构建失败。需要复用时请保存并传递返回的引用。
   * @example const objectiveDone = m.flag('objective_done', false);
   */
  flag(id: string, initial = false): Flag {
    this.flags.push({ id, initial });
    return ref("flag", id);
  }
  /**
   * 声明小队模板，生成原生 TaskForce、ScriptType 和 TeamType。
   * @param id 本任务内唯一的小队逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。
   * @param options 归属 House、默认航点、1～6 种不同成员及 1～50 条行为命令。
   * 成员数量必须为正整数；引用、数量、名称和容量由 Rust 构建阶段再次检查。
   * @returns Team 引用，交给 action.reinforce() 或 action.recruit() 激活。
   * @remarks 仅声明模板，不会立刻出兵；当前每个原生模板的 Max 固定为 1，
   * 多条规则复用它不保证同时存在多个同类型小队，实际组队行为由游戏处理。
   * 一个任务最多声明 256 个小队。
   * @example
   * const assault = m.team('assault', {
   *   house: m.house(House.Russians),
   *   waypoint: m.waypoint(Waypoint.WP20),
   *   units: [{ type: m.unit(Unit.HTNK), count: 2 }],
   *   orders: [order.moveTo(m.waypoint(Waypoint.WP21)), order.hunt()],
   * });
   */
  team(id: string, options: TeamOptions): Team {
    this.teams.push({ ...options, id });
    return ref("team", id);
  }
  /**
   * 登记一条一次性规则，条件满足后由游戏执行动作列表。
   * @param id 本任务内唯一的规则逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。
   * @param options 一个事件条件、1～64 个动作，以及固定的 once: true。
   * @returns 无返回值；此方法不会在构建时执行动作。
   * @remarks 一个任务最多 1024 条规则；当前不支持循环规则、多个条件组合或 JS 回调动作。
   * 动作按列表顺序写入原生触发器，不表示等待一个动作的游戏效果结束后再执行下一个。
   * @example
   * m.rule('win', {
   *   when: event.sinceScenarioStart(300),
   *   once: true,
   *   actions: [action.winPlayer()],
   * });
   */
  rule(id: string, options: RuleOptions): void {
    this.rules.push({ ...options, id });
  }
  /**
   * 汇总当前定义并返回任务描述，通常由 defineMission() 自动调用。
   * @param id 任务逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线，用于生成稳定 ID。
   * @param player 单人玩家 House ID，应来自当前地图目录；多人模式留空。
   * @returns 引用当前定义数组的 Mission 数据；不执行深拷贝、完整验证或地图应用。
   * @remarks 返回后不要继续登记定义。手动调用也不能跳过 Rust 对整个结果的验证。
   */
  finish(id: string, player = ""): Mission {
    return { version: 1, id, mode: this.mode, player, teams: this.teams, flags: this.flags, rules: this.rules };
  }
  private member<T>(values: readonly T[], id: T, label: string): void {
    if (values.indexOf(id) < 0) throw new Error(`Unknown ${label}: ${id}`);
  }
}

/**
 * 构造任务入口；请将返回值作为 mission.ts 的默认导出。
 * @param schema 从同目录 map.generated.ts 导入的 map，提供当前地图和资源的字面量类型。
 * @param options id 为任务逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线；
 * 单人模式 player 为已有的 House 枚举成员；多人模式指定 mode: MissionMode.Multiplayer，省略 player。
 * 应用时设置 Basic/MultiplayerOnly；仅单人模式写入 Basic/Player。
 * @param build 构建时同步调用一次的回调，在其中通过 m 声明引用、小队、标记和规则。
 * 可以使用同步函数和循环生成定义；回调必须返回 undefined，不能使用 async 或返回任务数据。
 * @returns 可序列化的 Mission 描述，随后由 Rust 验证并转换为原生地图记录。
 * @throws 玩家不在目录中、回调抛错或返回了非 undefined 的值（包括 Promise）时构建失败。
 * @remarks 声明成功不代表已经应用到地图；需要构建并应用后，游戏才会使用这些规则。
 * 当前每张地图管理一个脚本产物集合，源码可拆成多个相对导入的 .ts 模块。
 * @example
 * export default defineMission(map, { id: 'bridgehead', player: House.Americans }, m => {
 *   const ready = m.flag('ready');
 *   m.rule('prepare', {
 *     when: event.sinceScenarioStart(10), once: true,
 *     actions: [action.setFlag(ready)],
 *   });
 * });
 */
export function defineMission<const S extends MapSchema>(
  schema: S,
  options: SinglePlayerOptions<S>,
  build: (mission: MissionBuilder<S>) => void,
): Mission;
/**
 * 构造多人或遭遇战任务；请将返回值作为 mission.ts 的默认导出。
 * @param schema 从 map.generated.ts 导入的 map，用于检查单位、航点等资源枚举。
 * @param options 指定任务 id 和 mode: MissionMode.Multiplayer，不能指定固定 player。
 * 应用后设置 Basic/MultiplayerOnly=yes，保留原有 Basic/Player，不添加虚假的玩家 House。
 * @param build 构建时同步执行一次的回调，可用 m.playerAt() 选择某起点的人类或 AI。
 * 可以循环声明多个小队和规则；回调不能返回数据或 Promise。
 * @returns 可序列化的 Mission 描述；只有应用到地图并在游戏中触发规则后才会产生单位。
 * @throws 回调抛错或返回非 undefined 的值时失败；Rust 还会检查模式、引用和原生字段容量。
 * @remarks 使用遭遇战或联机地图方式运行。空起点不产生小队；winPlayer/losePlayer 仅支持单人模式。
 * @example
 * export default defineMission(map, { id: 'gift', mode: MissionMode.Multiplayer }, m => {
 *   const recipient = m.playerAt(Waypoint.WP0); // 占用 0 号起点的实际玩家
 *   // 将 recipient 传给 m.team() 的 house，再用 action.reinforce() 出兵。
 * });
 */
export function defineMission<const S extends MapSchema>(
  schema: S,
  options: MultiplayerOptions,
  build: (mission: MissionBuilder<S, MissionMode.Multiplayer>) => void,
): Mission;
export function defineMission<const S extends MapSchema>(
  schema: S,
  options: SinglePlayerOptions<S> | MultiplayerOptions,
  build: ((mission: MissionBuilder<S>) => void) | ((mission: MissionBuilder<S, MissionMode.Multiplayer>) => void),
): Mission {
  const mission = new MissionBuilder(schema, options.mode ?? MissionMode.SinglePlayer);
  if (options.mode !== MissionMode.Multiplayer) mission.house(options.player);
  // 公开重载保证回调与 options 的模式一致，此处统一执行同步构建流程。
  const result = (build as (builder: typeof mission) => void)(mission);
  if (result !== undefined) throw new Error("Mission construction must be synchronous and return no value");
  return mission.finish(options.id, options.player ?? "");
}

/** 触发条件工厂；只返回事件描述，必须传给 m.rule() 的 when 才会生成触发器。 */
export const event = {
  /**
   * 创建“自场景开始经过指定时间”的条件，所有规则共用场景起点。
   * @param seconds 游戏逻辑秒数，须为 0～143165576 的整数，不是现实时间计时器。
   * 游戏时间按每秒 15 逻辑帧解释，实际墙钟时长取决于游戏速度和暂停。
   * @returns Event 描述，不会暂停当前 JS 执行；范围由 Rust 构建阶段检查。
   * @example event.sinceScenarioStart(60) // 场景开始后第 60 个游戏秒
   */
  sinceScenarioStart: (seconds: number): Event => ({ kind: "scenarioTime", seconds }),
  /**
   * 创建“触发器启用后经过指定时间”的条件，计时起点属于游戏中的触发器。
   * @param seconds 游戏逻辑秒数，须为 0～143165576 的整数，按每秒 15 逻辑帧解释。
   * @returns Event 描述；不会从调用此 JS 方法的时刻计时，也不会阻塞脚本。
   * @remarks 当前 SDK 生成的触发器默认启用，且没有公开启用、禁用规则的动作。
   * 需要明确按场景开局安排波次时，使用 sinceScenarioStart()。
   * @example event.sinceEnabled(30)
   */
  sinceEnabled: (seconds: number): Event => ({ kind: "enabledTime", seconds }),
  /**
   * 创建“绑定的具体地图对象被摧毁”的条件。
   * @param object 通过 m.object() 取得的地图对象引用，不接受 m.unit() 的单位类型。
   * @returns Event 描述；应用任务时会将生成的 Tag 绑定到该对象。
   * @remarks 目标必须存在、记录无歧义且未绑定其他 Tag；同一对象只能绑定一条脚本规则。
   * 不表示“某类型的全部单位被摧毁”。这些限制在 Rust 构建阶段检查。
   * @example event.destroyed(m.object(MapObject.Structures_0))
   */
  destroyed: (object: MapObject): Event => ({ kind: "destroyed", object }),
  /**
   * 创建“局部标记为 true”的条件，检查状态而非仅监听 false 到 true 的变化。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Event 描述；若初始值就是 true，条件可以从任务开始时满足。
   * @example event.flagSet(objectiveDone)
   */
  flagSet: (flag: Flag): Event => ({ kind: "flagSet", flag }),
  /**
   * 创建“局部标记为 false”的条件，检查状态而非仅监听 true 到 false 的变化。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Event 描述；标记默认初始值为 false，使用默认值时条件可能开局即满足。
   * @example event.flagClear(alarm)
   */
  flagClear: (flag: Flag): Event => ({ kind: "flagClear", flag }),
};
/** 小队命令工厂；返回值放入 TeamOptions.orders，由游戏中的小队按行为脚本执行。 */
export const order = {
  /**
   * 创建移动到指定航点的命令。
   * @param waypoint 通过 m.waypoint() 取得的目标航点。
   * @returns Order 描述；不立即移动对象，也不承诺无视战斗或寻路状态。
   * @remarks 对应原生“移动到航点”，具体移动与完成判定由游戏的小队行为逻辑处理。
   * @example order.moveTo(m.waypoint(Waypoint.WP21))
   */
  moveTo: (waypoint: Waypoint): Order => ({ kind: "moveTo", waypoint }),
  /**
   * 创建原生“攻击航点”命令，以航点位置作为攻击目标。
   * @param waypoint 通过 m.waypoint() 取得的目标航点，不是对象引用。
   * @returns Order 描述；攻击、寻路及完成判定由游戏处理。
   * @remarks 此方法只生成攻击航点行为，不绑定具体建筑或保证清除该区域全部敌军。
   * @example order.attackWaypoint(m.waypoint(Waypoint.WP22))
   */
  attackWaypoint: (waypoint: Waypoint): Order => ({ kind: "attackWaypoint", waypoint }),
  /**
   * 创建原生“巡逻到航点”命令。
   * @param waypoint 通过 m.waypoint() 取得的目标航点。
   * @returns 一条巡逻 Order；沿途交战和完成条件由游戏处理。
   * @remarks 一次调用只生成一条行为，不会自动补齐往返路线或创建无限循环。
   * @example order.patrol(m.waypoint(Waypoint.WP21))
   */
  patrol: (waypoint: Waypoint): Order => ({ kind: "patrol", waypoint }),
  /**
   * 创建持续指定时间的原生警戒命令。
   * @param seconds 警戒的游戏逻辑秒数，须为 0～143165576 的整数，按每秒 15 逻辑帧解释。
   * @returns Order 描述；计时和后续命令推进由游戏处理，不会阻塞构建脚本。
   * @example order.guard(10)
   */
  guard: (seconds: number): Order => ({ kind: "guard", seconds }),
  /**
   * 创建原生“猎杀”命令，让小队进入由游戏 AI 控制的自主寻敌行为。
   * @returns 无需参数的 Order 描述；没有可供 JS 等待的完成通知。
   * @remarks 目标选择及持续行为由游戏决定，通常放在行为列表末尾；
   * 不保证该命令会结束并推进到下一条，也不会自动宣告任务胜利。
   * @example order.hunt()
   */
  hunt: (): Order => ({ kind: "hunt" }),
};
/** 规则动作工厂；只创建描述，放入 RuleOptions.actions 后才会在规则触发时执行。 */
export const action = {
  /**
   * 创建“在指定航点增援小队”的动作，使用已声明的小队模板。
   * @param team 本任务通过 m.team() 声明的小队，成员、归属和行为来自模板。
   * @param at 增援航点，通过 m.waypoint() 取得；该位置写入增援动作参数。
   * @returns Action 描述；调用本方法时不会生成单位。
   * @remarks 对应原生指定航点增援，入场方式、可用位置和组队限制由游戏处理。
   * 不会等待小队完成行为；复用同一模板仍受其原生 Max 等参数影响。
   * @example action.reinforce(assault, m.waypoint(Waypoint.WP20))
   */
  reinforce: (team: Team, at: Waypoint): Action => ({ kind: "reinforce", team, at }),
  /**
   * 创建原生“创建小队（招募）”动作，请求游戏依据模板组织小队。
   * @param team 本任务通过 m.team() 声明的小队。
   * @returns Action 描述；没有指定航点增援参数，也不返回游戏中的小队实例。
   * @remarks 招募来源、成员可用性和组队结果由游戏处理；需要从指定航点出兵时用 reinforce()。
   * @example action.recruit(assault)
   */
  recruit: (team: Team): Action => ({ kind: "recruit", team }),
  /**
   * 创建显示 CSF 消息的动作。
   * @param text 通过 m.text() 取得的已有标签引用，不接受直接输入的中文或其他文本原文。
   * @returns Action 描述；规则触发后由游戏查找标签并显示本地化内容。
   * @remarks 当前 API 不自定义显示时长、颜色或受众；标签及其资源须随任务正确发布。
   * @example action.message(m.text(Text.MISSION_INTRO))
   */
  message: (text: Text): Action => ({ kind: "message", text }),
  /**
   * 创建将局部标记置为 true 的动作，用于解锁其他规则条件。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Action 描述；修改发生在游戏中，不会改变构建阶段的 JS 变量。
   * @example action.setFlag(objectiveDone)
   */
  setFlag: (flag: Flag): Action => ({ kind: "setFlag", flag }),
  /**
   * 创建将局部标记置为 false 的动作，可让 event.flagClear() 条件满足。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Action 描述；不会重新启用已经执行完的一次性规则。
   * @example action.clearFlag(alarm)
   */
  clearFlag: (flag: Flag): Action => ({ kind: "clearFlag", flag }),
  /**
   * 创建宣告玩家胜利的原生动作。
   * @returns 无需参数的 Action 描述，在规则触发时交由游戏处理胜利流程。
   * @remarks 仅支持单人模式，用于 defineMission() 中配置的玩家任务流程，不提供指定其他 House 获胜的参数。
   * @example action.winPlayer()
   */
  winPlayer: (): Action => ({ kind: "winPlayer" }),
  /**
   * 创建宣告玩家失败的原生动作。
   * @returns 无需参数的 Action 描述，在规则触发时交由游戏处理失败流程。
   * @remarks 仅支持单人模式，用于 defineMission() 中配置的玩家任务流程；仅构造动作不会立即结束游戏。
   * @example action.losePlayer()
   */
  losePlayer: (): Action => ({ kind: "losePlayer" }),
};
