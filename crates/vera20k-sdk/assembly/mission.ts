/**
 * 面向地图任务的 AssemblyScript 便捷 API。
 *
 * 这里的 API 只是在引擎 TSSDK 之上提供更容易使用的中文注释和常用
 * 操作封装，不会复制或替代引擎 ABI。类型目录使用原版 RA2/YR 的 INI
 * 名称；Mod 自定义名称仍可直接传给 resolve。
 */
import {
  Action, Cell, Find, FindPage, frame, House, Mission, NO_HANDLE, Order, Ref,
  Rules, Status, Techno, TypeRef,
} from "@vera20k/sdk/assembly/core";
import { Every, onFrame } from "@vera20k/sdk/assembly/triggers";
import { log as engineLog } from "@vera20k/sdk/assembly/core";

/** 原版常见步兵、车辆、飞机与舰船的 INI 名称目录。 */
export namespace UnitType {
  /** 美国大兵（GI）的通用步兵别名。 */
  export const Rifleman: string = "E1";
  /** 动员兵。 */
  export const Conscript: string = "E2";
  /** 美国大兵。 */
  export const GI: string = "E1";
  /** 工程师。 */
  export const Engineer: string = "ENGINEER";
  /** 间谍。 */
  export const Spy: string = "SPY";
  /** 海豹突击队。 */
  export const Seal: string = "GHOST";
  /** 谭雅。 */
  export const Tanya: string = "TANY";
  /** 尤里改。 */
  export const YuriPrime: string = "YURIPR";
  /** 尤里新兵。 */
  export const Initiate: string = "INIT";
  /** 狂兽人。 */
  export const Brute: string = "BRUTE";
  /** 灰熊坦克。 */
  export const GrizzlyTank: string = "MTNK";
  /** 犀牛坦克。 */
  export const RhinoTank: string = "HTNK";
  /** 天启坦克。 */
  export const ApocalypseTank: string = "APOC";
  /** 防空履带车。 */
  export const FlakTrack: string = "HTK";
  /** 多功能步兵车。 */
  export const InfantryFightingVehicle: string = "FV";
  /** V3 火箭发射车。 */
  export const V3RocketLauncher: string = "V3";
  /** 恐怖机器人。 */
  export const TerrorDrone: string = "DRON";
  /** 狂风坦克。 */
  export const LasherTank: string = "LTNK";
  /** 磁电坦克。 */
  export const Magnetron: string = "TELE";
  /** 精神控制车。 */
  export const Mastermind: string = "MIND";
  /** 雷射幽浮。 */
  export const FloatingDisc: string = "DISK";
  /** 入侵者战机。 */
  export const Harrier: string = "ORCA";
  /** 黑鹰战机。 */
  export const BlackEagle: string = "BEAG";
  /** 火箭飞行兵。 */
  export const Rocketeer: string = "JUMPJET";
  /** 夜鹰直升机。 */
  export const NightHawk: string = "SHAD";
  /** 基洛夫空艇。 */
  export const Kirov: string = "ZEP";
  /** 无畏级战舰。 */
  export const Dreadnought: string = "DRED";
  /** 驱逐舰。 */
  export const Destroyer: string = "DEST";
  /** 神盾巡洋舰。 */
  export const AegisCruiser: string = "AEGIS";
  /** 海豚。 */
  export const Dolphin: string = "DLPH";
  /** 巨型乌贼。 */
  export const GiantSquid: string = "SQD";
  /** 台风级潜艇。 */
  export const AttackSubmarine: string = "SUB";

  /** 按当前规则集解析原版或 Mod 自定义 TechnoType；失败时返回 NO_HANDLE。 */
  export function resolve(name: string): TypeRef {
    return Rules.technoType(name);
  }
}

/** 原版常见盟军、苏联与尤里建筑的 INI 名称目录。 */
export namespace BuildingType {
  /** 盟军建造厂。 */
  export const AlliedConstructionYard: string = "GACNST";
  /** 发电厂。 */
  export const AlliedPowerPlant: string = "GAPOWR";
  /** 盟军兵营。 */
  export const AlliedBarracks: string = "GAPILE";
  /** 盟军战车工厂。 */
  export const AlliedWarFactory: string = "GAWEAP";
  /** 盟军矿石精炼厂。 */
  export const AlliedOreRefinery: string = "GAREFN";
  /** 盟军维修厂。 */
  export const AlliedServiceDepot: string = "GADEPT";
  /** 盟军作战实验室。 */
  export const AlliedBattleLab: string = "GATECH";
  /** 空指部。 */
  export const AlliedAirforceCommand: string = "GAAIRC";
  /** 间谍卫星。 */
  export const AlliedSpySat: string = "GASPYSAT";
  /** 苏军建造厂。 */
  export const SovietConstructionYard: string = "NACNST";
  /** 磁能反应炉。 */
  export const SovietPowerPlant: string = "NAPOWR";
  /** 苏军兵营。 */
  export const SovietBarracks: string = "NAHAND";
  /** 苏军战车工厂。 */
  export const SovietWarFactory: string = "NAWEAP";
  /** 苏军矿石精炼厂。 */
  export const SovietOreRefinery: string = "NAREFN";
  /** 苏军维修厂。 */
  export const SovietServiceDepot: string = "NADEPT";
  /** 苏军作战实验室。 */
  export const SovietBattleLab: string = "NATECH";
  /** 雷达。 */
  export const SovietRadar: string = "NARADR";
  /** 尤里建造厂。 */
  export const YuriConstructionYard: string = "YACNST";
  /** 生化反应炉。 */
  export const YuriBioReactor: string = "YAPOWR";
  /** 尤里兵营。 */
  export const YuriBarracks: string = "YABRCK";
  /** 尤里战车工厂。 */
  export const YuriWarFactory: string = "YAWEAP";
  /** 奴隶矿场；奴隶矿车（SMIN）的部署建筑形态，原版规则名 Yuri Ore Refinery。 */
  export const YuriOreRefinery: string = "YAREFN";
  /** 尤里作战实验室。 */
  export const YuriBattleLab: string = "YATECH";
  /** 盖特机炮。 */
  export const YuriGattlingCannon: string = "YAGGUN";

  /** 按当前规则集解析原版或 Mod 自定义建筑名；失败时返回 NO_HANDLE。 */
  export function resolve(name: string): TypeRef {
    return Rules.technoType(name);
  }
}

/** 单位常见操作；底层对象仍使用引擎统一的 Techno 引用。 */
export namespace Unit {
  /** 在指定格子为所属方创建类型；类型无效或创建失败时返回 NO_HANDLE。 */
  export function spawn(type: TypeRef, owner: House, at: Cell): Techno {
    if (type == NO_HANDLE) return NO_HANDLE;
    return Action.createObject(at, type, owner);
  }

  /** 先按名称解析，再创建单位；支持 Mod 自定义名称。 */
  export function spawnByName(name: string, owner: House, at: Cell): Techno {
    return spawn(UnitType.resolve(name), owner, at);
  }

  /** 设置单位任务，可选目标为 Techno、Cell 或 Waypoint；0 表示无目标。 */
  export function setMission(unit: Techno, mission: Mission, target: Ref = NO_HANDLE): Status {
    return Action.setMission(unit, mission, target);
  }

  /** 读取单位当前生命值；返回值单位为 HP。 */
  export function health(unit: Techno): i64 {
    return Techno.health(unit);
  }

  /** 直接设置单位生命值；返回引擎状态。 */
  export function setHealth(unit: Techno, health: i64): Status {
    return Techno.setHealth(unit, health);
  }

  /** 读取单位所属方。 */
  export function owner(unit: Techno): House {
    return Techno.owner(unit);
  }

  /** 读取单位当前任务。 */
  export function mission(unit: Techno): Mission {
    return Techno.mission(unit);
  }

  /** 读取单位的 TechnoType 引用。 */
  export function typeRef(unit: Techno): TypeRef {
    return Techno.typeRef(unit);
  }

  /** 恢复指定生命值；amount 的单位为 HP。 */
  export function heal(unit: Techno, amount: i32): Status {
    return Action.heal(unit, amount);
  }

  /** 更改单位所属方。 */
  export function setOwner(unit: Techno, owner: House): Status {
    return Techno.setOwner(unit, owner);
  }

  /** 查询指定类型、所属方和范围内的单位；结果写入 out，返回第一页信息。 */
  export function findNear(
    type: TypeRef,
    owner: House,
    center: Cell,
    radius: i32,
    out: StaticArray<Techno>,
  ): FindPage {
    return Find.objects().ofType(type).ownedBy(owner).near(center, radius).page(out, 0);
  }
}

/** 建筑常见操作；建筑与单位共用引擎 Techno 表示。 */
export namespace Building {
  /** 在指定格子为所属方放置类型；失败时返回 NO_HANDLE。 */
  export function place(type: TypeRef, owner: House, at: Cell): Techno {
    if (type == NO_HANDLE) return NO_HANDLE;
    return Action.createObject(at, type, owner);
  }

  /** 先按名称解析，再放置建筑；支持 Mod 自定义名称。 */
  export function placeByName(name: string, owner: House, at: Cell): Techno {
    return place(BuildingType.resolve(name), owner, at);
  }

  /** 设置建筑的在线状态。 */
  export function setOnline(building: Techno, online: bool): Status {
    return Techno.setOnline(building, online);
  }

  /** 读取建筑当前生命值；返回值单位为 HP。 */
  export function health(building: Techno): i64 {
    return Techno.health(building);
  }

  /** 直接设置建筑生命值；返回引擎状态。 */
  export function setHealth(building: Techno, health: i64): Status {
    return Techno.setHealth(building, health);
  }

  /** 读取建筑所属方。 */
  export function owner(building: Techno): House {
    return Techno.owner(building);
  }

  /** 读取建筑的 TechnoType 引用。 */
  export function typeRef(building: Techno): TypeRef {
    return Techno.typeRef(building);
  }

  /** 通过所属方的常规游戏命令出售建筑。 */
  export function sell(building: Techno): Status {
    const owner = Techno.owner(building);
    if (owner == NO_HANDLE) return Status.Gone;
    const units = new StaticArray<Techno>(1);
    unchecked((units[0] = building));
    return House.order(owner, Order.sell(units));
  }

  /** 查询指定类型、所属方和范围内的建筑；结果写入 out，返回第一页信息。 */
  export function findNear(
    type: TypeRef,
    owner: House,
    center: Cell,
    radius: i32,
    out: StaticArray<Techno>,
  ): FindPage {
    return Find.objects().ofType(type).ownedBy(owner).near(center, radius).page(out, 0);
  }
}

/** 可取消的模拟帧计时器句柄；它不是操作系统或浏览器的定时器 ID。 */
export type TimerId = i32;

/** 定时器回调；必须无捕获，可使用顶层函数或不捕获外部变量的回调。 */
export type TimerCallback = () => void;

const timerIds = new Array<i32>();
const timerReadyIndices = new Array<i32>();
const timerCallbacks = new Array<usize>();
const timerDueFrames = new Array<i64>();
const timerPeriods = new Array<i32>();
const timerRepeating = new Array<bool>();
let timerHookRegistered: bool = false;
let timerDispatching: bool = false;
let nextTimerId: i32 = 1;
const FIRST_TIMER_ID: i32 = 1;
const MAX_TIMER_ID: i32 = 2147483647;

function removeTimerAt(index: i32): void {
  if (timerDispatching) {
    // 派发期间只标记删除，使本帧保存的下标保持有效；末尾统一压缩。
    timerIds[index] = 0;
    return;
  }
  timerIds.splice(index, 1);
  timerCallbacks.splice(index, 1);
  timerDueFrames.splice(index, 1);
  timerPeriods.splice(index, 1);
  timerRepeating.splice(index, 1);
}

function timerExists(id: TimerId): bool {
  for (let i = 0; i < timerIds.length; i++) {
    if (unchecked(timerIds[i]) == id) return true;
  }
  return false;
}

function timerTick(): void {
  const now = <i64>frame();
  // 固定本帧到期顺序；派发期间不移动数组，避免逐项查找与删除的平方开销。
  timerReadyIndices.length = 0;
  for (let i = 0; i < timerIds.length; i++) {
    if (unchecked(timerDueFrames[i]) <= now) timerReadyIndices.push(i);
  }
  timerDispatching = true;
  for (let ready = 0; ready < timerReadyIndices.length; ready++) {
    const index = unchecked(timerReadyIndices[ready]);
    // 前面的回调可能已经取消了这个计时器。
    if (unchecked(timerIds[index]) == 0) continue;
    const callback = changetype<TimerCallback>(unchecked(timerCallbacks[index]));
    if (unchecked(timerRepeating[index])) {
      timerDueFrames[index] = now + <i64>unchecked(timerPeriods[index]);
    } else {
      removeTimerAt(index);
    }
    callback();
  }
  // 保留未取消项的注册顺序，也保留回调中新注册、需等下一帧的计时器。
  let live: i32 = 0;
  for (let i = 0; i < timerIds.length; i++) {
    if (unchecked(timerIds[i]) == 0) continue;
    if (live != i) {
      timerIds[live] = unchecked(timerIds[i]);
      timerCallbacks[live] = unchecked(timerCallbacks[i]);
      timerDueFrames[live] = unchecked(timerDueFrames[i]);
      timerPeriods[live] = unchecked(timerPeriods[i]);
      timerRepeating[live] = unchecked(timerRepeating[i]);
    }
    live++;
  }
  timerIds.length = live;
  timerCallbacks.length = live;
  timerDueFrames.length = live;
  timerPeriods.length = live;
  timerRepeating.length = live;
  timerDispatching = false;
}

function registerTimer(callback: TimerCallback, delayFrames: i32, repeating: bool): TimerId {
  if (!timerHookRegistered) {
    onFrame(new Every(1), timerTick);
    timerHookRegistered = true;
  }
  let id = nextTimerId;
  while (id <= 0 || timerExists(id)) id = id == MAX_TIMER_ID ? FIRST_TIMER_ID : id + 1;
  nextTimerId = id == MAX_TIMER_ID ? FIRST_TIMER_ID : id + 1;
  const period = max(delayFrames, 1);
  timerIds.push(id);
  timerCallbacks.push(changetype<usize>(callback));
  timerDueFrames.push(<i64>frame() + <i64>period);
  timerPeriods.push(period);
  timerRepeating.push(repeating);
  return id;
}

/**
 * 常见 console 调用的确定性映射：写入引擎任务日志，不连接浏览器控制台。
 *
 * 为保持 AssemblyScript/Wasm 的确定性，参数使用字符串，不实现浏览器的
 * 对象展开、占位符格式化和开发者工具交互。
 */
export namespace console {
  /** 将一条文本写入引擎任务日志。 */
  export function log(message: string): void {
    engineLog(message);
  }

  /** info 与 log 等价，用于兼容常见 JavaScript 写法。 */
  export function info(message: string): void {
    engineLog(message);
  }

  /** 以 debug 前缀写入引擎任务日志。 */
  export function debug(message: string): void {
    engineLog("[debug] " + message);
  }

  /** 以警告前缀写入引擎任务日志。 */
  export function warn(message: string): void {
    engineLog("[warn] " + message);
  }

  /** 以错误前缀写入引擎任务日志。 */
  export function error(message: string): void {
    engineLog("[error] " + message);
  }

  /** 条件为 false 时写入错误日志；条件为 true 时不产生输出。 */
  export function assert(condition: bool, message: string = "Assertion failed"): void {
    if (!condition) error(message);
  }
}

/**
 * 注册一次性计时器；delayFrames 是确定性的模拟帧数，不是墙上时钟毫秒。
 * 游戏默认每秒 15 帧，因此如需约 1 秒延迟可传入 15。
 * 零或负数延迟按一帧处理；回调必须无捕获，以符合 AssemblyScript 要求。
 * 回调中新建的计时器最早在下一模拟帧执行，取消其他计时器会立即生效。
 */
export function setTimeout(callback: TimerCallback, delayFrames: i32): TimerId {
  return registerTimer(callback, delayFrames, false);
}

/** 注册重复计时器；间隔至少一帧，回调必须无捕获。跳帧时只执行一次，不补跑。 */
export function setInterval(callback: TimerCallback, intervalFrames: i32): TimerId {
  return registerTimer(callback, intervalFrames, true);
}

/** 取消 setTimeout 或 setInterval 创建的计时器。 */
export function clearTimeout(id: TimerId): void {
  for (let i = 0; i < timerIds.length; i++) {
    if (unchecked(timerIds[i]) == id) {
      removeTimerAt(i);
      return;
    }
  }
}

/** clearInterval 是 clearTimeout 的别名；两者都可取消任一计时器。 */
export function clearInterval(id: TimerId): void {
  clearTimeout(id);
}
