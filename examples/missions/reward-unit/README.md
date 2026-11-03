# 奖励车辆属性 demo

[mission.ts](mission.ts) 是完整任务：建筑栏购买补给箱、放置、Ctrl 强制攻击拆毁、在 `rewardAt` 航点获得强化天启或侦察车。默认属性已经启用；运输、飞行、采矿等互有前提的配置保留为注释，按需选用。

1. 将 `mission.ts` 复制到自己的地图任务目录，重新加载地图资源，刷新 SDK 类型。
2. 使用单人地图，存在 `Americans` House、建造厂和对应资源；玩家 Country 不能与其他 House 共用，不能预放 GAPILL。Mod 名称不同时修改生成常量。
3. 构建并试玩。普通 `HTNK` / `FV` 不会变成强化版，只有抽奖池引用的副本使用修改值。
4. 将 `rewardAt` 的坐标改成地图上空旷可通行的奖励到达位置，或换成已有航点。它不是建筑的放置坐标。同时最多保留一座，拆毁结算后可再次购买。

| 属性类别 | demo 字段示例 | 说明 |
| --- | --- | --- |
| 生命与装甲 | Strength、Armor、SelfHealing | Armor 是类别；自修复速率仍受全局规则影响 |
| 免疫 | ImmuneToRadiation、ImmuneToPsionics、Warpable、Parasiteable | 心控免疫与心灵武器伤害免疫不同 |
| 移动与视野 | Speed、ROT、Sight、Weight、Accelerates | 沿用原生单位与地形约束 |
| 碾压 | Crusher、Crushable、OmniCrusher、OmniCrushResistant | 普通碾压与全能碾压分别控制 |
| 武器和目标 | Primary、Secondary、ElitePrimary、NavalTargeting | 引用已有武器，不创建新武器 |
| 战斗行为 | CanRetaliate、CanPassiveAquire、OpportunityFire | 键名保留原生拼写 |
| 弹药与装填 | Ammo、InitialAmmo、Reload、EmptyReload | 有对应原生装填机制才起效 |
| 老兵和精英 | Trainable、VeteranAbilities、EliteAbilities | 能力列表不会让单位出生即三星 |
| 隐身与探测 | Cloakable、CloakStop、Sensors、SensorsSight | 隐身与反隐是独立机制 |
| 运输 | Passengers、Size、SizeLimit、OpenTopped | 建议基于运输车；IFV 还需武器、炮塔配套 |
| 展开 | DeploysInto、IsSimpleDeployer、DeployFire | 目标建筑使用自己的属性，不自动继承奖励车属性 |
| 采矿 | Harvester、Storage、HarvestRate、Dock | 建议基于原生矿车，不要随意混搭 |
| 飞行车辆 | JumpjetSpeed、JumpjetHeight、BalloonHover | 须使用对应移动器与模型；不支持飞机奖励池 |
| 子机或奴隶 | Spawns、SpawnsNumber、Slaves、SlavesNumber | 需要完整配套单位和武器机制 |
| 经济和交互 | Cost、Soylent、Points、Selectable、Crewed | 奖励车 Cost 不等于补给箱购买价格 |
| 外观与声音 | Image、VoiceSelect、MoveSound、MaxDebris | 引用已有资源，SDK 不生成美术音频 |

`rules` 接受原生单位规则的键值，未覆盖的基础字段自动复制，不需要把每个字段重新写一遍。该 demo 覆盖常见可改属性及特殊能力写法，不是引擎所有版本、所有 Mod 扩展字段的完整实现清单。未识别的键可能被原生读取器忽略；建筑/步兵专用键不会因为写进车辆 section 就变成车辆能力。

- 值只接受 `string | number | boolean`。列表写成 `'A,B,C'`；boolean 写成 `true/false`；清空引用写 `'<none>'`。不要写数组、对象、null 或包含换行/分号的值。
- 使用原生键名的标准大小写。`TechLevel`、`CrateGoodie`、`AllowedToStartInMultiplayer`、`UIName` 由 SDK 管理，名称用 `name`。
- **Damage、ROF、Range、Warhead、Projectile 属于武器规则。** 要做仅奖励版使用的特殊武器，应在活动 Mod 中准备独立武器，再通过 `Primary` / `Secondary` 引用；不能在这里写 `Damage: 500`。
- **Voxel、Cameo、PrimaryFireFLH 等属于 ART。** `Image` 可以换成已有的完整 ART；SDK 不克隆 ART，也不生成木箱图标。
- 返回值 `RewardUnit` 只用于 `crateLottery.rewards`；最多 64 种独立奖励类型，不提供同一类型逐实例不同属性或单独随机权重。
- 副本规则在构建时复制；切换资源或修改基础规则后应重新加载、刷新并构建。复杂 Mod 的路径相关默认值需要实际验证，不能保证所有类型仅复制文本就与原类型完全相同。
- 本功能只修改编辑器和地图规则，不修改引擎，也不改普通箱子的奖励。奖励副本设置 `CrateGoodie=no`；发奖不经过箱子补矿车逻辑。
- 触发器检测玩家建筑“存在 → 不存在”，脚本移除也会触发。奖励固定归任务玩家，通过超时空增援到达航点；不能保证出现在建筑原位置，放置受阻时不退款。
- 使用原生随机计时器竞争选奖，存在短暂延迟，同秒到期按 Tag 顺序决胜，不保证等概率。
