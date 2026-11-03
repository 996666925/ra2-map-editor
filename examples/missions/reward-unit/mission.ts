import { defineMission } from './sdk';
import { map, House, Building, Unit } from './map.generated';

// 将本文件复制到地图的任务目录，然后用编辑器“刷新 SDK 类型”“构建”。
// 这是构建期规则，所有抽到该奖励类型的车辆都使用这些值。
export default defineMission(map, { id: 'reward_demo', player: House.Americans }, m => {
  const apocalypse = m.rewardUnit('prize_apocalypse', {
    base: m.unit(Unit.HTNK),
    name: '奖励天启',
    rules: {
      // 生命、防御：Armor 是装甲类别，不是减伤百分比。
      Strength: 1500,
      Armor: 'heavy', // none / flak / plate / light / medium / heavy / wood / steel / concrete 等
      SelfHealing: true, // 修复速率与上限仍按原生全局规则
      Immune: false, // true 为广义免伤；通常不要打开
      TypeImmune: false, // 同类型武器伤害免疫判定
      ImmuneToRadiation: true,
      ImmuneToPsionics: true, // 心灵控制免疫
      ImmuneToPsionicWeapons: false, // 心灵武器伤害免疫，与心控免疫不同
      ImmuneToPoison: true,
      Warpable: true, // 能否被超时空抹除
      Bombable: true, // 能否被伊文炸弹附着
      Parasiteable: false, // 能否被恐怖机器人寄生
      Organic: false, // 是否按有机单位处理
      OmniCrushResistant: true, // 抵抗战斗要塞一类的全能碾压

      // 移动、视野、转向：Speed 不是现实 km/h；ROT 为原生转向参数。
      Speed: 8,
      ROT: 5,
      Sight: 10,
      Weight: 3.5,
      Accelerates: true,
      MoveToShroud: true,
      TooBigToFitUnderBridge: false,
      Crusher: true, // 碾压普通可碾压目标
      Crushable: false,
      OmniCrusher: false, // 全能碾压，需按玩法选择
      TiltsWhenCrushes: true,

      // 战斗行为：武器及炮塔默认完整继承基础车辆。
      CanRetaliate: true,
      CanPassiveAquire: true, // 原生拼写就是 Aquire
      OpportunityFire: true,
      PreventAttackMove: false,
      DistributedFire: false,
      Turret: true, // 模型必须有配套炮塔
      TurretSpins: false,
      TargetLaser: false,
      Explodes: false, // 开启时要有合适的死亡武器
      DeathWeaponDamageModifier: 1.0,
      ThreatPosed: 60, // AI 威胁评分，不是伤害
      SpecialThreatValue: 1.0,
      LeadershipRating: 8,

      // 升级：决定允许哪些老兵/精英能力，不表示出生即三星。
      Trainable: true,
      VeteranAbilities: 'STRONGER,FIREPOWER,ROF,SIGHT,FASTER',
      EliteAbilities: 'SELF_HEAL,STRONGER,FIREPOWER,ROF',

      // 隐身、探测：此 demo 关闭自身隐身，保留反隐探测。
      Cloakable: false,
      CloakingSpeed: 7,
      CloakStop: true,
      Sensors: true,
      SensorsSight: 5,
      DetectDisguiseRange: 5,

      // 经济、计分、交互：Cost 也会影响原生奖励资格等逻辑。
      Cost: 2000,
      Soylent: 1000, // 回收价值，具体处理受回收机制影响
      Points: 80,
      DontScore: false,
      Insignificant: false,
      Selectable: true,
      Crewed: true,
      Repairable: true,
      Bunkerable: true,
      Size: 6, // 自身占运输空间的尺寸
      NoShadow: false,
      PixelSelectionBracketDelta: 0,

      // 以下是不同能力的可选写法；按需取消注释，不要全部同时开启。
      // 字段引用须使用当前活动资源中存在的原生 ID。

      // ── 武器切换（当前资源须已有该武器）──
      // Primary: '120mmx',
      // Secondary: 'MammothTusk',
      // ElitePrimary: '120mmxE',
      // EliteSecondary: 'MammothTuskE',
      // DeathWeapon: '<none>', // 清空引用用 <none>，不能用空字符串
      // BurstDelay0: 0,
      // BurstDelay1: 8,
      // NavalTargeting: 0, // 原生海军目标策略枚举
      // LandTargeting: 0, // 原生陆地目标策略枚举
      // SprayAttack: false,
      // FireAngle: 8,
      // DamageSelf: false,
      // 注意：Damage / ROF / Range / Warhead / Projectile 是武器字段。
      // 不能在这里写 Damage: 500 来提高攻击；应引用 Mod 已定义的专用武器。

      // ── 有限弹药（需原生弹药/装填机制支持）──
      // Ammo: 12, // -1 为无限
      // InitialAmmo: 12, // -1 表示沿用 Ammo
      // Reload: 30, // 原生帧
      // EmptyReload: 60,
      // ReloadIncrement: 1,
      // ManualReload: false,

      // ── 运输车（建议以 HTK / BFRT 为 base，保留其装卸机制）──
      // Passengers: 6,
      // SizeLimit: 2, // 允许乘客的最大单体尺寸
      // OpenTopped: true, // 乘客可从车内开火，武器仍须适配
      // PipScale: 'Passengers',
      // PipsDrawForAll: false,
      // PipWrap: 6,
      // EnterTransportSound: '<none>',
      // LeaveTransportSound: '<none>',
      // Gunner / WeaponCount / Weapon1..N / EliteWeapon1..N / TurretCount
      // 应与 FV 的多武器和炮塔规则一起配置；只改一个布尔值不能完成 IFV 模式。

      // ── 水陆移动（建议选已有水陆/悬浮车辆作 base）──
      // Locomotor: '{4A582741-9839-11d1-B709-00A024DDAFD1}', // 普通履带移动
      // SpeedType: 'Track',
      // MovementZone: 'Normal',
      // MovementRestrictedTo: 'none',
      // Naval: false,
      // Underwater: false,
      // 不要只改 Naval=true；移动器、地形资格、ART 与武器都需要匹配。

      // ── 飞行车辆（仅用于 VehicleTypes 中已有飞行机制的车辆，如 ZEP）──
      // JumpjetSpeed: 16,
      // JumpjetClimb: 5,
      // JumpjetCrash: 15,
      // JumpjetAccel: 2,
      // JumpjetTurnRate: 4,
      // JumpjetHeight: 750,
      // JumpjetWobbles: 0.01,
      // JumpjetDeviation: 1,
      // JumpjetNoWobbles: true,
      // BalloonHover: true,
      // HoverAttack: true,
      // ConsideredAircraft: true,
      // Crashable: true,
      // 配套 Jumpjet Locomotor / ART 必须继承或由 Mod 提供。

      // ── 展开（建议以已支持展开的车辆为 base）──
      // DeploysInto: 'GAARTY', // 必须是资源中已有的建筑，示例名需按 Mod 替换
      // IsSimpleDeployer: true,
      // DeployToLand: false,
      // DeployFacing: 2, // 0..7
      // DeployFire: true,
      // DeployFireWeapon: 1,
      // UndeployDelay: 30,
      // DeploySound: '<none>',
      // UndeploySound: '<none>',
      // 展开成现有建筑后使用该建筑规则，奖励车的修改不会自动传给建筑。

      // ── 采矿（建议以 HARV / CMIN 为 base）──
      // Harvester: true,
      // Storage: 40,
      // HarvestRate: 1,
      // ResourceGatherer: true,
      // Dock: 'NAREFN,GAREFN',
      // UnloadingClass: 'HARV',
      // 还须保留矿车的移动器、卸载和 ART；不要把它与普通战车配置混用。

      // ── 召唤子机/奴隶（需专用武器和子单位机制）──
      // Spawns: 'HORNET',
      // SpawnsNumber: 3,
      // SpawnRegenRate: 600,
      // SpawnReloadRate: 150,
      // MissileSpawn: false,
      // NoSpawnAlt: false,
      // Slaves: 'SLAV',
      // SlavesNumber: 5,
      // SlaveRegenRate: 500,
      // SlaveReloadRate: 150,

      // ── 外观、声音（引用已有资源，不生成模型/音频）──
      // Image: 'HTNK', // RULES Image 指向完整已有 ART，默认保留基础单位 Image
      // VoiceSelect: 'ApocalypseSelect',
      // VoiceMove: 'ApocalypseMove',
      // VoiceAttack: 'ApocalypseAttackCommand',
      // VoiceFeedback: '<none>',
      // MoveSound: 'ApocalypseMoveStart',
      // DieSound: 'GenVehicleDie',
      // CrushSound: 'TankCrush',
      // MaxDebris: 3,
      // MinDebris: 1,
      // DebrisTypes: 'PIECE,GASTANK',
      // DebrisMaximums: '2,1',
      // Voxel / Cameo / PrimaryFireFLH 等 ART 字段不能直接放在 rules 里生效。

      // SDK 管理且禁止覆盖：TechLevel、CrateGoodie、AllowedToStartInMultiplayer、UIName。
      // 名称用上面的 name 设置；新类型不进入普通生产栏或开局随机兵种。
    },
  });

  // 同一基础类型可以做多个互不影响的奖励版本。
  const scout = m.rewardUnit('prize_scout', {
    base: m.unit(Unit.FV),
    name: '奖励侦察车',
    rules: { Strength: 600, Speed: 12, Sight: 14, Sensors: true, SensorsSight: 8 },
  });

  m.crateLottery('supply', {
    ticket: m.building(Building.GAPILL),
    rewardAt: m.createWaypoint(30, 48, 60), // 改为地图上可通行的奖励到达位置
    name: '奖励补给箱（攻击拆开）',
    rewards: [apocalypse, scout],
    cost: 500,
    strength: 20,
    prerequisites: [],
  });
});
