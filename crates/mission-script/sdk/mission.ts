/** Mission SDK v2.
 * 任务脚本 SDK v2，兼容已有任务源码。
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
  readonly nativeDefinitions?: Readonly<Record<string,readonly string[]>>;
  readonly particleSystems?: readonly string[];
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
  /** 当前资源配置可引用的建筑类型；旧目录可省略，刷新类型后生成。 */
  readonly buildings?: readonly string[];
  /** 当前资源配置可引用的科技类型（含建筑、步兵、车辆和飞机）；旧目录可省略，刷新类型后生成。 */
  readonly technos?: readonly string[];
  /** 当前资源配置可引用的国家，区别于地图 House；旧目录可省略，刷新类型后生成。 */
  readonly countries?: readonly string[];
  /** 当前资源配置可引用的声音；旧目录可省略，刷新类型后生成。 */
  readonly sounds?: readonly string[];
  /** 当前资源配置可引用的EVA 语音；旧目录可省略，刷新类型后生成。 */
  readonly eva?: readonly string[];
  /** 当前资源配置可引用的音乐；旧目录可省略，刷新类型后生成。 */
  readonly themes?: readonly string[];
  /** 当前资源配置可引用的影片；旧目录可省略，刷新类型后生成。 */
  readonly movies?: readonly string[];
  /** 当前资源配置可引用的动画类型；旧目录可省略，刷新类型后生成。 */
  readonly animations?: readonly string[];
  /** 当前规则的 VoxelAnimType 目录，与普通 SHP 动画独立。 */
  readonly voxelAnims?: readonly string[];
  /** 当前规则按原生分配顺序注册的武器类型；爆炸动作读取其 Damage 和 Warhead。 */
  readonly weapons?: readonly string[];
  /** 当前资源配置可引用的超级武器类型；旧目录可省略，刷新类型后生成。 */
  readonly superWeapons?: readonly string[];
  /** 当前资源配置可引用的弹头类型；旧目录可省略，刷新类型后生成。 */
  readonly warheads?: readonly string[];
  /** 当前规则配置可用的 House 颜色方案。 */
  readonly houseColors?: readonly string[];
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
/** 可用的 House 地图颜色方案，由当前 Rules 的 [Colors] 生成。 */
export type HouseColor = NonNullable<MapSchema["houseColors"]>[number];
/** 占用指定玩家起点的人类或 AI，由多人任务中的 m.playerAt() 取得。 */
export type StartPlayer = Ref<"startPlayer", number>;
/** VERA 支持的玩家起点编号；还须在当前地图中定义。 */
export type PlayerStartId = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9
  | 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19
  | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 | 28 | 29;
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
/** 本任务定义的规则引用，由 m.rule()/m.ruleRef() 返回。 */
export type Rule = Ref<"rule">;
/** 显式 Tag，可由多个对象/格子/小队共享。 */
export type Tag = Ref<"tag">;
/** 可共享的特遣部队引用。 */
export type TaskForce = Ref<"taskForce">;
/** 可共享的行为脚本引用。 */
export type Script = Ref<"script">;
/** 当前任务定义的布尔局部标记引用，由 m.flag() 返回。 */
export type Flag = Ref<"flag">;
/** 已有 VariableNames 槽位的引用；不会重置初值或接管手工定义。 */
export type LocalFlag = Ref<"localFlag", number>;
/** 战役共享全局变量 0～49；引用不设置初值。 */
export type GlobalFlag = Ref<"globalFlag", number>;
/** 已加载的 CSF 标签引用，由 m.text() 取得，区别于直接显示的文本内容。 */
export type Text = Ref<"text">;
/** 建筑类型引用，由 m.building() 取得。 */
export type BuildingType = Ref<"building">;
/** 科技类型（含建筑、步兵、车辆和飞机）引用，由 m.techno() 取得。 */
export type TechnoType = Ref<"techno">;
/** 国家，区别于地图 House引用，由 m.country() 取得。 */
export type Country = Ref<"country">;
/** 当前活动触发器保留的事件所属方（原生 8997）；不是 authored owner，也不是事件 actor。 */
export type RetainedHouse = Ref<"retainedHouse">;
/** 支持原生动作所属方选择的接口，仅接受 Country、多人起点玩家或事件保留所属方。 */
export type ActionHouse = Country | StartPlayer | RetainedHouse;
/** 声音引用，由 m.sound() 取得。 */
export type Sound = Ref<"sound">;
/** EVA 语音引用，由 m.eva() 取得。 */
export type Eva = Ref<"eva">;
/** 音乐引用，由 m.theme() 取得。 */
export type Theme = Ref<"theme">;
/** 影片引用，由 m.movie() 取得。 */
export type Movie = Ref<"movie">;
/** 动画类型引用，由 m.animation() 取得。 */
export type AnimationType = Ref<"animation">;
/** 体素碎片动画类型，由 m.voxelAnim() 取得。 */
export type VoxelAnimType = Ref<"voxelAnim">;
/** 武器类型引用，由 m.weapon() 取得；与动画、弹头及超级武器分别编号。 */
export type WeaponType = Ref<"weapon">;
/** 原生粒子系统类型，不是单个 Particle。 */
export type ParticleSystemType = Ref<"particleSystem">;
/** 超级武器类型引用，由 m.superWeapon() 取得。 */
export type SuperWeaponType = Ref<"superWeapon">;
/** 弹头类型引用，由 m.warhead() 取得。 */
export type WarheadType = Ref<"warhead">;

/** 规则条件；all 遵循各原生事件的锁存/状态检查语义。 */
export type Event =
  | { readonly kind: "damagedByHouse"; readonly object: MapObject; readonly house: House }
  | { readonly kind: "buildingBuilt" | "buildingExists" | "buildingAbsent"; readonly building: BuildingType }
  | { readonly kind: "vehicleBuilt" | "infantryBuilt" | "aircraftBuilt" | "spyDisguisedAsInfantry"; readonly type: TechnoType }
  | { readonly kind: "randomTime"; readonly seconds: number }
  | { readonly kind: "creditsAtLeast" | "creditsAtMost"; readonly amount: number }
  | { readonly kind: "buildingsLost" | "unitsLost"; readonly count: number }
  | { readonly kind: "noFactories" | "anyCratePickedUp" }
  | { readonly kind: "ambientAtMost" | "ambientAtLeast"; readonly value: number }
  | { readonly kind: "countryDiscovered" | "countryUnitsDestroyed" | "countryBuildingsDestroyed" | "countryAllDestroyed" | "countryNavyDestroyed" | "countryLandDestroyed" | "lowPower" | "fullPower" | "crossedHorizontal" | "crossedVertical" | "enteredOrOverflown" | "spyDisguisedAsCountry"; readonly country: Country }
  | { readonly kind: "teamLeftMap"; readonly team: Team }
  | { readonly kind: "discovered" | "attacked" | "destroyedAny" | "selected" | "spotlightDetected" | "firstDamagedBySource" | "halfHealthBySource" | "redHealthBySource" | "firstDamaged" | "halfHealth" | "redHealth" | "destroyedOrRemoved" | "pickedUpCrate"; readonly object: MapObject }
  | { readonly kind: "all"; readonly events: readonly Event[] }
  | { readonly kind: "any"; readonly events: readonly Event[] }
  | { readonly kind: "scenarioTime"; readonly seconds: number }
  | { readonly kind: "enabledTime"; readonly seconds: number }
  | { readonly kind: "countdownElapsed" }
  | { readonly kind: "tagNotification" | "bridgeDestroyed" }
  | { readonly kind: "entered" | "enteredZone"; readonly country: Country }
  | { readonly kind: "nearWaypoint"; readonly object: MapObject; readonly waypoint: Waypoint }
  | { readonly kind: "technoExists"; readonly type: TechnoType; readonly count: number }
  | { readonly kind: "technoAbsent"; readonly type: TechnoType }
  | { readonly kind: "destroyed"; readonly object: MapObject }
  | { readonly kind: "flagSet" | "flagClear"; readonly flag: Flag }
  | { readonly kind: "localSet" | "localClear"; readonly flag: LocalFlag }
  | { readonly kind: "globalSet" | "globalClear"; readonly flag: GlobalFlag };

/** 小队行为脚本的一条命令，使用 order 中的方法构造，按数组顺序交给游戏执行。 */
export type Order =
  | { readonly kind: "startLightningStorm"; readonly frames: number }
  | { readonly kind: "followFriendlies" | "scout" | "scoutHouses" | "stopLightningStorm" | "fireSale" | "gatherAtEnemyBase" | "regroupAtBase" | "ironCurtain" | "enterGrinder" | "enterTankBunker" | "enterBioReactor" | "enterBattleBunker" | "enterNearestOccupiable" }
  | { readonly kind: "idleAnimation"; readonly animation: number }
  | { readonly kind: "centerCamera"; readonly velocity: CameraVelocity }
  | { readonly kind: "attackEnemyBuilding" | "moveToEnemyBuilding" | "moveToOwnBuilding" | "chronoshiftToBuilding"; readonly building: BuildingType; readonly mode: BuildingSelection }
  | { readonly kind: "playAnimation"; readonly animation: AnimationType; readonly loops: number }
  | { readonly kind: "talkBubble"; readonly bubble: number }
  | { readonly kind: "chronoshiftToQuarry"; readonly quarry: Quarry }
  | { readonly kind: "attackWaypointBuilding"; readonly waypoint: Waypoint }
  | { readonly kind: "attack"; readonly quarry: Quarry }
  | { readonly kind: "moveToCell"; readonly x: number; readonly y: number }
  | { readonly kind: "mission"; readonly mission: UnitMission }
  | { readonly kind: "face"; readonly direction: Direction }
  | { readonly kind: "unload"; readonly mode: UnloadMode }
  | { readonly kind: "spyWaypoint"; readonly waypoint: Waypoint }
  | { readonly kind: "label" | "jump"; readonly label: string }
  | { readonly kind: "waitForever" | "deploy" | "scatter" | "panic" | "calm" | "load" | "waitLoaded" | "next" | "succeed" | "killMembers" | "removeMembers" | "winPlayer" | "losePlayer" }
  | { readonly kind: "setFlag" | "clearFlag"; readonly flag: Flag }
  | { readonly kind: "setLocal" | "clearLocal"; readonly flag: LocalFlag }
  | { readonly kind: "setGlobal" | "clearGlobal"; readonly flag: GlobalFlag }
  | { readonly kind: "playSound"; readonly sound: Sound }
  | { readonly kind: "playMovie"; readonly movie: Movie }
  | { readonly kind: "playMusic"; readonly theme: Theme }
  | { readonly kind: "flash"; readonly frames: number }
  | { readonly kind: "changeCountry"; readonly country: Country }
  | { readonly kind: "changeScript"; readonly script: Script }
  | { readonly kind: "moveTo" | "attackWaypoint" | "patrol"; readonly waypoint: Waypoint }
  | { readonly kind: "guard"; readonly seconds: number }
  | { readonly kind: "hunt" };

/** 规则触发后执行的一个动作，使用 action 中的方法构造。 */
export type Action =
  | { readonly kind: "playVoxelAnimAt"; readonly animation: VoxelAnimType; readonly at: Waypoint }
  | { readonly kind: "setSpotlightBehavior"; readonly mode: 0|1|2|3 }
  | { readonly kind: "playAnimationAt"; readonly animation: AnimationType; readonly at: Waypoint }
  | { readonly kind: "explosionAt"; readonly weapon: WeaponType; readonly at: Waypoint }
  | { readonly kind: "createParticleSystem"; readonly system: ParticleSystemType; readonly at: Waypoint }
  | { readonly kind: "setAmbient" | "setLightingRed" | "setLightingGreen" | "setLightingBlue"; readonly percent: number }
  | { readonly kind: "setAmbientStep" | "setAmbientRate" | "setAiTriggerRatio" | "setAircraftRatio" | "setInfantryRatio" | "setVehicleRatio" | "setSuperTargetKind"; readonly value: number }
  | { readonly kind: "combatFlash"; readonly at: Waypoint; readonly strength: CombatFlash }
  | { readonly kind: "damageBridgeAt" | "reduceTiberiumAt" | "ionBlastAt" | "nukeAt" | "chemicalStrikeAt" | "markParticlesDone"; readonly at: Waypoint }
  | { readonly kind: "meteorShower"; readonly at: Waypoint; readonly intensity: MeteorIntensity }
  | { readonly kind: "setVeinGrowth" | "setTiberiumGrowth" | "setIceGrowth" | "setAutoBaseBuilding"; readonly enabled: boolean }
  | { readonly kind: "toggleTrainCrate" | "clearSmudges" | "endScenario" }
  | { readonly kind: "fireSale" | "enableAutocreate" | "makeAlly" | "makeEnemy" | "cheer"; readonly house: ActionHouse }
  | { readonly kind: "setTechLevel"; readonly type: TechnoType; readonly level: number }
  | { readonly kind: "deleteRule"; readonly rule: Rule }
  | { readonly kind: "deleteTag"; readonly tag: Tag }
  | { readonly kind: "playMovie"; readonly movie: Movie }
  | { readonly kind: "playRadarMovie"; readonly movie: Movie; readonly pause: boolean }
  | { readonly kind: "playSoundAt"; readonly sound: Sound; readonly at: Waypoint }
  | { readonly kind: "playSoundAtRandomWaypoint"; readonly sound: Sound }
  | { readonly kind: "stopSoundAt"; readonly at: Waypoint }
  | { readonly kind: "lockInput" | "unlockInput" | "clearTalkBubble" | "destroyAttached" | "sellAttachedBuildings" | "makeAttachedInfantryPsycho" | "wakeAttached" | "wakeOtherHouseSleepers" | "wakeHouseHarmless" | "releaseGarrison" | "clearSuperTarget" | "clearSuperDefense" | "stopLightningStorm" }
  | { readonly kind: "pauseScene" | "chronoScreen"; readonly duration: number }
  | { readonly kind: "talkBubble"; readonly team: Team; readonly bubble: number }
  | { readonly kind: "setAttachedBuildingsOnline"; readonly online: boolean }
  | { readonly kind: "wakeGroup"; readonly group: number }
  | { readonly kind: "createBuilding"; readonly building: BuildingType; readonly at: Waypoint }
  | { readonly kind: "destroyHouseObjects"; readonly house: ActionHouse; readonly subset: HouseObjectSubset }
  | { readonly kind: "controlHouseBuildings"; readonly house: ActionHouse; readonly restore: boolean }
  | { readonly kind: "restoreInitialObjects"; readonly house: ActionHouse; readonly buildingsOnly: boolean }
  | { readonly kind: "changeAllHouseObjectsOwner"; readonly house: ActionHouse }
  | { readonly kind: "flashHouseBuildings"; readonly building: BuildingType; readonly frames: number }
  | { readonly kind: "grantSuperWeapon"; readonly weapon: SuperWeaponType; readonly oneShot: boolean }
  | { readonly kind: "setSuperCharge"; readonly weapon: SuperWeaponType; readonly percent: number }
  | { readonly kind: "setSuperRecharge"; readonly weapon: SuperWeaponType; readonly frames: number }
  | { readonly kind: "resetSuperRecharge" | "resetSuperWeapon"; readonly weapon: SuperWeaponType }
  | { readonly kind: "setSuperTarget" | "setSuperDefense" | "lightningAt" | "lightningStormAt" | "chronoshiftHouse" | "ironCurtainAt"; readonly at: Waypoint }
  | { readonly kind: "startLightningStorm"; readonly frames: number }
  | { readonly kind: "centerCamera"; readonly at: Waypoint; readonly velocity: CameraVelocity }
  | { readonly kind: "jumpCamera" | "revealAt" | "revealZone" | "concealAt"; readonly at: Waypoint }
  | { readonly kind: "homeCamera" | "growShroud" | "reshroudMap" }
  | { readonly kind: "visibleMapArea"; readonly x: number; readonly y: number; readonly width: number; readonly height: number }
  | { readonly kind: "radarEvent"; readonly event: RadarEvent; readonly at: Waypoint }
  | { readonly kind: "sidebarTab"; readonly tab: SidebarTab }
  | { readonly kind: "flashCameo"; readonly type: TechnoType; readonly frames: number }
  | { readonly kind: "radarOutage"; readonly frames: number }
  | { readonly kind: "reinforce"; readonly team: Team; readonly at: Waypoint }
  | { readonly kind: "recruit"; readonly team: Team }
  | { readonly kind: "deleteTeam" | "reinforceDefault"; readonly team: Team }
  | { readonly kind: "chronoReinforce"; readonly team: Team; readonly at: Waypoint }
  | { readonly kind: "flashTeam"; readonly team: Team; readonly frames: number }
  | { readonly kind: "playSound"; readonly sound: Sound }
  | { readonly kind: "playMusic"; readonly theme: Theme }
  | { readonly kind: "playEva"; readonly eva: Eva }
  | { readonly kind: "revealMap" }
  | { readonly kind: "dropZone"; readonly at: Waypoint }
  | { readonly kind: "setAlternateBase"; readonly at: Waypoint }
  | { readonly kind: "clearAlternateBase" }
  | { readonly kind: "createCrate"; readonly crate: Crate; readonly at: Waypoint }
  | { readonly kind: "allToHunt" | "startProduction" | "enableAi" | "disableAi" | "changeAttachedOwner"; readonly house: ActionHouse }
  | { readonly kind: "winCountry" | "loseCountry"; readonly country: Country }
  | { readonly kind: "message"; readonly text: Text }
  | { readonly kind: "setCountdown" | "extendCountdown" | "shortenCountdown"; readonly seconds: number }
  | { readonly kind: "startCountdown" | "stopCountdown" }
  | { readonly kind: "setCountdownTitle"; readonly text: Text }
  | { readonly kind: "setFlag" | "clearFlag"; readonly flag: Flag }
  | { readonly kind: "setLocal" | "clearLocal"; readonly flag: LocalFlag }
  | { readonly kind: "setGlobal" | "clearGlobal"; readonly flag: GlobalFlag }
  | { readonly kind: "enableRule" | "disableRule" | "forceRule"; readonly rule: Rule }
  | { readonly kind: "winPlayer" | "losePlayer" };

/** 小队模板参数，定义归属、成员、集结航点和行为，不会立即在地图上生成单位。 */
export type TeamOptions = {
  /** 小队所属方；m.house() 使用该 House 的有效 Country，目标须为该国家的首个 House，否则构建报错。
   * m.playerAt() 指定开局占用某起点的玩家。
   */
  readonly house: House | StartPlayer;
  /** 小队模板的默认航点；指定航点增援时使用 action.reinforce() 的 at 参数。 */
  readonly waypoint: Waypoint;
  /** 新创建的小队及成员使用的 Tag；不会改写地图中已有对象。 */
  readonly tag?: Tag;
  /** 同时存在的小队模板实例上限；原生 Max，默认 1。 */
  readonly max?: number;
  /** AI 抢占/替换小队的优先级；原生 Priority，默认 5。 */
  readonly priority?: number;
  /** 新成员等级；默认 Regular。 */
  readonly veteranLevel?: VeteranLevel;
  /** 招募组编号；默认 -1。相同组允许互相招募。 */
  readonly group?: number;
  readonly autocreate?: boolean;
  /** 原生 Full，默认 true；增援时给合格运输成员预装载荷，并填满新建矿车的矿仓。
   * 预装集中到最后创建的合格运输成员，不按普通上车容量分配；应让成员数量适合运输类型。
   * 原版 PDPLANE 没有 Passengers 容量；步兵空投请使用 droppod。
   */
  readonly full?: boolean;
  /** 是否可作为其他小队的招募者；默认 false。 */
  readonly recruiter?: boolean;
  /** 是否为 AI 建造的基地防御小队；默认 false。 */
  readonly isBaseDefense?: boolean;
  readonly suicide?: boolean;
  readonly aggressive?: boolean;
  /** 成员不足时是否继续增援；默认 true。 */
  readonly reinforce?: boolean;
  /** 成员是否可被其他小队招募；默认 true。 */
  readonly areTeamMembersRecruitable?: boolean;
  readonly onTransOnly?: boolean;
  readonly avoidThreats?: boolean;
  readonly annoyance?: boolean;
  readonly guardSlower?: boolean;
  /** 卸载时运输成员优先离队，向已有 ArchiveTarget 移动并清除该记录。
   * load() 在仍有空位、已无待上车成员时记录当前位置；满载提前完成的分支不记录，不保证自动返回起点。
   */
  readonly transportsReturnOnUnload?: boolean;
  readonly mindControlDecision?: number;
  /** 普通增援使用 PDPLANE 投放已创建成员；运输机自行离场，成员继续执行小队脚本。 */
  readonly droppod?: boolean;
  readonly onlyTargetHouseEnemy?: boolean;
  readonly loadable?: boolean;
  readonly whiner?: boolean;
  /** Droppod 运输机使用 transportWaypoint 作为出生格；默认按所属 House 边缘选择。 */
  readonly useTransportOrigin?: boolean;
  /** 配合 useTransportOrigin 指定运输机出生航点，投放目标仍由小队/增援航点决定。 */
  readonly transportWaypoint?: Waypoint;
} & ({
  /** 内联成员；与 taskForce 二选一。1～6 种不同类型，数量须为正整数。 */
  readonly units: TaskForceOptions["units"];
  readonly taskForce?: never;
} | {
  /** 已声明的共享特遣部队，也可使用 m.taskForceRef() 前向引用。 */
  readonly taskForce: TaskForce;
  readonly units?: never;
}) & ({
  /** 内联行为；与 script 二选一。1～50 行，由游戏按顺序执行。 */
  readonly orders: readonly Order[];
  readonly script?: never;
} | {
  /** 已声明的共享脚本，也可使用 m.scriptRef() 前向引用。 */
  readonly script: Script;
  readonly orders?: never;
});
/** 单人建筑抽奖：购买、放置、拆毁后，在 rewardAt 发放随机车辆；不改普通箱子。 */
export interface CrateLotteryOptions {
  /** 使用普通可生产建筑（如 GAPILL），其整个类型改作补给箱；外观与图标沿用现有 ART。 */
  readonly ticket: BuildingType;
  readonly rewards: readonly (UnitType | RewardUnit)[];
  /** 奖励到达航点，与建筑放置位置独立；归任务 player。 */
  readonly rewardAt: Waypoint;
  /** 侧栏显示名称，默认“抽奖箱”。 */
  readonly name?: string;
  /** 原始 credits 价格；省略时沿用建筑原价，最终价格受游戏倍率影响。 */
  readonly cost?: number;
  /** 可选建造前置；省略时保留建筑原有前置条件。 */
  readonly prerequisites?: readonly BuildingType[];
  /** 补给箱建筑生命值，默认 20，须为正整数。 */
  readonly strength?: number;
}
/** 本任务专用的奖励车辆类型，只能放入抽奖池。不会替换普通车辆。 */
export type RewardUnit = Ref<"rewardUnit">;
/** 原生 RULES 单位字段；键名区分大小写。图片／模型等 ART 字段须由活动资源提供。 */
export type UnitRuleValue = string | number | boolean;
export interface RewardUnitOptions {
  readonly base: UnitType;
  readonly name?: string;
  /** 修改独立副本的原生单位属性。TechLevel/CrateGoodie/AllowedToStartInMultiplayer/UIName 由 SDK 管理。 */
  readonly rules: Readonly<Record<string, UnitRuleValue>>;
}
export type RewardUnitDefinition = Omit<RewardUnitOptions, "name"> & { readonly id: string; readonly uiName?: Text };
export type CrateLottery = BuildingType;
export type CrateLotteryDefinition = Omit<CrateLotteryOptions, "name"> & { readonly uiName: Text };
/** 共享成员清单，不会立刻生成游戏单位。 */
export interface TaskForceOptions {
  /** 1～6 种不同单位；数量为 1～2147483647 的整数。 */
  readonly units: readonly { readonly type: UnitType; readonly count: number }[];
  /** 原生 Group，默认为 -1。 */
  readonly group?: number;
}
/** 一条触发规则的参数。数量、数值范围和原生字段容量会在 Rust 构建阶段检查。 */
export interface RuleOptions {
  /** 一个触发条件，由 event 方法构造。 */
  readonly when: Event;
  /** 条件满足时按列表顺序执行的 1～64 个动作；过长的原生字段仍可能被拒绝。 */
  readonly actions: readonly Action[];
  /** true 使用一次性 Tag；false 使用原生重复 Tag。每隔 N 秒执行时搭配 event.sinceEnabled(N)。
   * 对象受伤等通知在重复 Tag 中会锁存，后续其他通知也可能再次执行；只处理一次时可在动作末尾 disableRule 自身。
   */
  readonly once: boolean;
  /** 初始启用，默认 true。重新启用会重置 sinceEnabled 计时，不能复活已删除的一次性规则。 */
  readonly enabled?: boolean;
  /** 原生 Trigger 所属国家；运行时选择该国家的第一个 House，不接受任意 House/起点玩家。 */
  readonly owner?: Country;
  /** 启用难度，默认全部。空列表表示所有难度均不启用，强制触发也不能绕过禁用状态。 */
  readonly difficulties?: readonly Difficulty[];
  /** 共享 Tag 链中的下一个规则；不是先后执行或条件完成后的跳转。链内 once 必须一致。
   * once=true 的任一规则满足后可解绑整个 Tag；同一对象的多种通知各处理一次，可用 once=false 并分别禁用自身。
   */
  readonly linked?: Rule;
  /** 被工程师占领时允许转移触发器保留所属方，默认 false。需对象 Tag 绑定。 */
  readonly transferOnCapture?: boolean;
}
/** 原生场景难度；只控制规则准入。 */
export enum Difficulty { Easy = "easy", Normal = "normal", Hard = "hard" }
/** 一次性阶段，默认禁用；完成后可启用下一个阶段。 */
export type StageOptions = Omit<RuleOptions, "once"> & { readonly next?: Rule };
/** 原生 Quarry 目标类别，具体准入仍受武器和敌我筛选限制。 */
export enum Quarry { None = 0, Anything = 1, Buildings = 2, Harvesters = 3, Infantry = 4, Vehicles = 5,
  Factories = 6, BaseDefenses = 7, BaseThreats = 8, Power = 9, OccupiableBuildings = 10, TechBuildings = 11 }
/** 原生 Mission 编号；各成员按自身类型执行。QMove/Ambush/AttackMove 为保留的原生空闲分支。 */
export enum UnitMission { Sleep = 0, Attack = 1, Move = 2, QMove = 3, Retreat = 4, Guard = 5, Sticky = 6,
  Enter = 7, Capture = 8, Eaten = 9, Harvest = 10, AreaGuard = 11, Return = 12, Stop = 13, Ambush = 14,
  Hunt = 15, Unload = 16, Sabotage = 17, Construction = 18, Selling = 19, Repair = 20, Rescue = 21,
  Missile = 22, Harmless = 23, Open = 24, Patrol = 25, ParadropApproach = 26, ParadropOverfly = 27,
  Wait = 28, AttackMove = 29, SpyplaneApproach = 30, SpyplaneOverfly = 31 }
/** 成员朝向的原生八方向。 */
export enum Direction { North = 0, NorthEast = 1, East = 2, SouthEast = 3, South = 4, SouthWest = 5, West = 6, NorthWest = 7 }
/** 卸载完成后从 Team 中解除哪些成员；不销毁单位。模板 TransportsReturnOnUnload 对运输成员有优先路径。
 * ReleaseAll 使小队变空时会清理小队，后续脚本行不执行。
 */
export enum UnloadMode { KeepAll = 0, ReleasePassengers = 1, ReleaseTransports = 2, ReleaseAll = 3 }
/** 原生 Tag 重复方式。
 * Once 首次满足便执行并解绑整个共享 Tag；其他附着者之后不再触发它。
 * AllAttachments 逐次解绑，最后一个引用才执行动作。
 * Repeat 保留共享 Tag；一个附着对象消失后，其他附着者仍可触发。
 */
export enum TagRepeat { Once = 0, AllAttachments = 1, Repeat = 2 }
/** 原生 Powerups 编号；具体奖励由活动规则决定。 */
/** 原生雷达提示种类；可见性、去重及播放由客户端处理。 */
export enum RadarEvent {
  Combat=0, Noncombat=1, Dropzone=2, BaseUnderAttack=3, HarvesterUnderAttack=4,
  EnemyObjectSensed=5, UnitReady=6, UnitLost=7, UnitRepaired=8, SpyInfiltration=9,
  BuildingCaptured=10, BeaconPlaced=11, ConstructionComplete=12, ImpactSilent=13,
  BridgeRepaired=14, StructureAbandoned=15, AllyUnderAttack=16,
}
/** 原生 House 对象销毁集合。Land 包含原生陆军集合，不含海军。 */
export enum HouseObjectSubset { All=0, Buildings=1, Land=2, Naval=3 }
/** 原生 C4 战斗光照伤害档位；实际闪光取决于弹头 Bright 等属性。 */
export enum CombatFlash { Small=0, Medium=1, Large=2 }
/** 建筑选择模式。威胁读取成员所属 House 在建筑中心格保留的区域威胁；距离比较对象 Location，不是屏幕距离。相同评分保留先遍历的建筑。 */
export enum BuildingSelection { LeastThreat=0, MostThreat=1, Nearest=2, Farthest=3 }
/** 原生侧栏分类；目标页必须有可用项目才能切换。 */
export enum SidebarTab { Buildings=0, Defenses=1, Infantry=2, Vehicles=3 }
/** 原生镜头平移速度档位，0 最慢、4 最快；不是持续秒数。 */
export type CameraVelocity = 0 | 1 | 2 | 3 | 4;
/** 陨石雨四档强度，数量基数分别为1、5、9、15，实际数量还受场景随机数影响。 */
export type MeteorIntensity = 0 | 1 | 2 | 3;
export enum Crate {
  Money = 0, Unit = 1, HealBase = 2, Cloak = 3, Explosion = 4, Napalm = 5,
  Squad = 6, Darkness = 7, Reveal = 8, Armor = 9, Speed = 10, Firepower = 11,
  Icbm = 12, Invulnerability = 13, Veteran = 14,
  IonStorm = 15, Gas = 16, Tiberium = 17, Pod = 18, Random = 20,
}
export interface TagOptions {
  /** 链首规则；同一条关联规则链只声明一个显式 Tag。 */
  readonly rule: Rule;
  /** 默认由规则 once 推导；Once/AllAttachments 要求 once:true，Repeat 要求 once:false。 */
  readonly repeat?: TagRepeat;
}
/** House 的国家、开局参数与单向外交字段。未指定的字段沿用地图或原生默认。 */
export interface HouseSettings<C extends string = string> {
  readonly country?: Country;
  readonly color?: C;
  /** 地图 House 显示名，CSF 标签最多 31 ASCII 字节，显示内容截取前 20 个 UTF-16 单元。多人参赛者仍使用房间名称。 */
  readonly uiName?: Text;
  /** 地图 House 增援边缘；不会改写按起点计算的所属边缘，显式运输航点仍优先。 */
  readonly edge?: HouseEdge;
  /** PlayerControl 开局提示；不等同于运行时是否有人类控制。 */
  readonly playerControl?: boolean;
  /** 初始资金的地图单位，每单位为 100 游戏资金；默认 100。 */
  readonly credits?: number;
  /** House IQ；引擎还会按 Rules 的最高 IQ 级别处理。 */
  readonly iq?: number;
  /** House 初始科技等级；默认 10。 */
  readonly techLevel?: number;
  /** 单向 Allies 清单；不自动修改被列 House 的反向列表。 */
  readonly allies?: readonly House[];
  /** 显式替换原生 AI 基地计划；不指定时保留地图现有计划。 */
  readonly basePlan?: BasePlanOptions;
}
/** 原生 House/Edge；Automatic 恢复 -1。Air 被读取为 4，飞机入场路径回退到所属方起点边缘。 */
export enum HouseEdge { Automatic = "automatic", North = "north", East = "east", South = "south", West = "west", Air = "air" }
/** 原生 BaseClass 控制点：墙防御、暂停计划或扫描基地周边。 */
export enum BasePlanControl { WallDefense = -1, Hold = -2, ScanPerimeter = -3 }
export type BasePlanNode =
  | { readonly kind: "building"; readonly building: BuildingType; readonly x: number; readonly y: number;
      /** 修改同 House/类型/坐标的唯一已有建筑重建位；找不到唯一建筑时报错。 */
      readonly rebuild?: boolean }
  | { readonly kind: "control"; readonly control: BasePlanControl; readonly x: number; readonly y: number };
export interface BasePlanOptions {
  /** 按 AI 处理顺序排列的建筑/控制点。 */
  readonly nodes: readonly BasePlanNode[];
  /** 当前 House 基地预建百分比，原生有效范围 0～100。 */
  readonly percentBuilt?: number;
}
/** AITriggerTypes 原生条件编号。 */
export enum AiTriggerCondition {
  Always = -1, EnemyOwnsObjects = 0, OwnsObjects = 1, EnemyPowerBelow100 = 2,
  EnemyLowPower = 3, EnemyCredits = 4, IronCurtainAlmostReady = 5,
  ChronosphereAlmostReady = 6, CivilianOwnsObjects = 7,
}
/** AITriggerTypes 比较运算原生编号。 */
export enum AiTriggerComparison { Less = 0, LessOrEqual = 1, Equal = 2, GreaterOrEqual = 3, Greater = 4, NotEqual = 5 }
export interface AiTriggerOptions {
  readonly primary: Team;
  readonly owner?: Country | "all";
  readonly condition: AiTriggerCondition;
  readonly objectType?: TechnoType;
  readonly amount?: number;
  readonly comparison?: AiTriggerComparison;
  readonly weight?: number;
  readonly minWeight?: number;
  readonly maxWeight?: number;
  readonly multiplayer?: boolean;
  /** 原生字段 12：0 表示不限制，1～3 表示游戏内部侧别 0～2。 */
  readonly side?: 0 | 1 | 2 | 3;
  readonly secondary?: Team;
  readonly difficulties?: readonly Difficulty[];
  readonly enabled?: boolean;
}
/** Basic 中可声明的开局参数和媒体引用。HomeCell 使用数字航点编号。 */
export interface MissionBasicOptions {
  readonly name?: string;
  readonly author?: string;
  readonly homeCell?: Waypoint;
  readonly altHomeCell?: Waypoint;
  readonly intro?: Movie;
  readonly brief?: Movie;
  readonly action?: Movie;
  readonly theme?: Theme;
  readonly win?: Movie;
  readonly lose?: Movie;
  readonly postScore?: Movie;
  readonly preMapSelect?: Movie;
  readonly endOfGame?: boolean;
  readonly oneTimeOnly?: boolean;
  readonly skipScore?: boolean;
  readonly ignoreGlobalAiTriggers?: boolean;
  readonly freeRadar?: boolean;
  readonly fillSilos?: boolean;
  readonly tiberiumGrowthEnabled?: boolean;
  readonly iceGrowthEnabled?: boolean;
  readonly veinGrowthEnabled?: boolean;
  readonly trainCrate?: boolean;
  readonly truckCrate?: boolean;
  readonly timerInherit?: boolean;
  /** 接收关继承资金比例，0～100 百分比，写为原生百分比格式。 */
  readonly carryOverMoneyPercent?: number;
  /** 接收关继承资金上限；-1 表示不限。 */
  readonly carryOverCap?: number;
  readonly skipMapSelect?: boolean;
  /** 原生后续关文件名，不含路径；发布时仍需战役目录定义该关。 */
  readonly nextScenario?: string;
  readonly altNextScenario?: string;
}
export interface WaypointOptions { readonly id: number; readonly x: number; readonly y: number }
/** 开局光照分量，使用地图比例值：1 表示 100%，允许负值和大于 1 的亮度。 */
export interface MissionLightingProfile {
  readonly ambient?: number;
  readonly red?: number;
  readonly green?: number;
  readonly blue?: number;
  /** 地面亮度修正；地图默认 0.05，原生内部以千分之一计。 */
  readonly ground?: number;
  /** 每级高度的亮度修正；地图默认 0.008，原生内部以千分之一计。 */
  readonly level?: number;
}
/** 初始 Lighting；两种任务模式均可用，未声明字段保留地图现值。 */
export interface MissionLightingOptions {
  readonly normal?: MissionLightingProfile;
  /** 闪电风暴期间使用的光照，不会自行启动风暴。 */
  readonly ion?: MissionLightingProfile;
  /** 心灵控制器期间使用的光照，不会自行启动超武。 */
  readonly dominator?: MissionLightingProfile;
  /** 原生地图比例值，乘 1000 后截断为逻辑帧间隔；0.001 表示 1 帧，须至少产生 1 帧。 */
  readonly dominatorAmbientChangeRate?: number;
  /** 核爆闪光淡变间隔，正整数逻辑帧；与上一个字段的单位不同。 */
  readonly nukeAmbientChangeRate?: number;
}
/** 单人地图的初始场景标志；多人模式由房间设置控制这些标志。 */
export interface CampaignSpecialFlagsOptions {
  /** 冻结直接和范围伤害。 */
  readonly inert?: boolean;
  /** 启用半透明战争迷雾；未探索黑幕由游戏自身另行控制。 */
  readonly fogOfWar?: boolean;
  /** 自动选敌排除 Rules/HarvesterUnit 类型；不阻止对矿车的直接伤害。 */
  readonly harvesterImmune?: boolean;
  readonly destroyableBridges?: boolean;
  /** 加速矿石密度增长的调度（Growth×0.3）；false 使用普通速度，并非关闭增长。 */
  readonly tiberiumGrows?: boolean;
  readonly tiberiumSpreads?: boolean;
}
/** 多人/遭遇战的地图开局标志，仅影响随机起始部队，不提升地图预放对象。 */
export interface MultiplayerSpecialFlagsOptions {
  /** 成功放置的非 MCV 起始单位直接为精英。 */
  readonly initialVeteran?: boolean;
}
export type MissionSpecialFlagsOptions<M extends MissionMode = MissionMode> =
  M extends MissionMode.SinglePlayer ? CampaignSpecialFlagsOptions : MultiplayerSpecialFlagsOptions;
/** 单人结算页；省略的字段保留地图现值。标题/评语引用 CSF，标签最多 30 ASCII 字节。 */
export interface MissionRankingOptions {
  /** 各难度基准时间，单位秒，整数 0～359999（99:59:59）；normal 写入 ParTimeMedium。 */
  readonly parTime?: { readonly easy?: number; readonly normal?: number; readonly hard?: number };
  /** 用时小于或等于基准时使用的标题和评语。 */
  readonly underParTitle?: Text;
  readonly underParMessage?: Text;
  /** 用时大于基准时使用的标题和评语。 */
  readonly overParTitle?: Text;
  readonly overParMessage?: Text;
}
export interface StartOwnerOptions { readonly object: MapObject; readonly player: number }
/** 新 House 必须指定现有 Country；同任务 House ID 唯一。 */
export type NewHouseOptions<C extends string = string> = HouseSettings<C> & { readonly country: Country };
/** defineMission() 返回的可序列化任务描述，应作为 mission.ts 的默认导出。 */
export interface Mission {
  readonly rewardUnits?: readonly RewardUnitDefinition[];
  readonly crateLottery?: CrateLotteryDefinition;
  /** 新 SDK 输出版本 2；Rust 同时接受旧 v1 数据。 */
  readonly version: 1 | 2;
  /** 任务逻辑名称，参与生成稳定的原生记录 ID。 */
  readonly id: string;
  /** 单人或多人模式，决定原生地图运行方式。 */
  readonly mode: MissionMode;
  /** 单人玩家 House ID，写入 Basic/Player；多人模式为空字符串，不覆盖该字段。 */
  readonly player: string;
  /** 本任务创建或配置的 House；旧任务数据可省略。 */
  readonly houseDefinitions?: readonly ({ readonly id: string; readonly create: boolean } & HouseSettings)[];
  /** 本任务声明的 AI 小队选择器。 */
  readonly aiTriggers?: readonly ({ readonly id: string } & AiTriggerOptions)[];
  readonly basic?: MissionBasicOptions;
  readonly specialFlags?: MissionSpecialFlagsOptions;
  readonly lighting?: MissionLightingOptions;
  readonly ranking?: MissionRankingOptions;
  /** 本任务新增的普通任务航点；不覆盖现有航点或 0～29 玩家起点。 */
  readonly waypoints?: readonly WaypointOptions[];
  /** 开局按实际玩家起点归属的对象；通过原生一次性触发器执行转移。 */
  readonly startOwners?: readonly StartOwnerOptions[];
  /** 相对任务入口目录的媒体依赖，宿主在试玩/发布前读取和检查。 */
  readonly assets?: readonly { readonly name: string; readonly source: string }[];
  readonly nativeDefinitions?: readonly {readonly kind: string; readonly id: string; readonly native: string}[];
  readonly aliases?: readonly {readonly kind: string; readonly id: string; readonly target: string}[];
  readonly origins?: readonly {readonly kind: string; readonly id: string; readonly module: string}[];
  readonly texts?: readonly { readonly id: string; readonly label: string; readonly value: string }[];
  /** 本任务声明的小队模板。 */
  readonly teams: readonly (TeamOptions & { readonly id: string })[];
  /** 本任务声明的共享成员清单；旧数据可省略。 */
  readonly taskForces?: readonly (TaskForceOptions & { readonly id: string })[];
  /** 本任务声明的独立脚本；旧数据可省略。 */
  readonly scripts?: readonly { readonly id: string; readonly orders: readonly Order[] }[];
  readonly tags?: readonly (TagOptions & { readonly id: string })[];
  readonly bindings?: readonly (
    | { readonly kind: "object"; readonly object: MapObject; readonly tag: Tag }
    | { readonly kind: "cell"; readonly x: number; readonly y: number; readonly tag: Tag }
  )[];
  /** 本任务声明的局部标记及其游戏初始值。 */
  readonly flags: readonly { readonly id: string; readonly initial: boolean }[];
  /** 本任务声明的触发规则。 */
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
  private crateLotteryRules?: CrateLotteryDefinition;
  private readonly rewardUnits: RewardUnitDefinition[] = [];
  private readonly nativeDefinitions: NonNullable<Mission["nativeDefinitions"]>[number][] = [];
  private readonly aliases: NonNullable<Mission["aliases"]>[number][] = [];
  private readonly origins: NonNullable<Mission["origins"]>[number][] = [];
  private sourceModule: string | undefined;
  private readonly teams: (TeamOptions & { id: string })[] = [];
  private readonly houseDefinitions: ({ id: string; create: boolean } & HouseSettings)[] = [];
  private readonly aiTriggers: ({ id: string } & AiTriggerOptions)[] = [];
  private basic?: MissionBasicOptions;
  private specialFlags?: MissionSpecialFlagsOptions;
  private lighting?: MissionLightingOptions;
  private ranking?: MissionRankingOptions;
  private readonly waypoints: WaypointOptions[] = [];
  private readonly startOwners: StartOwnerOptions[] = [];
  private readonly assets: { name: string; source: string }[] = [];
  private readonly texts: NonNullable<Mission["texts"]>[number][] = [];
  private readonly taskForces: (TaskForceOptions & { id: string })[] = [];
  private readonly scripts: { id: string; orders: readonly Order[] }[] = [];
  private readonly tags: (TagOptions & { id: string })[] = [];
  private readonly bindings: NonNullable<Mission["bindings"]>[number][] = [];
  private readonly flags: { id: string; initial: boolean }[] = [];
  private readonly rules: (RuleOptions & { id: string })[] = [];

  /**
   * 创建绑定到地图目录的构建器；一般由 defineMission() 自动调用。
   * @param schema 从 map.generated.ts 导入的 map，保留 as const 字面量类型以启用引用检查。
   * @param mode 由 defineMission() 传入的地图模式；默认单人模式。
   */
  constructor(private readonly schema: S, private readonly mode: M = MissionMode.SinglePlayer as M, private readonly missionId = "") {}

  /** 声明中文文本，返回可直接传给 message/倒计时标题的 Text。ID 决定稳定标签；不会在构建时写外部文件。 */
  defineText(id: string, value: string): Text {
    if (!this.missionId) throw new Error("defineText requires a mission ID; use defineMission or pass the ID to MissionBuilder");
    let hash = 0x811c9dc5;
    for (const character of `${this.missionId}/${id}`) hash = Math.imul(hash ^ character.charCodeAt(0),0x01000193) >>> 0;
    const label = `TSTX${("00000000"+hash.toString(16).toUpperCase()).slice(-8)}`;
    this.texts.push({id,label,value});
    return ref("text",label);
  }

  /** 声明独立数据文件。source 相对任务入口目录，不能越界；只声明，不在 JS 中访问磁盘。
   * 影片字幕可声明 subtitlemd.txt（UTF-16LE，含 BOM）；其他任意 .txt 文件不接受。
   */
  asset(name: string, source: string): void { this.assets.push({name, source}); }
  /** 引用 asset 声明的 soundmd.ini 中的声音；宿主解析发布目录并在 Rust 再验证。 */
  assetSound(id: string): Sound {return ref("sound",id);}
  /** 引用 asset 声明的 evamd.ini 中的 EVA。 */
  assetEva(id: string): Eva {return ref("eva",id);}
  /** 引用 asset 声明的 thememd.ini 中的音乐。 */
  assetTheme(id: string): Theme {return ref("theme",id); }

  /** 为模块内声明的项目记录来源路径；模块路径仅用于定位，无文件访问。 */
  source(module: string, build: () => void): void {
    const previous=this.sourceModule;this.sourceModule=module;
    try { build(); } finally { this.sourceModule=previous; }
  }
  private origin(kind: string,id: string): void { if(this.sourceModule!==undefined) this.origins.push({kind,id,module:this.sourceModule}); }
  private nativeRef<K extends "team"|"script"|"taskForce"|"rule"|"tag">(kind: K, native: string, id: string): Ref<K> {
    this.nativeDefinitions.push({kind,id,native});return ref(kind,id);
  }
  /** 引用已有 TeamType，保留手工/固定 AIMD 定义的所有权。Rust 检查存在和类别。 */
  nativeTeam(native: string, alias = "native_"+native): Team {return this.nativeRef("team",native,alias);}
  /** 引用已有 ScriptType；changeScript 仍按最终注册顺序编码。 */
  nativeScript(native: string, alias = "native_"+native): Script {return this.nativeRef("script",native,alias);}
  /** 引用已有 TaskForce，不复制或接管定义。 */
  nativeTaskForce(native: string, alias = "native_"+native): TaskForce {return this.nativeRef("taskForce",native,alias);}
  /** 引用地图已有规则，可启停/删除；已有规则的 force 递归预算无法验证，构建会拒绝。 */
  nativeRule(native: string, alias = "native_"+native): Rule {return this.nativeRef("rule",native,alias);}
  /** 引用地图已有 Tag，允许显式绑定；不接管原定义。 */
  nativeTag(native: string, alias = "native_"+native): Tag {return this.nativeRef("tag",native,alias);}
  /** 为对象记录取逻辑名；重建时检查原记录指纹，重排有歧义时要求重新选择。 */
  aliasObject(id: string, object: MapObject): MapObject {this.aliases.push({kind:"object",id,target:object.id});return object;}
  /** 为已有航点取逻辑名；重建时检查原坐标。 */
  aliasWaypoint(id: string, waypoint: Waypoint): Waypoint {this.aliases.push({kind:"waypoint",id,target:String(waypoint.id)});return waypoint;}

  /** 前向引用本任务的小队。不会创建模板；构建时须有同名 m.team() 定义。 */
  teamRef(id: string): Team { return ref("team", id); }
  /** 前向引用本任务的规则。不会创建规则；构建时须有同名 m.rule() 定义。 */
  ruleRef(id: string): Rule { return ref("rule", id); }
  /** 前向引用显式 Tag；构建时必须有同名 m.tag 定义。 */
  tagRef(id: string): Tag { return ref("tag", id); }
  /** 声明一个共享 Tag；规则须是关联链首，重复方式与链内 once 一致。 */
  tag(id: string, options: TagOptions): Tag {
    this.origin("tag", id);
    this.tags.push({ ...options, id }); return ref("tag", id);
  }
  /** 绑定已有对象；拒绝覆盖手工 Tag，同一对象只能有一个 Tag。 */
  bindObject(object: MapObject, tag: Tag): void { this.bindings.push({ kind: "object", object, tag }); }
  /** 绑定地图格子；按当前 NewINIFormat 编码，坐标和已有 CellTag 在 Rust 中检查。 */
  bindCell(x: number, y: number, tag: Tag): void { this.bindings.push({ kind: "cell", x, y, tag }); }
  /** 前向引用本任务的共享特遣部队；实际定义可在后面或相对导入模块中。 */
  taskForceRef(id: string): TaskForce { return ref("taskForce", id); }
  /** 前向引用本任务的共享脚本；实际定义可在后面或相对导入模块中。 */
  scriptRef(id: string): Script { return ref("script", id); }

  /** 引用已有局部变量 0～99；须有 VariableNames 定义，不修改其初值。 */
  localFlag(index: number): LocalFlag {
    if (!Number.isInteger(index) || index < 0 || index > 99) throw new Error("局部变量编号须为 0～99");
    return ref("localFlag", index);
  }
  /** 引用战役全局变量 0～49；本任务不写入或重置 GlobalVariables。 */
  globalFlag(index: number): GlobalFlag {
    if (!Number.isInteger(index) || index < 0 || index > 49) throw new Error("全局变量编号须为 0～49");
    return ref("globalFlag", index);
  }
  /** 选择原生 8997 保留 House；仅用于允许此选择器的动作，尚未保留事件 House 时动作不会产生效果。 */
  retainedHouse(): RetainedHouse { return ref("retainedHouse", "event"); }

  /** 声明可被多个小队共享的成员清单；同任务内 ID 不得重复，最多 256 个定义。 */
  taskForce(id: string, options: TaskForceOptions): TaskForce {
    this.origin("taskForce", id);
    this.taskForces.push({ ...options, id });
    return ref("taskForce", id);
  }
  /** 声明可被多个小队共享的 1～50 行行为脚本，最多 256 个定义。 */
  script(id: string, orders: readonly Order[]): Script {
    this.origin("script", id);
    this.scripts.push({ id, orders });
    return ref("script", id);
  }

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
  /** 供向前引用本任务稍后声明的 House，或声明单向外交关系。 */
  houseRef(id: string): House { return ref("house", id); }
  /**
   * 创建一个地图 House，并返回可用于小队归属和外交列表的引用。
   * @param id 稳定的逻辑 ID；不能与地图已有 House 或本任务其他声明冲突。
   * @param options 必须选择现有 Country；颜色只能从生成的 HouseColor 枚举选取。
   */
  createHouse(id: string, options: NewHouseOptions<NonNullable<S["houseColors"]>[number]>): House {
    this.houseDefinitions.push({ ...options, id, create: true });
    return ref("house", id);
  }
  /** 修改已有地图 House 的显式字段；未列出的字段保持原值。 */
  configureHouse(house: House, options: HouseSettings<NonNullable<S["houseColors"]>[number]>): void {
    this.houseDefinitions.push({ ...options, id: house.id, create: false });
  }
  /** 声明 AI 小队选择器；所有小队/类型引用都在 Rust 构建阶段再次校验。 */
  aiTrigger(id: string, options: AiTriggerOptions): void {
    const { owner, ...rest } = options;
    this.aiTriggers.push({ ...rest, ...(owner === "all" ? {} : { owner }), id });
  }
  /** 设置受支持的 Basic 字段；未声明字段沿用地图现值。 */
  configureBasic(options: MissionBasicOptions): void {
    if (this.basic) throw new Error("configureBasic() 每个任务只能调用一次");
    this.basic = options;
  }
  /** 声明初始 SpecialFlags；字段按任务模式收窄，省略项保留地图现值。每任务一次。 */
  configureSpecialFlags(options: MissionSpecialFlagsOptions<M>): void {
    if (this.specialFlags) throw new Error("configureSpecialFlags() 每个任务只能调用一次");
    this.specialFlags = options;
  }
  /** 设置开局光照；每任务一次，数值由 Rust 按原生单精度读取及量化边界校验。运行时改变用 action.setAmbient 等。 */
  configureLighting(options: MissionLightingOptions): void {
    if (this.lighting) throw new Error("configureLighting() 每个任务只能调用一次");
    for (const profile of [options.normal, options.ion, options.dominator]) {
      if (profile === undefined) continue;
      for (const value of [profile.ambient, profile.red, profile.green, profile.blue, profile.ground, profile.level]) {
        if (value !== undefined && !Number.isFinite(value)) throw new Error("光照分量须为有限数值");
      }
    }
    for (const value of [options.dominatorAmbientChangeRate, options.nukeAmbientChangeRate]) {
      if (value !== undefined && (!Number.isFinite(value) || value <= 0)) throw new Error("光照过渡间隔须为正有限数值");
    }
    this.lighting = options;
  }
  /** 设置单人战役的结算评级；每个任务只能调用一次，Rust 再校验引用和数值。 */
  configureRanking(this: MissionBuilder<S, MissionMode.SinglePlayer>, options: MissionRankingOptions): void {
    if (this.mode !== MissionMode.SinglePlayer) throw new Error("结算评级仅支持单人任务");
    if (this.ranking) throw new Error("configureRanking() 每个任务只能调用一次");
    for (const seconds of [options.parTime?.easy, options.parTime?.normal, options.parTime?.hard]) {
      if (seconds === undefined) continue;
      if (!Number.isInteger(seconds) || seconds < 0 || seconds > 359999) throw new Error("评级基准时间须为 0～359999 整数秒");
    }
    this.ranking = options;
  }
  /**
   * 取得开局占用指定玩家起点的实际所属方，包含人类和 AI，不绑定国家或玩家颜色。
   * @param id 当前地图 Waypoint 枚举中的 WP0～WP29，仅接受地图已有的这些起点。
   * @returns 可传入 TeamOptions.house 的 StartPlayer；生成 House=<Player @ A>～<Player @ Z>、<Player @ AA>～<Player @ AD>。
   * @throws 单人模式、起点不存在或超出 0～29 时构建失败；Rust 会再次验证。
   * @remarks 仅在 MissionMode.Multiplayer 中可用。玩家随机起点时按实际占位解析；
   * 该起点无人占用时游戏无法创建此小队，不会自动送给中立方。
   * 这里只选择归属，不决定出兵位置；位置仍由小队 waypoint 和 action.reinforce() 指定。
   * 8～29 为 VERA 扩展起点；需在地图设置中配置相应出生点数量，普通任务航点不会因此成为玩家起点。
   * @example house: m.playerAt(Waypoint.WP0)
   */
  playerAt(this: MissionBuilder<S, MissionMode.Multiplayer>, id: Extract<S["waypoints"][number], PlayerStartId>): StartPlayer {
    if (this.mode !== MissionMode.Multiplayer) throw new Error("按起点选择玩家仅支持多人任务");
    this.member(this.schema.waypoints, id, "player start");
    if (!Number.isInteger(id) || id < 0 || id > 29) throw new Error("玩家起点仅支持 0～29");
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
  /** 声明普通任务航点 30～701；不能覆盖地图已有航点或玩家起点。 */
  createWaypoint(id: number, x: number, y: number): Waypoint {
    if (!Number.isInteger(id) || id < 30 || id > 701) throw new Error("新任务航点编号须为 30～701");
    if (!Number.isInteger(x) || x < 0 || x > 65535 || !Number.isInteger(y) || y < 0 || y > 65535) {
      throw new Error("任务航点坐标须为非负整数");
    }
    if (this.waypoints.some(point => point.id === id)) throw new Error(`任务航点 ${id} 重复声明`);
    this.waypoints.push({ id, x, y });
    return ref("waypoint", id);
  }
  /**
   * 通过零秒一次性触发器，在开局普通帧中把已有对象转交给指定起点的实际占用玩家。
   * 加载完成时对象先属 Neutral；多个转交规则可能分帧执行，不能假定首帧前已全部转交。
   * 不能与该对象的其他 Tag 绑定并用。
   */
  assignObjectToPlayer(this: MissionBuilder<S, MissionMode.Multiplayer>, object: MapObject, player: number): void {
    if (!Number.isInteger(player) || player < 0 || player > 29) throw new Error("对象开局归属仅支持玩家起点 0～29");
    if (this.startOwners.some(item => item.object.id === object.id)) throw new Error(`对象 ${object.id} 重复指定开局归属`);
    this.startOwners.push({ object, player });
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
   * 取得建筑类型引用，不会创建资源。
   * @param id map.generated.ts 中的 Building 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  building(id: NonNullable<S["buildings"]>[number]): BuildingType {
    this.member(this.schema.buildings ?? [], id, "building");
    return ref("building", id);
  }

  /**
   * 取得科技类型（含建筑、步兵、车辆和飞机）引用，不会创建资源。
   * @param id map.generated.ts 中的 Techno 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  techno(id: NonNullable<S["technos"]>[number]): TechnoType {
    this.member(this.schema.technos ?? [], id, "techno");
    return ref("techno", id);
  }

  /**
   * 取得国家，区别于地图 House引用，不会创建资源。
   * @param id map.generated.ts 中的 Country 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  country(id: NonNullable<S["countries"]>[number]): Country {
    this.member(this.schema.countries ?? [], id, "country");
    return ref("country", id);
  }

  /**
   * 取得声音引用，不会创建资源。
   * @param id map.generated.ts 中的 Sound 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  sound(id: NonNullable<S["sounds"]>[number]): Sound {
    this.member(this.schema.sounds ?? [], id, "sound");
    return ref("sound", id);
  }

  /**
   * 取得EVA 语音引用，不会创建资源。
   * @param id map.generated.ts 中的 Eva 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  eva(id: NonNullable<S["eva"]>[number]): Eva {
    this.member(this.schema.eva ?? [], id, "eva");
    return ref("eva", id);
  }

  /**
   * 取得音乐引用，不会创建资源。
   * @param id map.generated.ts 中的 Theme 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  theme(id: NonNullable<S["themes"]>[number]): Theme {
    this.member(this.schema.themes ?? [], id, "theme");
    return ref("theme", id);
  }

  /**
   * 取得影片引用，不会创建资源。
   * @param id map.generated.ts 中的 Movie 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  movie(id: NonNullable<S["movies"]>[number]): Movie {
    this.member(this.schema.movies ?? [], id, "movie");
    return ref("movie", id);
  }

  /**
   * 取得体素动画类型引用，不会创建资源。
   * @param id map.generated.ts 中的 VoxelAnim 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  voxelAnim(id: NonNullable<S["voxelAnims"]>[number]): VoxelAnimType {
    this.member(this.schema.voxelAnims ?? [], id, "voxelAnim");
    return ref("voxelAnim", id);
  }
  /** 引用当前资源目录中的普通 SHP 动画类型。 */
  animation(id: NonNullable<S["animations"]>[number]): AnimationType {
    this.member(this.schema.animations ?? [], id, "animation");
    return ref("animation", id);
  }
  /** 引用当前规则中的 WeaponType，包括单位武器及规则隐式分配的武器。请刷新目录取得 Weapon 枚举。 */
  weapon(id: NonNullable<S["weapons"]>[number]): WeaponType {
    this.member(this.schema.weapons ?? [], id, "weapon");
    return ref("weapon", id);
  }
  /** 引用当前资源目录中的原生 ParticleSystemType。 */
  particleSystem(id: NonNullable<S["particleSystems"]>[number]): ParticleSystemType {
    this.member(this.schema.particleSystems ?? [],id,"particleSystem");
    return ref("particleSystem",id);
  }

  /**
   * 取得超级武器类型引用，不会创建资源。
   * @param id map.generated.ts 中的 SuperWeapon 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  superWeapon(id: NonNullable<S["superWeapons"]>[number]): SuperWeaponType {
    this.member(this.schema.superWeapons ?? [], id, "superWeapon");
    return ref("superWeapon", id);
  }

  /**
   * 取得弹头类型引用，不会创建资源。
   * @param id map.generated.ts 中的 Warhead 枚举成员。
   * @remarks 请刷新类型取得当前地图和资源配置的目录；原生名称/注册序号由 Rust 编码。
   */
  warhead(id: NonNullable<S["warheads"]>[number]): WarheadType {
    this.member(this.schema.warheads ?? [], id, "warhead");
    return ref("warhead", id);
  }

  /**
   * 声明小队模板，生成原生 TaskForce、ScriptType 和 TeamType。
   * @param id 本任务内唯一的小队逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。
   * @param options 归属、航点、成员/脚本和 TeamType AI、招募及运输参数。
   * 成员数量必须为正整数；引用、数量、名称和容量由 Rust 构建阶段再次检查。
   * @returns Team 引用，交给 action.reinforce() 或 action.recruit() 激活。
   * @remarks 仅声明模板，不会立刻出兵；同一模板可通过 reinforce() 多次增援。
   * Max、Priority 等设置影响原生 AI 组队/招募；Autocreate 由 AI 自行创建实例。
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
    this.origin("team", id);
    this.teams.push({ ...options, id });
    return ref("team", id);
  }
  /**
   * 登记一条触发规则，条件满足后由游戏执行动作列表。
   * @param id 本任务内唯一的规则逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线。
   * @param options 一个事件条件、1～64 个动作，以及是否仅触发一次的 once。
   * @returns 本任务的 Rule 引用；此方法不会在构建时执行动作。
   * @remarks 一个任务最多 1024 条规则；重复计时使用 once: false 和 event.sinceEnabled(N)，
   * 每次触发后重新计时。其他重复条件按原生语义执行，状态持续满足时可能连续触发。
   * 多条件使用 event.all/any；游戏动作使用描述数据。
   * 动作按列表顺序写入原生触发器，不表示等待一个动作的游戏效果结束后再执行下一个。
   * @example
   * m.rule('win', {
   *   when: event.sinceScenarioStart(300),
   *   once: true,
   *   actions: [action.winPlayer()],
   * });
   */
  rule(id: string, options: RuleOptions): Rule {
    this.origin("rule", id);
    this.rules.push({ ...options, id });
    return ref("rule", id);
  }
  /** 声明一次性阶段；默认 enabled:false，首阶段应显式 enabled:true。
   * 完成后按顺序执行 actions，再启用 next。next 使用规则引用，支持前向声明。 */
  stage(id: string, options: StageOptions): Rule {
    const { next, ...rule } = options;
    return this.rule(id, { ...rule, enabled: rule.enabled ?? false, once: true,
      actions: next ? [...rule.actions, action.enableRule(next)] : rule.actions });
  }
  /** 从现有车辆复制一个抽奖专用类型，普通生产单位保持原规则。
   * rules 接受原生单位规则名；只能引用当前资源已有的武器和美术资源，不创建武器／ART。
   * 返回值仅供 crateLottery.rewards 使用。删除定义并重建会移除生成类型。
   */
  rewardUnit(id: string, options: RewardUnitOptions): RewardUnit {
    if (!/^[A-Za-z0-9_]{1,52}$/.test(id) || this.rewardUnits.some(unit => unit.id === id))
      throw new Error("rewardUnit 名称须唯一且为 1～52 位 ASCII 字母、数字或下划线");
    const { name, ...definition } = options;
    this.rewardUnits.push({
      ...definition, id,
      ...(name === undefined ? {} : { uiName: this.defineText(`${id}_name`, name) }),
    });
    return ref("rewardUnit", id);
  }
  /**
   * 单人建筑栏购买并放置补给箱，Ctrl+攻击拆毁后，随机车辆在 rewardAt 航点到达。
   * 限制同时一座，存在期间及结算期间暂时隐藏生产；奖励固定归任务 player。
   * 通过原生建筑存在/不存在条件检测拆毁，失去建筑（如脚本移除）同样触发。
   * ticket 须专用且不能预放；外观与图标沿用现有 ART。其他箱子的奖励保持原样。
   * 原生随机计时竞争选奖有短暂延迟，同秒到期按 Tag 顺序决胜，不保证等概率。
   */
  crateLottery(id: string, options: CrateLotteryOptions): CrateLottery {
    if (this.mode !== MissionMode.SinglePlayer) throw new Error("crateLottery 拆毁发奖目前仅支持单人任务");
    if (this.crateLotteryRules) throw new Error("每个任务只能声明一个 crateLottery");
    if (!/^[A-Za-z0-9_]{1,52}$/.test(id)) throw new Error("crateLottery 名称须为 1～52 位 ASCII 字母、数字或下划线");
    if (options.ticket.kind !== "building" || !options.rewardAt || "buyers" in options || "pollSeconds" in options)
      throw new Error("crateLottery 须使用建筑 ticket 并指定 rewardAt；移除 buyers、waypoint 和 pollSeconds");
    const {name,...rules} = options;
    this.crateLotteryRules = {...rules, uiName:this.defineText(`${id}_name`,name ?? "补给箱（攻击拆开）")};
    return options.ticket;
  }
  /**
   * 汇总当前定义并返回任务描述，通常由 defineMission() 自动调用。
   * @param id 任务逻辑名称，须为 1～64 位 ASCII 字母、数字或下划线，用于生成稳定 ID。
   * @param player 单人玩家 House ID，应来自当前地图目录；多人模式留空。
   * @returns 引用当前定义数组的 Mission 数据；不执行深拷贝、完整验证或地图应用。
   * @remarks 返回后不要继续登记定义。手动调用也不能跳过 Rust 对整个结果的验证。
   */
  finish(id: string, player = ""): Mission {
    return { version: 2, id, mode: this.mode, player, teams: this.teams, houseDefinitions: this.houseDefinitions, aiTriggers: this.aiTriggers, ...(this.basic === undefined ? {} : {basic: this.basic}), ...(this.ranking === undefined ? {} : {ranking: this.ranking}), waypoints: this.waypoints, startOwners: this.startOwners, texts: this.texts, assets: this.assets, nativeDefinitions: this.nativeDefinitions, aliases: this.aliases, origins: this.origins,
      ...(this.specialFlags === undefined ? {} : {specialFlags: this.specialFlags}),
      ...(this.lighting === undefined ? {} : {lighting: this.lighting}),
      ...(this.crateLotteryRules === undefined ? {} : {crateLottery: this.crateLotteryRules}),
      rewardUnits: this.rewardUnits,
      taskForces: this.taskForces,
      scripts: this.scripts, tags: this.tags, bindings: this.bindings, flags: this.flags, rules: this.rules };
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
 * 可以使用同步函数和循环生成定义；回调可省略返回值或直接返回 m.rule() 的引用，不能使用 async 或返回任务数据。
 * @returns 可序列化的 Mission 描述，随后由 Rust 验证并转换为原生地图记录。
 * @throws 玩家不在目录中、回调抛错或返回了任务数据、Promise 等不支持的值时构建失败。
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
 * 可以循环声明多个小队和规则；回调可直接返回 m.rule() 的引用，不能返回任务数据或 Promise。
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
  const mission = new MissionBuilder(schema, options.mode ?? MissionMode.SinglePlayer, options.id);
  if (options.mode !== MissionMode.Multiplayer) mission.house(options.player);
  // 公开重载保证回调与 options 的模式一致，此处统一执行同步构建流程。
  const result: unknown = (build as (builder: typeof mission) => void)(mission);
  const finished = mission.finish(options.id, options.player ?? "");
  // m.rule() used to return undefined. Preserve concise callbacks such as
  // m => m.rule(...), while continuing to reject Promises and returned data.
  const returnedRule = typeof result === "object" && result !== null && "kind" in result
    && result.kind === "rule" && "id" in result && finished.rules.some(rule => rule.id === result.id);
  if (result !== undefined && !returnedRule) throw new Error("Mission construction must be synchronous and return no data");
  return finished;
}

/** 触发条件工厂；只返回事件描述，必须传给 m.rule() 的 when 才会生成触发器。
 * House 状态/建造条件由规则 owner 对应的实际 House 轮询；无匹配 House 的单独规则没有该轮询。
 * 与逻辑条件组合会增加其他轮询入口，无 owner 的部分 House 条件此时直接满足；请指定实际 House 对应的 owner。
 */
export const event = {
  /** 附着对象受到指定实际 House 的有来源伤害；单人模式，Rust 按最终 Houses 表投影原生序号。 */
  damagedByHouse: (object: MapObject, house: House): Event => ({kind:"damagedByHouse",object,house}),
  /** 规则所属 House 最近记录的建筑类型；运行时创建并被发现的建筑也会更新，匹配后原生锁存。不是累计建造计数。 */
  buildingBuilt: (building: BuildingType): Event => ({ kind: "buildingBuilt", building }),
  /** 规则所属 House 成功交付后记录的最近车辆类型；须为原生 VehicleType，匹配后可锁存。生产中limbo对象、取消或出厂失败退款不表示建成。 */
  vehicleBuilt: (type: TechnoType): Event => ({ kind: "vehicleBuilt", type }),
  /** 规则所属 House 成功交付后记录的最近步兵类型；须为原生 InfantryType，匹配后可锁存。不是构造或开始生产通知。 */
  infantryBuilt: (type: TechnoType): Event => ({ kind: "infantryBuilt", type }),
  /** 规则所属 House 成功交付后记录的最近飞机类型；须为原生 AircraftType，匹配后可锁存。不是构造或开始生产通知。 */
  aircraftBuilt: (type: TechnoType): Event => ({ kind: "aircraftBuilt", type }),
  /** 规则所属 House 的该建筑当前 active count 非零；与其他条件组合时重新读取，不锁存曾经存在的结果。 */
  buildingExists: (building: BuildingType): Event => ({ kind: "buildingExists", building }),
  /** 规则所属 House 的该建筑当前 active count 为零；与其他条件组合时重新读取，不锁存曾经不存在的结果。 */
  buildingAbsent: (building: BuildingType): Event => ({ kind: "buildingAbsent", building }),
  /** Agent 进入绑定格子且强制伪装类型为指定 InfantryType；比较显示的伪装身份，不是对象自身的 SPY 类型。 */
  spyDisguisedAsInfantry: (type: TechnoType): Event => ({ kind: "spyDisguisedAsInfantry", type }),
  /** 每轮由引擎抽取 floor(seconds/2)～floor(seconds/2)+seconds 整数游戏秒，含两端。
   * 重复规则及重新启用时重新抽取；0 不形成等待间隔。最大 95443717，与 sinceEnabled 共用一个计时器。
   * 当前引擎读取快照会重置场景随机流；已抽取的计时保留，后续抽样不保证与连续运行相同。
   */
  randomTime: (seconds: number): Event => ({ kind: "randomTime", seconds }),
  /** 规则所属 House 当前可用资金不少于 amount（原始 credits，非千元）。 */
  creditsAtLeast: (amount: number): Event => ({ kind: "creditsAtLeast", amount }),
  /** 规则所属 House 当前可用资金不多于 amount。 */
  creditsAtMost: (amount: number): Event => ({ kind: "creditsAtMost", amount }),
  /** 规则所属 House 累计损失建筑不少于 count。 */
  buildingsLost: (count: number): Event => ({ kind: "buildingsLost", count }),
  /** 规则所属 House 累计损失单位不少于 count。 */
  unitsLost: (count: number): Event => ({ kind: "unitsLost", count }),
  /** 规则所属 House 没有非 limbo 工厂建筑；不是检查生产队列是否为空。 */
  noFactories: (): Event => ({ kind: "noFactories" }),
  /** 当前环境亮度整数 <= value；100 表示亮度 100%，与 configureLighting 的比例值 1 对应；不读取目标亮度。 */
  ambientAtMost: (value: number): Event => ({ kind: "ambientAtMost", value }),
  /** 当前环境亮度整数 >= value；包含等值，单位与 ambientAtMost 相同。 */
  ambientAtLeast: (value: number): Event => ({ kind: "ambientAtLeast", value }),
  /** Country 首个 House 已被当前玩家发现；若地图没有该 Country 的 House，原生条件直接满足。 */
  countryDiscovered: (country: Country): Event => ({ kind: "countryDiscovered", country }),
  /** Country 首个 House 的在场 Unit 载具（含海军）及 Infantry 步兵计数分别 <= 0。
   * 不统计 Aircraft 类或 limbo 中的乘客；飞行的 Unit 类仍算载具。没有匹配 House 时也满足。
   */
  countryUnitsDestroyed: (country: Country): Event => ({ kind: "countryUnitsDestroyed", country }),
  /** Country 首个 House 的跟踪建筑数 <= 0；没有匹配 House 时也满足。 */
  countryBuildingsDestroyed: (country: Country): Event => ({ kind: "countryBuildingsDestroyed", country }),
  /** Country 首个 House 的跟踪建筑数 <= 0，且 countryUnitsDestroyed 条件满足。
   * 不统计 Aircraft 类；存活乘客进入 limbo 后也可能满足，不保证地图中所有对象均已销毁。没有匹配 House 时也满足。
   */
  countryAllDestroyed: (country: Country): Event => ({ kind: "countryAllDestroyed", country }),
  /** Country 首个 House 的跟踪海军数 <= 0；只统计 Naval=yes 且 Passengers=0 的 Unit 类。
   * 有载客容量的海军运输工具不计入此数。没有匹配 House 时也满足。
   */
  countryNavyDestroyed: (country: Country): Event => ({ kind: "countryNavyDestroyed", country }),
  /** Country 首个 House 的（跟踪车辆数 - 跟踪海军数）及跟踪步兵数均 <= 0。
   * 有载客容量的海军车辆仍算在此列，普通运输中的步兵也保留跟踪计数；与在场计数不同。没有匹配 House 时也满足。
   */
  countryLandDestroyed: (country: Country): Event => ({ kind: "countryLandDestroyed", country }),
  /** Country 首个 House 的保留电力状态为缺电；选择对象与规则 owner 独立。没有匹配 Country House 时直接满足。 */
  lowPower: (country: Country): Event => ({ kind: "lowPower", country }),
  /** Country 首个 House 的保留电力状态为充足；没有匹配 Country House 时也直接满足。 */
  fullPower: (country: Country): Event => ({ kind: "fullPower", country }),
  /** 指定国家经过与绑定格子同 ry 的整条原生格子行；用 bindCell 绑定，不必经过该格本身。
   * 同 Tag 同时有水平/垂直线时只初始化水平线；不是屏幕坐标中的横线。
   */
  crossedHorizontal: (country: Country): Event => ({ kind: "crossedHorizontal", country }),
  /** 指定国家经过与绑定格子同 rx 的整条原生格子列；用 bindCell 绑定，不必经过该格本身。 */
  crossedVertical: (country: Country): Event => ({ kind: "crossedVertical", country }),
  /** 指定国家进入或飞越绑定格子；地面到达时检查，高空移动对象按原生帧每16帧采样当前格。
   * 快速飞行在两次采样之间经过该格可能不触发；这里比较实际所属方，不是间谍伪装所属方。
   */
  enteredOrOverflown: (country: Country): Event => ({ kind: "enteredOrOverflown", country }),
  /** 进入绑定格子的 Agent 显示为指定国家首个 House；比较伪装所属方，不是实际所属方。
   * 联机只比较其实际伪装所属方，不使用当前玩家回退身份。
   */
  spyDisguisedAsCountry: (country: Country): Event => ({ kind: "spyDisguisedAsCountry", country }),
  /**
   * 指定 TeamType 的小队已空且保留原生 leaving-map 标记时收到23通知；接受前向 teamRef。
   * 标记不等于几何上已出界：步兵正常AI也可置位，随后在场内移除成员仍可能通知。
   * 要按 TeamType 筛选，规则 owner 须对应实际 House；owner 国家没有匹配 House 时，收到任意23广播都会满足，绕过筛选。
   * deleteTeam() 直接解散不等同于这条通知；需要确认实体实际离场时应另行核对任务条件。
   */
  teamLeftMap: (team: Team): Event => ({ kind: "teamLeftMap", team }),
  /** 对象发现通知；揭示格子/正常视野可发布，历史发现标记本身不是新通知。原生有来源击杀路径也可能发布同编号，不能用于精确首次看见判定。 */
  discovered: (object: MapObject): Event => ({ kind: "discovered", object }),
  /** 对象遭攻击通知；要求伤害来源，击杀/心控路径也可能发布。 */
  attacked: (object: MapObject): Event => ({ kind: "attacked", object }),
  /** 对象销毁通知（包含无攻击来源的原生销毁路径）。 */
  destroyedAny: (object: MapObject): Event => ({ kind: "destroyedAny", object }),
  /** 对象被选择；当前引擎仅单人模式发布，Rust 拒绝多人使用。 */
  selected: (object: MapObject): Event => ({ kind: "selected", object }),
  /** 绑定探照灯所在建筑，在模式1扫描到附近地面非友军时通知；模式0/2/3不发布此检测事件，建筑须可运作。 */
  spotlightDetected: (object: MapObject): Event => ({ kind: "spotlightDetected", object }),
  /** 对象从满血首次受伤，要求存在伤害来源，不要求敌对。 */
  firstDamagedBySource: (object: MapObject): Event => ({ kind: "firstDamagedBySource", object }),
  /** 对象生命从 >= floor(Strength/2) 降至该值以下，要求伤害来源；同一击触发红血时不再发黄血通知。 */
  halfHealthBySource: (object: MapObject): Event => ({ kind: "halfHealthBySource", object }),
  /** 对象生命从严格高于 Strength*ConditionRed 降至严格低于该值，要求伤害来源；等于线值不触发。 */
  redHealthBySource: (object: MapObject): Event => ({ kind: "redHealthBySource", object }),
  /** 对象从满血首次受伤；有来源时原生可发布两次同编号通知。重复 Tag 会保留锁存；需单次处理可 disableRule 自身。 */
  firstDamaged: (object: MapObject): Event => ({ kind: "firstDamaged", object }),
  /** 对象生命从 >= floor(Strength/2) 降至该值以下；同一击触发红血时不再发黄血通知，不是持续血量谓词。 */
  halfHealth: (object: MapObject): Event => ({ kind: "halfHealth", object }),
  /** 对象生命从严格高于 Strength*ConditionRed 降至严格低于该值；命中恰好线值或从线值继续下降不触发。 */
  redHealth: (object: MapObject): Event => ({ kind: "redHealth", object }),
  /** 对象销毁或进入建筑/离场等移除通知；不同移动类别遵循原生通知路径。 */
  destroyedOrRemoved: (object: MapObject): Event => ({ kind: "destroyedOrRemoved", object }),
  /**
   * 绑定对象拾取 CrateTrigger=yes 的木箱，在奖励处理前同步通知。
   * 回调可转移所属；后续奖励重读拾取者的 House。
   * 回调后若对象已不再 Alive，则中止全局通知及奖励；仅生命归零但仍在死亡动画中的步兵不满足此条件。
   */
  pickedUpCrate: (object: MapObject): Event => ({ kind: "pickedUpCrate", object }),
  /** 地图中发生 CrateTrigger=yes 的木箱拾取；对象回调返回且仍 Alive 后标记，后续逻辑阶段分发；不表示奖励类型或数额。 */
  anyCratePickedUp: (): Event => ({ kind: "anyCratePickedUp" }),
  /** 指定国家进入附着格/对象；需显式 Tag 绑定。国家为运行时首个匹配 House。 */
  entered: (country: Country): Event => ({ kind: "entered", country }),
  /** 指定国家进入附着格所属的原生移动 zone；并非矩形区域，需显式 Tag 绑定。 */
  enteredZone: (country: Country): Event => ({ kind: "enteredZone", country }),
  /** 原生事件8，条件始终为真；也会注册到常规 Logic 轮询，无绑定时仍可触发。
   * linked 共用的一次性 Tag 可因此提前消耗；仅需特定通知时直接使用对应事件。
   */
  tagNotification: (): Event => ({ kind: "tagNotification" }),
  /** 桥梁破坏通知；使用 m.bindCell 将所属规则 Tag 绑定桥梁跨度格。 */
  bridgeDestroyed: (): Event => ({ kind: "bridgeDestroyed" }),
  /** 附着移动对象接近航点，阈值 1280 leptons（5 格），由对象移动通知评估。 */
  nearWaypoint: (object: MapObject, waypoint: Waypoint): Event => ({ kind: "nearWaypoint", object, waypoint }),
  /** 全地图存储的 Techno 类型数量达到 count（默认 1），包含建筑及 limbo/零生命记录；不按所属方过滤。 */
  technoExists: (type: TechnoType, count = 1): Event => ({ kind: "technoExists", type, count }),
  /** 全地图已存储的该具体科技类型数量为零，不按所属方过滤。 */
  technoAbsent: (type: TechnoType): Event => ({ kind: "technoAbsent", type }),
  /** 原生多事件 AND；重复 Tag（once:false）可锁存部分对象通知，once:true 不保证跨时间锁存。
   * 如需先收到通知、稍后满足其他条件且只执行一次，可使用 once:false 并在动作中 disableRule 自身。
   * 变量条件检查当前状态，执行后按原生规则重置计时。最多 32 个叶事件。 */
  all: (...events: readonly Event[]): Event => ({ kind: "all", events }),
  /** 分支 OR，最多 32 个分支，同帧命中只执行一次。辅助标记汇合最多延后一个普通逻辑帧。
   * 须为 when 顶层，不与 linked/显式 Tag 混用。重复规则仅支持时间/变量/类型状态条件，
   * 每轮结束重置分支计时；附着对象通知的重复锁存不能安全降级，构建时报错。 */
  any: (...events: readonly Event[]): Event => ({ kind: "any", events }),
  /** 全局变量已设置；编号共享于战役，不在声明时清零。 */
  globalSet: (flag: GlobalFlag): Event => ({ kind: "globalSet", flag }),
  /** 全局变量已清除。 */
  globalClear: (flag: GlobalFlag): Event => ({ kind: "globalClear", flag }),
  /**
   * 创建“自场景开始经过指定时间”的条件，所有规则共用场景起点。
   * @param seconds 游戏逻辑秒数，须为 0～143165576 的整数，不是现实时间计时器。
   * 游戏时间按每秒 15 逻辑帧解释，实际墙钟时长取决于游戏速度和暂停。
   * @returns Event 描述，不会暂停当前 JS 执行；范围由 Rust 构建阶段检查。
   * @remarks 此条件达到时间后持续满足，不会在重复规则中重新计时；定时间隔请用 sinceEnabled()。
   * @example event.sinceScenarioStart(60) // 场景开始后第 60 个游戏秒
   */
  sinceScenarioStart: (seconds: number): Event => ({ kind: "scenarioTime", seconds }),
  /**
   * 创建“触发器启用后经过指定时间”的条件，计时起点属于游戏中的触发器。
   * @param seconds 游戏逻辑秒数，须为 0～143165576 的整数，按每秒 15 逻辑帧解释。
   * @returns Event 描述；不会从调用此 JS 方法的时刻计时，也不会阻塞脚本。
   * @remarks 规则默认启用；可用 enabled: false 和 action.enableRule/disableRule 控制起点。
   * 配合 once: false 可每隔指定秒数执行，首次在启用后达到该时间时触发，每次触发后重新计时。
   * seconds 为 0 时没有等待间隔；持续出兵应传入正整数。
   * 需要明确按场景开局安排波次时，使用 sinceScenarioStart()。
   * @example event.sinceEnabled(30)
   */
  sinceEnabled: (seconds: number): Event => ({ kind: "enabledTime", seconds }),
  /**
   * 创建“任务倒计时结束”的条件；对应右下角显示的全地图共享倒计时。
   * @returns Event 描述；倒计时须正在运行且已归零，停止的倒计时不会触发。
   * @remarks 归零事件处理后游戏会停止并隐藏倒计时；不会自动宣告胜负。
   * 持续出兵可搭配 once: false，在动作中增援并调用 setCountdown() 开始下一轮。
   * 重设倒计时会立即改变共享状态；多条结束规则不保证都能观察到同一次归零。
   * @example event.countdownElapsed()
   */
  countdownElapsed: (): Event => ({ kind: "countdownElapsed" }),
  /**
   * 创建“绑定的具体地图对象被摧毁”的条件。
   * @param object 通过 m.object() 取得的地图对象引用，不接受 m.unit() 的单位类型。
   * @returns Event 描述；应用任务时会将生成的 Tag 绑定到该对象。
   * @remarks 原生事件7要求存在攻击来源；killMembers() 等无来源销毁应使用 destroyedAny()。
   * 目标必须存在、记录无歧义且未绑定其他 Tag；多条规则需 linked 共用同一 Tag。
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
  flagSet: (flag: Flag | LocalFlag): Event => flag.kind === "localFlag"
    ? { kind: "localSet", flag } : { kind: "flagSet", flag },
  /**
   * 创建“局部标记为 false”的条件，检查状态而非仅监听 true 到 false 的变化。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Event 描述；标记默认初始值为 false，使用默认值时条件可能开局即满足。
   * @example event.flagClear(alarm)
   */
  flagClear: (flag: Flag | LocalFlag): Event => flag.kind === "localFlag"
    ? { kind: "localClear", flag } : { kind: "flagClear", flag },
};
/** 原生 Veterancy 级别；Regular 对应引擎默认等级。 */
export enum VeteranLevel { Rookie = 0, Regular = 1, Veteran = 2, Elite = 3 }
/** 小队命令工厂；返回值放入 TeamOptions.orders，由游戏中的小队按行为脚本执行。 */
export const order = {
  /** 跟随队外最近的存活载具/步兵；由候选方到小队所属方的单向同盟决定友军。抵达可推进，无候选时等待，并非永久跟随。 */
  followFriendlies: (): Order => ({ kind: "followFriendlies" }),
  /** 请求步兵 Idle1（0）或 Idle2（1），不等待动画结束；准入由当前动作/类型决定，其他类别不播放，脚本仍推进。 */
  idleAnimation: (animation: 0|1): Order => ({ kind: "idleAnimation", animation }),
  /** 按当前玩家的迷雾选择附近目标，并正常移动；首次选点后保持目标，不读取小队所属方的迷雾。
   * 无候选时保留原中心。仅单人可用，联机构建时报错。
   */
  scout: (): Order => ({ kind: "scout" }),
  /** Script48：选择尚未侦察的 House 建筑并移动，抵达后标记；参数由引擎忽略，支持多人。 */
  scoutHouses: (): Order => ({ kind: "scoutHouses" }),
  /** Script32：无正在进行的闪电风暴时请求开始，时间为有符号逻辑帧，直接传给引擎；随后推进脚本。 */
  startLightningStorm: (frames: number): Order => ({ kind: "startLightningStorm", frames }),
  /** Script33：请求结束当前闪电风暴，随后推进脚本。 */
  stopLightningStorm: (): Order => ({ kind: "stopLightningStorm" }),
  /** 将小队所属 House 的紧急策略设为状态 4 后推进，不重置策略时钟；AI 在时钟到期后出售建筑并让成员离队 Hunt，真人控制的 House 不走此策略更新。 */
  fireSale: (): Order => ({ kind: "fireSale" }),
  /** 镜头平移到小队当前 zone，速度档位为 0～4；脚本不等待镜头移动结束。 */
  centerCamera: (velocity: CameraVelocity = 1): Order => ({ kind: "centerCamera", velocity }),
  /** 按首个成员所属 House 搜索指定类型建筑并攻击；遵循 OnlyTargetHouseEnemy 及原生目标准入，找不到目标时推进。工程师不会因此自动切换为占领。 */
  attackEnemyBuilding: (building: BuildingType, mode = BuildingSelection.Nearest): Order => ({ kind: "attackEnemyBuilding", building, mode }),
  /** 按首个成员所属 House 搜索目标，移动到其附近、成员当前 zone 内的可通行格；找不到建筑或落脚格时推进。 */
  moveToEnemyBuilding: (building: BuildingType, mode = BuildingSelection.Nearest): Order => ({ kind: "moveToEnemyBuilding", building, mode }),
  /** 搜索领队所属 House 的指定类型建筑，移动到附近同 zone 可通行格；找不到目标时推进，不等待建造。 */
  moveToOwnBuilding: (building: BuildingType, mode = BuildingSelection.Nearest): Order => ({ kind: "moveToOwnBuilding", building, mode }),
  /** 使用领队所属 House 的超时空，将小队中心区域传到选定类型建筑附近；不授予超武。临近就绪时等待，未接近就绪/无超武/无目标时推进；成对发射后推进，不等待成员传送完成。 */
  chronoshiftToBuilding: (building: BuildingType, mode = BuildingSelection.Nearest): Order => ({ kind: "chronoshiftToBuilding", building, mode }),
  /** 首次执行时给每个成员附着动画并推进，不等待播放完毕。
   * loops 打包为16位构造参数，实际与 ART.LoopCount 作字节运算；0/1 同处理，0 不表示无限，256 等值可能回绕。
   */
  playAnimation: (animation: AnimationType, loops = 1): Order => ({ kind: "playAnimation", animation, loops }),
  /** 在执行脚本的当前小队首成员处显示气泡 1～3，0 清除；立即推进，不等待墙钟超时。
   * 全局只有一个气泡；需要 talkbubl.shp 和 palette.pal，不承载自定义文字。 */
  talkBubble: (bubble: 0|1|2|3): Order => ({ kind: "talkBubble", bubble }),
  /** 在领队所属 House 指定敌人的基地外 AISafeDistance 处集结；己方无基地时以领队位置决定方向，无指定敌人或敌方基地则推进。 */
  gatherAtEnemyBase: (): Order => ({ kind: "gatherAtEnemyBase" }),
  /** 在己方基地外 AISafeDistance 处集合：朝向指定敌人的基地，无指定敌人则随机方向。需要有效己方基地；找不到可通行3×3区域时保留原生(0,0)目标。 */
  regroupAtBase: (): Order => ({ kind: "regroupAtBase" }),
  /** 使用所属 House 可用铁幕；保留原生充能/电力准入。 */
  ironCurtain: (): Order => ({ kind: "ironCurtain" }),
  /** 与建筑超时空共用充能/电力准入和两次发射，目标由领队的原生 Quarry 威胁搜索选择；无武器不保证搜索失败。源区域内的非小队对象也遵循普通超时空效果。 */
  chronoshiftToQuarry: (quarry: Quarry): Order => ({ kind: "chronoshiftToQuarry", quarry }),
  /** 攻击航点格阻挡的建筑对象；空地、步兵、载具或地形对象不会成为此命令的建筑目标，无目标时推进。 */
  attackWaypointBuilding: (waypoint: Waypoint): Order => ({ kind: "attackWaypointBuilding", waypoint }),
  /** 搜索己方 Grinder；找到目标的成员立即离队并自行前往，命令不等待实际进入。搜索失败也推进。 */
  enterGrinder: (): Order => ({ kind: "enterGrinder" }),
  /** 搜索己方可用坦克碉堡；找到目标的成员立即离队并自行前往，进入仍受类别/占用准入限制。 */
  enterTankBunker: (): Order => ({ kind: "enterTankBunker" }),
  /** 搜索己方生化反应炉；找到接纳目标的成员立即离队并自行前往，命令不等待吸收完成。 */
  enterBioReactor: (): Order => ({ kind: "enterBioReactor" }),
  /** 搜索己方允许该步兵驻守的建筑；找到目标后立即离队并自行前往，不限于名为碉堡的类型。 */
  enterBattleBunker: (): Order => ({ kind: "enterBattleBunker" }),
  /** 在全体建筑中搜索最近的可驻守目标，保留原生敌我/容量准入；找到目标的成员立即离队并自行前往。 */
  enterNearestOccupiable: (): Order => ({ kind: "enterNearestOccupiable" }),
  /** 将成员转给指定国家的首个 House；不改变 Team.House，也不接受起点玩家选择器。 */
  changeCountry: (country: Country): Order => ({ kind: "changeCountry", country }),
  /** 攻击目标类别，受原生武器、敌我和威胁筛选控制。 */
  attack: (quarry: Quarry): Order => ({ kind: "attack", quarry }),
  /** 永久停留当前行为，不自动进入下一行。 */
  waitForever: (): Order => ({ kind: "waitForever" }),
  /** 移动到地图格子；原生固定 y*128+x，x 必须小于 128。 */
  moveToCell: (x: number, y: number): Order => ({ kind: "moveToCell", x, y }),
  /** 展开可展开成员；条件与完成判定由原生行为控制。 */
  deploy: (): Order => ({ kind: "deploy" }),
  /** 持续向成员下达指定 Mission，原生 Script11 停留当前行，不自动执行后续命令；不改变成员类别能力。 */
  mission: (mission: UnitMission): Order => ({ kind: "mission", mission }),
  /** 让成员散开。 */
  scatter: (): Order => ({ kind: "scatter" }),
  /** 设置允许恐慌的步兵成员的恐惧值；无畏类型及其他类别由原生回调拒绝，脚本仍推进。 */
  panic: (): Order => ({ kind: "panic" }),
  /** 按原生类型准入清除步兵恐惧值；不直接重置卧倒姿态或动作时钟，脚本仍推进。 */
  calm: (): Order => ({ kind: "calm" }),
  /** 设置原生八方向朝向。 */
  face: (direction: Direction): Order => ({ kind: "face", direction }),
  /** 装载小队中的运输工具，需模板和成员实际满足装载条件；空座不阻止本行完成。
   * 普通上车时乘客离开Team，卸载时再加入载具所在Team。满载提前完成时不保存返程位置。
   */
  load: (): Order => ({ kind: "load" }),
  /** 等待每个运输成员的乘客数达到其 Passengers 容量；空座仍会等待，并非仅等待当前乘客上车。 */
  waitLoaded: (): Order => ({ kind: "waitLoaded" }),
  /** 等待实际卸载后按 mode 解除成员；不直接删除单位。所有成员离队后，小队清理，不执行后续行。
   * transportsReturnOnUnload 优先让运输成员离队并使用已有返程位置；乘客仍按mode处理。
   */
  unload: (mode: UnloadMode = UnloadMode.KeepAll): Order => ({ kind: "unload", mode }),
  /** 间谍进入航点上的目标，准入由原生间谍行为判断。 */
  spyWaypoint: (waypoint: Waypoint): Order => ({ kind: "spyWaypoint", waypoint }),
  /** 逻辑行标签，不占原生脚本行；须指向其后的实际命令，支持后向跳转。 */
  label: (label: string): Order => ({ kind: "label", label }),
  /** 跳转到本脚本逻辑标签，插入命令后自动重算原生一基行号。 */
  jump: (label: string): Order => ({ kind: "jump", label }),
  /** 原生 Script17：替换当前脚本、光标重置为 -1，但不自动推进新脚本；需要其他原生所有者推进。
   * 不将它当作函数调用或自动接续；注册序号由 Rust 从固定 AIMD + 最终地图列表解析。 */
  changeScript: (script: Script): Order => ({ kind: "changeScript", script }),
  /** 标记进入下一行，由正常 Team 更新推进。 */
  next: (): Order => ({ kind: "next" }),
  /** 锁存小队成功并推进，不判定玩家胜利，也不立即结束小队。
   * 小队结束/解散时才向以此 TeamType 为 primary 的 AITrigger 记一次结果；重复调用不重复计数。
   */
  succeed: (): Order => ({ kind: "succeed" }),
  /** 经无攻击来源的伤害链杀死成员并计入损失；不触发要求来源的 event.destroyed()。 */
  killMembers: (): Order => ({ kind: "killMembers" }),
  /** 经 Limbo/UnInit 移除成员，不计入击杀损失，也不保证发布对象销毁/移除通知。 */
  removeMembers: (): Order => ({ kind: "removeMembers" }),
  /** 将当前单人本地玩家设为胜利，与 Team 所属方无关；结果经过 SavourDelay。 */
  winPlayer: (): Order => ({ kind: "winPlayer" }),
  /** 将当前单人本地玩家设为失败，与 Team 所属方无关；结果经过 SavourDelay。 */
  losePlayer: (): Order => ({ kind: "losePlayer" }),
  /** 小队脚本设置本任务/已有局部标记。 */
  setFlag: (flag: Flag | LocalFlag): Order => flag.kind === "localFlag" ? { kind: "setLocal", flag } : { kind: "setFlag", flag },
  /** 小队脚本清除本任务/已有局部标记。 */
  clearFlag: (flag: Flag | LocalFlag): Order => flag.kind === "localFlag" ? { kind: "clearLocal", flag } : { kind: "clearFlag", flag },
  /** 设置战役全局变量。 */
  setGlobal: (flag: GlobalFlag): Order => ({ kind: "setGlobal", flag }),
  /** 清除战役全局变量。 */
  clearGlobal: (flag: GlobalFlag): Order => ({ kind: "clearGlobal", flag }),
  /** 居中播放声音并推进下一行，不等待声音结束；Rust 按声音原生注册顺序编码整数。 */
  playSound: (sound: Sound): Order => ({ kind: "playSound", sound }),
  /** 请求全屏影片并推进下一行；战役客户端负责播放期间暂停与结束后恢复。按固定 ART Movies 顺序编码；遭遇战/多人或缺少影片文件时跳过播放。 */
  playMovie: (movie: Movie): Order => ({ kind: "playMovie", movie }),
  /** 请求切换背景音乐并推进下一行，不等待曲目结束；Rust 按活动 ThemeCatalog 注册顺序编码整数。 */
  playMusic: (theme: Theme): Order => ({ kind: "playMusic", theme }),
  /** 闪烁小队成员，带符号逻辑帧（15 帧/秒）。 */
  flash: (frames: number): Order => ({ kind: "flash", frames }),
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
  /** 设置附着建筑探照灯模式：0 关闭、1 往返扫描、2 绕建筑持续旋转、3 跟踪灯点附近的最近地面非友军。模式3无有效目标时会退回1。 */
  setSpotlightBehavior: (mode: 0|1|2|3): Action => ({ kind: "setSpotlightBehavior", mode }),
  /** 动作43：在航点生成 VoxelAnimType；索引取自 VoxelAnims，不能传入普通 Animation。 */
  playVoxelAnimAt: (animation: VoxelAnimType, at: Waypoint): Action => ({ kind: "playVoxelAnimAt", animation, at }),
  /** 在航点播放指定原生注册 SHP 动画。 */
  playAnimationAt: (animation: AnimationType, at: Waypoint): Action => ({ kind: "playAnimationAt", animation, at }),
  /** 在航点引爆武器的 Damage/Warhead，造成范围伤害并按弹头选取动画；不创建飞行弹体。 */
  explosionAt: (weapon: WeaponType, at: Waypoint): Action => ({ kind: "explosionAt", weapon, at }),
  /** 在航点创建原生粒子系统。 */
  createParticleSystem: (system: ParticleSystemType, at: Waypoint): Action => ({ kind: "createParticleSystem", system, at }),
  /** 设置普通环境亮度百分数；风暴期间保留普通值，当前目标由引擎处理。 */
  setAmbient: (percent: number): Action => ({ kind: "setAmbient", percent }),
  /** 环境变化步长，以浮点数编码为原生 binary32 位，不是直接写整数。 */
  setAmbientStep: (value: number): Action => ({ kind: "setAmbientStep", value }),
  /** 环境变化率，以浮点数编码为原生 binary32 位。 */
  setAmbientRate: (value: number): Action => ({ kind: "setAmbientRate", value }),
  setLightingRed: (percent: number): Action => ({ kind: "setLightingRed", percent }),
  setLightingGreen: (percent: number): Action => ({ kind: "setLightingGreen", percent }),
  setLightingBlue: (percent: number): Action => ({ kind: "setLightingBlue", percent }),
  /** 请求原生 C4 impact 光照；不强制屏幕闪白，不直接造成爆炸伤害。 */
  combatFlash: (at: Waypoint, strength = CombatFlash.Medium): Action => ({ kind: "combatFlash", at, strength }),
  /** 在航点周围进行原生 C4 范围伤害，再尝试破坏桥梁；也可能伤及附近对象。 */
  damageBridgeAt: (at: Waypoint): Action => ({ kind: "damageBridgeAt", at }),
  /** 原生陨石雨，intensity为0～3档；规则须已注册METSMALL/METLARGE且提供对应ART/SHP。 */
  meteorShower: (at: Waypoint, intensity: MeteorIntensity): Action => ({ kind: "meteorShower", at, intensity }),
  /** 原生动作59连续减少航点中心的矿石；不会清除周围整片矿场。 */
  reduceTiberiumAt: (at: Waypoint): Action => ({ kind: "reduceTiberiumAt", at }),
  /** 在航点创建离子冲击；首次更新使用CombatDamage的IonCannonDamage/IonCannonWarhead伤害，普通帧中可与动作同帧执行。 */
  ionBlastAt: (at: Waypoint): Action => ({ kind: "ionBlastAt", at }),
  /** 从航点上空发射NukePayload，飞行和核爆动画后才伤害；不授予或消耗House超武。 */
  nukeAt: (at: Waypoint): Action => ({ kind: "nukeAt", at }),
  /** 从地图边缘发射已注册的ChemLauncher；规则须提供其弹体/弹头，不授予或消耗House超武。 */
  chemicalStrikeAt: (at: Waypoint): Action => ({ kind: "chemicalStrikeAt", at }),
  /** 保留原生藤蔓生长标志；不自动创建藤蔓地形。 */
  setVeinGrowth: (enabled: boolean): Action => ({ kind: "setVeinGrowth", enabled }),
  /** 开关矿石生长/扩散驱动，实际速度及条件仍由Rules/地图决定；加载时禁用形成的空队列不会自动补种。 */
  setTiberiumGrowth: (enabled: boolean): Action => ({ kind: "setTiberiumGrowth", enabled }),
  /** 保留原生冰层生长标志；不自动创建冰层地形。 */
  setIceGrowth: (enabled: boolean): Action => ({ kind: "setIceGrowth", enabled }),
  /** 翻转场景TrainCrate标志，不是在地图立即创建火车或木箱。 */
  toggleTrainCrate: (): Action => ({ kind: "toggleTrainCrate" }),
  /** 清除全部污迹身份，保留原生污迹数据字节。 */
  clearSmudges: (): Action => ({ kind: "clearSmudges" }),
  /** 标记航点上的粒子系统完成，由原生生命周期结束。 */
  markParticlesDone: (at: Waypoint): Action => ({ kind: "markParticlesDone", at }),
  /** 令选定 House 的紧急策略转入状态 4，不重置策略时钟；AI 在时钟到期后出售/猎杀，真人控制的 House 不走此策略更新。 */
  fireSale: (house: ActionHouse): Action => ({ kind: "fireSale", house }),
  enableAutocreate: (house: ActionHouse): Action => ({ kind: "enableAutocreate", house }),
  /** 开关规则所属 House 的自动基地建设。 */
  setAutoBaseBuilding: (enabled: boolean): Action => ({ kind: "setAutoBaseBuilding", enabled }),
  /** 规则所属 House 与目标互设同盟。 */
  makeAlly: (house: ActionHouse): Action => ({ kind: "makeAlly", house }),
  /** 规则所属 House 与目标互设敌对。 */
  makeEnemy: (house: ActionHouse): Action => ({ kind: "makeEnemy", house }),
  /** 设置规则所属 House 的 AI 组队比例原生整数，不重启选择计时。 */
  setAiTriggerRatio: (value: number): Action => ({ kind: "setAiTriggerRatio", value }),
  setAircraftRatio: (value: number): Action => ({ kind: "setAircraftRatio", value }),
  setInfantryRatio: (value: number): Action => ({ kind: "setInfantryRatio", value }),
  setVehicleRatio: (value: number): Action => ({ kind: "setVehicleRatio", value }),
  /** 设置首选超武目标类别；1 为原生最佳集结目标，其余按 Quarry 处理。 */
  setSuperTargetKind: (value: number): Action => ({ kind: "setSuperTargetKind", value }),
  /** 修改科技类型的运行时 TechLevel；level 为原生有符号整数。 */
  setTechLevel: (type: TechnoType, level: number): Action => ({ kind: "setTechLevel", type, level }),
  cheer: (house: ActionHouse): Action => ({ kind: "cheer", house }),
  /** 走当前单人任务结果/退出入口。 */
  endScenario: (): Action => ({ kind: "endScenario" }),
  /** 标记目标规则的活动实例为待删除。 */
  deleteRule: (rule: Rule): Action => ({ kind: "deleteRule", rule }),
  /** 删除显式 Tag 活动实例及附着关系；关联规则按原生延迟清理。 */
  deleteTag: (tag: Tag): Action => ({ kind: "deleteTag", tag }),
  /** 播放全屏影片；仅单人。资源必须存在，播放结束由宿主恢复游戏。 */
  playMovie: (movie: Movie): Action => ({ kind: "playMovie", movie }),
  /** 播放雷达影片；仅战役路径。先经过 EVA 和雷达开启阶段，pause=true 在实际播放时暂停场景；
   * 影片结束由宿主自动恢复，既有输入锁保留。字幕使用 asset 声明的 subtitlemd.txt。
   */
  playRadarMovie: (movie: Movie, pause = false): Action => ({ kind: "playRadarMovie", movie, pause }),
  /** 在航点播放位置声音。 */
  playSoundAt: (sound: Sound, at: Waypoint): Action => ({ kind: "playSoundAt", sound, at }),
  /** 在引擎随机选择的已有航点播放声音。 */
  playSoundAtRandomWaypoint: (sound: Sound): Action => ({ kind: "playSoundAtRandomWaypoint", sound }),
  /** 停止航点建筑/地形自定义声源或匹配位置声音。 */
  stopSoundAt: (at: Waypoint): Action => ({ kind: "stopSoundAt", at }),
  /** 锁定玩家输入；后续须使用 unlockInput 或受控呈现恢复。 */
  lockInput: (): Action => ({ kind: "lockInput" }),
  /** 解除玩家输入锁定。 */
  unlockInput: (): Action => ({ kind: "unlockInput" }),
  /** 单人定时暂停并自动恢复；duration 为原生单位（60 个 16ms 桶，约 0.96 秒），须大于 0。 */
  pauseScene: (duration: number): Action => ({ kind: "pauseScene", duration }),
  /** 超时空画面：两段各 duration 步的过渡，中间45步白屏；0 不启动。
   * 暂停期间由宿主服务继续推进后段，自动恢复场景；原先已锁定的输入保持锁定。
   * duration 是每段的原生处理步数，不是秒数或整段效果的逻辑帧数。 */
  chronoScreen: (duration: number): Action => ({ kind: "chronoScreen", duration }),
  /** 为首个指定类型小队的第一个成员显示气泡 1～3；0 清除。
   * 全局只有一个气泡，后一次请求替换前一次；没有该类型的活动小队时保留原气泡。
   * 持续时间来自 TalkBubbleTime，使用墙钟计时；游戏逻辑帧数不决定气泡何时消失。
   * 需要活动资源中的 talkbubl.shp 和 palette.pal；接口不自动补充原版图形。 */
  talkBubble: (team: Team, bubble: 0|1|2|3): Action => ({ kind: "talkBubble", team, bubble }),
  /** 清除当前对话气泡。 */
  clearTalkBubble: (): Action => ({ kind: "clearTalkBubble" }),
  /** C4 销毁与当前活动触发器 Tag 关联的对象；免疫对象按引擎准入处理。 */
  destroyAttached: (): Action => ({ kind: "destroyAttached" }),
  /** 出售当前 Tag 关联的建筑。 */
  sellAttachedBuildings: (): Action => ({ kind: "sellAttachedBuildings" }),
  /** 设置当前 Tag 关联建筑的在线状态；离线仅对耗电或 Powered 建筑有效，普通发电厂保持在线。 */
  setAttachedBuildingsOnline: (online: boolean): Action => ({ kind: "setAttachedBuildingsOnline", online }),
  /** 使当前 Tag 关联的步兵进入原生 Psycho 状态。 */
  makeAttachedInfantryPsycho: (): Action => ({ kind: "makeAttachedInfantryPsycho" }),
  /** 唤醒当前 Tag 关联的 Sleep/Harmless 移动对象。 */
  wakeAttached: (): Action => ({ kind: "wakeAttached" }),
  /** 唤醒所有非规则所属 House 的 Sleep 移动对象；不按敌对关系过滤。 */
  wakeOtherHouseSleepers: (): Action => ({ kind: "wakeOtherHouseSleepers" }),
  /** 唤醒规则所属 House 的 Harmless 移动对象。 */
  wakeHouseHarmless: (): Action => ({ kind: "wakeHouseHarmless" }),
  /** 唤醒指定 Group 的 Sleep/Harmless 对象，不按 House 过滤。 */
  wakeGroup: (group: number): Action => ({ kind: "wakeGroup", group }),
  /** 在航点为规则所属 House 创建建筑；遵循原生清理地基/放置准入。 */
  createBuilding: (building: BuildingType, at: Waypoint): Action => ({ kind: "createBuilding", building, at }),
  /** 释放当前 Tag 首个匹配建筑的驻军；首个建筑为空时也不继续搜索其他建筑。 */
  releaseGarrison: (): Action => ({ kind: "releaseGarrison" }),
  /** 按原生集合摧毁选定 House 对象。
   * Land/Naval 按类型 Naval 标志筛选非建筑对象，并跳过 limbo；Land 也包括非 Naval 的 Aircraft 类。
   * Naval 包括海军运输工具，与 countryNavyDestroyed 的计数口径不同。
   */
  destroyHouseObjects: (house: ActionHouse, subset = HouseObjectSubset.All): Action => ({ kind: "destroyHouseObjects", house, subset }),
  /** 规则所属 House 控制选定 House 的全部建筑；restore=true 归还此前记录由该 House 控制的建筑。 */
  controlHouseBuildings: (house: ActionHouse, restore = false): Action => ({ kind: "controlHouseBuildings", house, restore }),
  /** 从引擎开局历史恢复对象；buildingsOnly=true 修复/补建建筑，false 尝试重建四类初始对象。 */
  restoreInitialObjects: (house: ActionHouse, buildingsOnly = false): Action => ({ kind: "restoreInitialObjects", house, buildingsOnly }),
  /** 将规则所属 House 的全部对象转交选定 House。 */
  changeAllHouseObjectsOwner: (house: ActionHouse): Action => ({ kind: "changeAllHouseObjectsOwner", house }),
  /** 闪烁规则所属 House 的指定类型建筑，时间为带符号逻辑帧。 */
  flashHouseBuildings: (building: BuildingType, frames: number): Action => ({ kind: "flashHouseBuildings", building, frames }),
  /** 首次授予默认单次使用且就绪；oneShot=false 首次授予可重复超武。已有活动实例保留其单次标记，不会被重新授予转换；仍遵守发射准入。 */
  grantSuperWeapon: (weapon: SuperWeaponType, oneShot = true): Action => ({ kind: "grantSuperWeapon", weapon, oneShot }),
  /** 设置已授予超武的充能百分比 0～100；原生浮点截断可让 100% 留下 1 帧，须等待下一次超武更新才就绪。 */
  setSuperCharge: (weapon: SuperWeaponType, percent: number): Action => ({ kind: "setSuperCharge", weapon, percent }),
  /** 覆盖超武充能时间（逻辑帧）；不修改当前正在运行的计时。 */
  setSuperRecharge: (weapon: SuperWeaponType, frames: number): Action => ({ kind: "setSuperRecharge", weapon, frames }),
  /** 恢复超武类型的充能时间，不重启当前计时。 */
  resetSuperRecharge: (weapon: SuperWeaponType): Action => ({ kind: "resetSuperRecharge", weapon }),
  /** 走原生 Reset 重置超武当前充能及相关状态。 */
  resetSuperWeapon: (weapon: SuperWeaponType): Action => ({ kind: "resetSuperWeapon", weapon }),
  /** 设置规则所属 House 的首选超武目标。 */
  setSuperTarget: (at: Waypoint): Action => ({ kind: "setSuperTarget", at }),
  clearSuperTarget: (): Action => ({ kind: "clearSuperTarget" }),
  /** 设置规则所属 House 的超武防御警报位置及帧戳。 */
  setSuperDefense: (at: Waypoint): Action => ({ kind: "setSuperDefense", at }),
  clearSuperDefense: (): Action => ({ kind: "clearSuperDefense" }),
  /** 在航点创建单次云层，随后打击并退场；不启动全局风暴。 */
  lightningAt: (at: Waypoint): Action => ({ kind: "lightningAt", at }),
  /** 请求无 House 的原生全局闪电风暴；延迟来自 Rules.LightningDeferment，开始后的持续时间为逻辑帧。 */
  startLightningStorm: (frames: number): Action => ({ kind: "startLightningStorm", frames }),
  /** 请求正在活动的闪电风暴结束，等待已有云层退场；不会取消尚在倒数的风暴。 */
  stopLightningStorm: (): Action => ({ kind: "stopLightningStorm" }),
  /** 在航点启动规则所属 House 的风暴，时间/延迟来自 Rules。 */
  lightningStormAt: (at: Waypoint): Action => ({ kind: "lightningStormAt", at }),
  /** 将规则所属 House 的可移动对象超时空移动到航点。 */
  chronoshiftHouse: (at: Waypoint): Action => ({ kind: "chronoshiftHouse", at }),
  /** 在航点执行铁幕范围效果。 */
  ironCurtainAt: (at: Waypoint): Action => ({ kind: "ironCurtainAt", at }),
  /** 镜头平移至航点；velocity 为 0～4 原生速度档位，不是秒数。 */
  centerCamera: (at: Waypoint, velocity: CameraVelocity = 1): Action => ({ kind: "centerCamera", at, velocity }),
  /** 镜头立即跳到航点。 */
  jumpCamera: (at: Waypoint): Action => ({ kind: "jumpCamera", at }),
  /** 镜头回到本地玩家基地。 */
  homeCamera: (): Action => ({ kind: "homeCamera" }),
  /** 按原生固定揭示范围显示航点周围；使用当前玩家视野入口。 */
  revealAt: (at: Waypoint): Action => ({ kind: "revealAt", at }),
  /** 揭示航点所在的原生 zone；不是自定义半径。 */
  revealZone: (at: Waypoint): Action => ({ kind: "revealZone", at }),
  /** 重新遮蔽航点周围的原生固定范围。 */
  concealAt: (at: Waypoint): Action => ({ kind: "concealAt", at }),
  /** 执行原生全地图迷雾生长服务。 */
  growShroud: (): Action => ({ kind: "growShroud" }),
  /** 重新遮蔽规则所属 House 的地图。 */
  reshroudMap: (): Action => ({ kind: "reshroudMap" }),
  /** 修改 LocalSize；坐标使用 Map/Size 矩形，须完全包含于地图范围。 */
  visibleMapArea: (x: number, y: number, width: number, height: number): Action => ({ kind: "visibleMapArea", x, y, width, height }),
  /** 在航点请求客户端雷达提示。 */
  radarEvent: (event: RadarEvent, at: Waypoint): Action => ({ kind: "radarEvent", event, at }),
  /** 请求切换侧栏页。 */
  sidebarTab: (tab: SidebarTab): Action => ({ kind: "sidebarTab", tab }),
  /** 闪烁指定科技类型的侧栏图标；frames 为带符号逻辑帧。 */
  flashCameo: (type: TechnoType, frames: number): Action => ({ kind: "flashCameo", type, frames }),
  /** 规则所属 House 的雷达中断 frames 个逻辑帧。 */
  radarOutage: (frames: number): Action => ({ kind: "radarOutage", frames }),
  /** 设置活动触发器所属 House 的备用基地坐标；不移动建筑或重写基地节点。 */
  setAlternateBase: (at: Waypoint): Action => ({ kind: "setAlternateBase", at }),
  /** 清除活动触发器所属 House 的备用基地坐标。 */
  clearAlternateBase: (): Action => ({ kind: "clearAlternateBase" }),
  /** 让选择的 House 全部单位进入原生猎杀。 */
  allToHunt: (house: ActionHouse): Action => ({ kind: "allToHunt", house }),
  /** 开启所选 House 的脚本生产；不会创建不存在的工厂。 */
  startProduction: (house: ActionHouse): Action => ({ kind: "startProduction", house }),
  /** 开启 House AI 小队选择；AITriggerTypesEnable/条件及工厂仍单独生效。 */
  enableAi: (house: ActionHouse): Action => ({ kind: "enableAi", house }),
  /** 停止 House AI 小队选择，保留已出队小队。 */
  disableAi: (house: ActionHouse): Action => ({ kind: "disableAi", house }),
  /** 转移挂接该活动触发器的有效对象到所选 House；需对象/小队 Tag 绑定。 */
  changeAttachedOwner: (house: ActionHouse): Action => ({ kind: "changeAttachedOwner", house }),
  /** 比较指定 Country 与当前本地玩家；相同则玩家胜利，不同则玩家失败。仅单人任务。 */
  winCountry: (country: Country): Action => ({ kind: "winCountry", country }),
  /** 比较指定 Country 与当前本地玩家；相同则玩家失败，不同则玩家胜利。仅单人任务。 */
  loseCountry: (country: Country): Action => ({ kind: "loseCountry", country }),
  /** 按活动声音 ID 播放；需发布声音目录及其样本。 */
  playSound: (sound: Sound): Action => ({ kind: "playSound", sound }),
  /** 按活动 THEME 曲目 ID 播放音乐。 */
  playMusic: (theme: Theme): Action => ({ kind: "playMusic", theme }),
  /** 按活动 EVA 事件 ID 播放语音。 */
  playEva: (eva: Eva): Action => ({ kind: "playEva", eva }),
  /** 移除该模板的活动 Team 并解散成员；不会直接杀死成员。 */
  deleteTeam: (team: Team): Action => ({ kind: "deleteTeam", team }),
  /** 使用 Team 模板默认航点普通增援；区别于指定 at 的 reinforce。 */
  reinforceDefault: (team: Team): Action => ({ kind: "reinforceDefault", team }),
  /** 在指定航点超时空增援；入场条件由实际运输/超时空生产路径处理。 */
  chronoReinforce: (team: Team, at: Waypoint): Action => ({ kind: "chronoReinforce", team, at }),
  /** 闪烁该模板的活动小队成员，持续 frames 逻辑帧（15 帧/秒），允许带符号原生值。 */
  flashTeam: (team: Team, frames: number): Action => ({ kind: "flashTeam", team, frames }),
  /** 揭示整张地图，呈现受游戏模式和本地显示流程控制。 */
  revealMap: (): Action => ({ kind: "revealMap" }),
  /** 在航点创建投放区信标；信标效果使用活动规则资源。 */
  dropZone: (at: Waypoint): Action => ({ kind: "dropZone", at }),
  /**
   * 在航点放置指定木箱；位置/模式准入由游戏决定，创建本身不触发拾取事件。
   * Random 使用原生随机选择。单人 Money 还受 CrateRules 的图像对应奖励及 SoloCrateMoney 控制；多人另有资格回退。
   * 拾取通知要求实际覆盖物 CrateTrigger=yes；关闭此标志仍可领取奖励。
   */
  createCrate: (crate: Crate, at: Waypoint): Action => ({ kind: "createCrate", crate, at }),
  /** 启用目标规则并重置其计时；遵守难度，不复活已删除的活动规则。 */
  enableRule: (rule: Rule): Action => ({ kind: "enableRule", rule }),
  /** 禁用目标规则，保留实例。 */
  disableRule: (rule: Rule): Action => ({ kind: "disableRule", rule }),
  /** 同步执行已启用规则的动作，跳过条件；不消费 once、不自动重置，拒绝递归强制触发链。 */
  forceRule: (rule: Rule): Action => ({ kind: "forceRule", rule }),
  /** 设置全局状态，供后续关卡使用；实际跨关保存由游戏战役流程负责。 */
  setGlobal: (flag: GlobalFlag): Action => ({ kind: "setGlobal", flag }),
  /** 清除全局状态。 */
  clearGlobal: (flag: GlobalFlag): Action => ({ kind: "clearGlobal", flag }),
  /**
   * 创建“在指定航点增援小队”的动作，使用已声明的小队模板。
   * @param team 本任务通过 m.team() 声明的小队，成员、归属和行为来自模板。
   * @param at 增援航点，通过 m.waypoint() 取得；该位置写入增援动作参数。
   * @returns Action 描述；调用本方法时不会生成单位。
   * @remarks 对应原生指定航点增援，入场方式、可用位置和组队限制由游戏处理。
   * 不会等待小队完成行为；同一模板可重复增援，旧小队仍存活时也可创建新小队。
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
   * 设置剩余时间并立即启动任务倒计时，游戏在右下角显示时间。
   * @param seconds 游戏逻辑秒数，须为 0～143165576 的整数，按每秒 15 逻辑帧解释。
   * @returns Action 描述；每张地图只有一个共享任务倒计时，支持单人和多人模式。
   * @remarks 每次设置都会覆盖旧时间并清空标题；需要标题时在此动作之后调用 setCountdownTitle()。
   * 0 表示立即到期。此方法不等待、不自动增援，也不自动宣告胜负。
   * @example action.setCountdown(30)
   */
  setCountdown: (seconds: number): Action => ({ kind: "setCountdown", seconds }),
  /**
   * 继续已停止的共享任务倒计时，并恢复右下角显示，保留剩余时间和标题。
   * @returns Action 描述；开始新的倒计时应使用 setCountdown(seconds)。
   */
  startCountdown: (): Action => ({ kind: "startCountdown" }),
  /**
   * 停止共享任务倒计时并隐藏右下角显示，保留剩余时间和标题。
   * @returns Action 描述；停止期间 countdownElapsed() 不会满足，可用 startCountdown() 继续。
   */
  stopCountdown: (): Action => ({ kind: "stopCountdown" }),
  /**
   * 给共享任务倒计时增加时间，保留标题。
   * @param seconds 增加的游戏逻辑秒数，须为 0～143165576 的整数。
   * @returns Action 描述；按原生语义，修改时间也会恢复已停止的倒计时。
   */
  extendCountdown: (seconds: number): Action => ({ kind: "extendCountdown", seconds }),
  /**
   * 给共享任务倒计时减少时间，保留标题；不足指定时间时归零。
   * @param seconds 减少的游戏逻辑秒数，须为 0～143165576 的整数。
   * @returns Action 描述；按原生语义，修改时间也会恢复已停止的倒计时。
   */
  shortenCountdown: (seconds: number): Action => ({ kind: "shortenCountdown", seconds }),
  /**
   * 设置右下角任务倒计时的标题，显示在剩余时间旁边。
   * @param text 通过 m.text() 取得的已有 CSF 标签引用，不接受直接输入的中文原文。
   * @returns Action 描述；不启动倒计时，标签及其资源须随地图发布。
   * @remarks setCountdown() 会清空标题；动作顺序应为先设置时间，再设置标题。
   * @example action.setCountdownTitle(m.text(Text.MISSION_TIMER))
   */
  setCountdownTitle: (text: Text): Action => ({ kind: "setCountdownTitle", text }),
  /**
   * 创建将局部标记置为 true 的动作，用于解锁其他规则条件。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Action 描述；修改发生在游戏中，不会改变构建阶段的 JS 变量。
   * @example action.setFlag(objectiveDone)
   */
  setFlag: (flag: Flag | LocalFlag): Action => flag.kind === "localFlag"
    ? { kind: "setLocal", flag } : { kind: "setFlag", flag },
  /**
   * 创建将局部标记置为 false 的动作，可让 event.flagClear() 条件满足。
   * @param flag 本任务通过 m.flag() 声明的标记。
   * @returns Action 描述；一次性规则执行后不会因此重新启用，重复规则仍按其条件判断。
   * @example action.clearFlag(alarm)
   */
  clearFlag: (flag: Flag | LocalFlag): Action => flag.kind === "localFlag"
    ? { kind: "clearLocal", flag } : { kind: "clearFlag", flag },
  /**
   * 创建宣告玩家胜利的原生动作。
   * @returns 无需参数的 Action 描述，在规则触发时交由游戏处理胜利流程。
   * @remarks 仅支持单人模式。以当前本地玩家为结果对象，立即设置结果并跳过 SavourDelay 等待；不作用于触发器 owner。
   * @example action.winPlayer()
   */
  winPlayer: (): Action => ({ kind: "winPlayer" }),
  /**
   * 创建宣告玩家失败的原生动作。
   * @returns 无需参数的 Action 描述，在规则触发时交由游戏处理失败流程。
   * @remarks 仅支持单人模式。以当前本地玩家为结果对象，立即设置失败并跳过 SavourDelay 等待；不作用于触发器 owner。
   * @example action.losePlayer()
   */
  losePlayer: (): Action => ({ kind: "losePlayer" }),
};

/** 战役关卡。地图、脚本及资源路径均相对 campaign.ts 所在目录。 */
export interface CampaignMission {
  readonly file: string;
  readonly source: string;
  readonly script?: string;
  readonly side?: "GDI" | "Nod" | "ThirdSide";
  /** 可直接写中文，构建器生成短 CSF 标签。 */
  readonly title?: string;
  readonly briefing?: string;
  readonly loadMessage?: string;
  readonly loadBriefing?: string;
  readonly background640?: string;
  readonly background800?: string;
  readonly backgroundPalette?: string;
  /** 全局变量 0 清除时前往 next，设置时前往 alternate（默认同 next）。 */
  readonly next?: string;
  readonly alternate?: string;
  readonly endOfGame?: boolean;
}
export interface CampaignOptions {
  readonly id: string;
  readonly entry: string;
  readonly missions: readonly CampaignMission[];
  readonly assets?: readonly { readonly name: string; readonly source: string }[];
}
/** 独立战役入口；宿主先构建全部关卡，再准备原生目录、CSF 与资源，任一失败均不发布。 */
export function defineCampaign(options: CampaignOptions) {
  const strings: Record<string,string> = {}, scripts: Record<string,string> = {}, transitions: Record<string,unknown> = {};
  function label(file: string, field: string, value?: string): string {
    if (value === undefined) return "";
    let hash=0x811c9dc5;
    for (const c of options.id+"/"+file+"/"+field) hash=Math.imul(hash^c.charCodeAt(0),0x01000193)>>>0;
    const key="TSCP"+("00000000"+hash.toString(16).toUpperCase()).slice(-8);
    if (key in strings) throw new Error("Campaign text ID collision: "+key);
    strings[key]=value;return key;
  }
  const missions=options.missions.map(m=>{
    if(m.script!==undefined) scripts[m.file]=m.script;
    if(m.next!==undefined || m.alternate!==undefined || m.endOfGame!==undefined)
      transitions[m.file]={next:m.next,alternate:m.alternate,endOfGame:m.endOfGame??false};
    return {file:m.file,source:m.source,side:m.side??"GDI",ui_name:label(m.file,"title",m.title),briefing:label(m.file,"briefing",m.briefing),
      load_message:label(m.file,"loadMessage",m.loadMessage),load_briefing:label(m.file,"loadBriefing",m.loadBriefing),
      background_640:m.background640??"",background_800:m.background800??"",background_palette:m.backgroundPalette??""};
  });
  return {project:{version:1,id:options.id,entry:options.entry,missions,strings,assets:options.assets??[]},scripts,transitions};
}
