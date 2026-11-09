# TypeScript 任务制作

## Ares 任务

使用启用 `ares` feature 的编辑器和 VERA20k 客户端。默认编辑器在构建阶段拒绝 Ares SDK 调用，不会将不支持的编号写入任务。支持的事件、动作、绑定和参数可从能力清单查询，`runtime: "ares"` 表示扩展能力，`enabled` 表示本次编辑器构建已启用它。事件 66 缺少运行时通知，仍不提供 SDK 方法。

```ts
import { ares } from './sdk';

// 放入 defineMission 的回调；techno 为 m.techno(...) 返回的类型引用，
// country 为 m.country(...) 返回的目标国家引用。
m.rule('reserve-power', {
  when: ares.event.ownsAtLeast(techno, 3),
  once: true,
  actions: [ares.action.addAuxPower(country, 100)],
});
```

`ares.event` 提供 EMP、驾驶员、接管、劫持、超武、逆向工程、类型数量、攻击/摧毁和 KeepAlive 条件。指定来源 House 的事件使用 House 数组索引，SDK 只允许单人任务；`noKeepAlive` / `noKeepAliveBuildings` 使用 Country 或多人起点选择器，不接受保留 House。House 类事件的所属方由规则 `owner` 决定。

`ares.action` 提供 `firestorm`、`addAuxPower`、`killDrivers`、`setEvaVoice`、`setGroup`；分组和驾驶员动作依赖规则的附着对象。`ares.order` 提供 `enterOccupiableVehicle` 与 `convertScript`；后者读取单位类型的 `Convert.Script`。EVA 语音数量由实际客户端检查。

本地资源 INI 的 include、继承和迭代键交给引擎解析。地图中的继承或迭代键仍保留原文，物理编辑前需展开为显式键值。默认 YR 与 Ares 的资源配置 schema 不混用。

[SDK 扩展计划与功能清单](../../docs/mission-sdk-plan.zh-CN.md)

编辑器内置纯 Rust SWC 转译器和 Boa 0.22 引擎。类型检查由外部代码编辑器完成，
“应用到地图”只转译、执行脚本并校验生成的地图数据，不执行 TS 类型检查。
运行时不需要 Node.js、Deno、npm、tsc 或网络，也不使用系统安装的这些程序。

## 在编辑器中使用

1. 打开地图，在“任务制作”中配置玩家 House 和需要的航点。
2. 点击顶栏“TypeScript 任务”→“新建…”。模板会在场景第 300 秒宣告胜利；已有脚本可从“选择脚本”下拉列表选取，也可直接填写完整路径。
3. 在外部编辑器中编辑、检查类型并保存源码，点击“应用到地图”，转译和地图校验通过后自动应用。
4. 关闭任务窗口并保存地图；单人脚本使用“单人任务”试玩，多人脚本按遭遇战或联机地图运行。

转译和执行在进程内的后台线程完成，不启动额外编译进程。SDK 只转译一次，后续构建复用未修改模块的 JS，
只转译当前入口引用的运行时模块；类型导入和 `.d.ts` 声明交给外部代码编辑器。
源码完全未变时直接复用成功的任务描述；每次应用仍重新验证当前地图、托管记录冲突和源码快照，
保留原有的撤销与过期结果检查。缓存只保留在当前后台线程内。
任务窗口只提供新建、刷新类型和应用到地图，不包含源码编辑器。
每次应用读取磁盘中已保存的源码，结果形成一个地图撤销步骤。
下拉列表显示当前脚本目录中的 `.ts` 文件；尚未指定脚本时，使用当前地图所在目录，
未保存地图则使用程序工作目录。不列出 `sdk.ts`、`map.generated.ts` 和 `.d.ts` 声明文件。
每次展开列表都会读取目录，外部新增的脚本也可直接选取。

源码目录会生成 `sdk.ts` 和 `map.generated.ts`，并在没有现有配置时创建 `tsconfig.json`。
用 VS Code 等支持 TypeScript 的代码编辑器打开该目录，即可使用严格类型检查、中文悬停注释和补全；
配置包含 `strict`、`noUncheckedIndexedAccess`、`exactOptionalPropertyTypes` 和 `isolatedModules`。
刷新类型不会覆盖已有 `tsconfig.json`；已有配置请自行开启这些选项。`isolatedModules` 用于提示单文件转译的限制。
`map.generated.ts` 导出 `House`、`Waypoint`、`Unit`、`MapObject`、`Text` 五类枚举和 `map` 目录，
分别用于所属方、航点、步兵/车辆/飞机类型、具体地图对象和 CSF 文本标签。构建时重新生成，
SDK 和 Rust 会校验实际使用的地图引用。修改地图后可点击“刷新类型”更新外部编辑器提示，
不必应用任务；保存地图或在外部编辑器保存源码不会自动刷新类型。
目录数组使用对应枚举的只读数组类型，直接从枚举生成，避免几千条文本标签被重复展开成超长元组；
空目录保持 `readonly never[]`，不存在的成员和跨类别参数仍会产生类型错误。

每个枚举成员上方都有中文 JSDoc。输入 `Unit.` 可补全单位，悬停 `Unit.HTNK` 可看到
“车辆：犀牛坦克（HTNK）”，`Unit.TTNK` 对应“磁能坦克”。名称优先使用当前资源的
本地化显示名；原版英文名称有内置中文回退，Mod 自定义名称保留。对象注释包含原始记录键、
所属方和坐标，航点注释包含坐标；文本注释显示标签及其内容。

```ts
import { map, House, Unit, Waypoint, MapObject, Text } from './map.generated';

// 以下成员须在当前地图或资源中存在；输入枚举名和点号即可查看补全及中文注释。
const tank = m.unit(Unit.HTNK);
const enemy = m.house(House.Russians);
const entry = m.waypoint(Waypoint.WP20);
const target = m.object(MapObject.Structures_0);
const intro = m.text(Text.MISSION_INTRO);
```

成员名将标点替换为下划线，数字航点加 `WP` 前缀，重名时追加数字后缀；
枚举值始终保留原始 ID 或编号，不修改地图数据。旧脚本中的 `m.unit('HTNK')`、
`player: 'Americans'` 等字符串引用应改为枚举。任务、规则、小队和标记的自定义逻辑名称仍为字符串。

## 示例

以下代码假定地图有 `Americans`、`Russians` 两个 House、20～22 号航点和对应单位类型。
把它写入新建的任务源码；从同目录的生成文件中导入 API 和地图类型。

```ts
import { defineMission, event, action, order } from './sdk';
import { map, House, Unit, Waypoint } from './map.generated';

export default defineMission(map, {
  id: 'bridgehead',
  player: House.Americans,
}, m => {
  const assault = m.team('assault', {
    house: m.house(House.Russians),
    waypoint: m.waypoint(Waypoint.WP20),
    units: [
      { type: m.unit(Unit.E2), count: 6 },
      { type: m.unit(Unit.HTNK), count: 2 },
    ],
    orders: [
      order.moveTo(m.waypoint(Waypoint.WP21)),
      order.attackWaypoint(m.waypoint(Waypoint.WP22)),
    ],
  });

  for (const seconds of [60, 120, 180]) {
    m.rule(`wave_${seconds}`, {
      when: event.sinceScenarioStart(seconds),
      once: true,
      actions: [action.reinforce(assault, m.waypoint(Waypoint.WP20))],
    });
  }
});
```

`m.unit('HTNK')`、`m.unit(House.Americans)`、`order.moveTo(m.house(House.Americans))`
会在外部代码编辑器中产生 TS 类型错误。应用时不会拦截仅涉及类型的错误，例如未使用变量的错误类型标注。
实际执行的引用由 SDK 检查，任务数据结构、数字范围、原生容量和外部工具改动等由 Rust 校验；
`as any` 不会绕过这些地图校验。转译语法错误包含文件、行列；执行及地图数据错误单独报告。

## 每隔 30 秒增援小队

使用 `once: false` 和 `event.sinceEnabled(30)` 创建持续重复的规则。
首次在开局后 30 个游戏秒触发，此后每次触发都重新计时；不需要用 JS 循环预先生成有限波次。
下面示例要求地图已有 `Americans`、`Russians` 两个 House、20 号航点和 `HTNK` 单位类型：

```ts
import { defineMission, event, action, order } from './sdk';
import { map, House, Unit, Waypoint } from './map.generated';

export default defineMission(map, {
  id: 'reinforcement_loop',
  player: House.Americans,
}, m => {
  const entry = m.waypoint(Waypoint.WP20);
  const assault = m.team('assault', {
    house: m.house(House.Russians),
    waypoint: entry,
    units: [{ type: m.unit(Unit.HTNK), count: 3 }],
    orders: [order.hunt()],
  });

  m.rule('reinforce_every_30_seconds', {
    when: event.sinceEnabled(30),
    once: false,
    actions: [action.reinforce(assault, entry)],
  });
});
```

同一小队模板可重复增援，上一波仍存活也可以继续出兵；增援的实际入场位置由游戏处理。
多人地图同样适用，改用 `MissionMode.Multiplayer` 和 `house: m.playerAt(...)` 指定所属玩家即可。
点击“刷新类型”或“应用到地图”会更新源码目录中的 `sdk.ts`，让外部编辑器识别 `once: false`。

计时按每秒 15 逻辑帧计算，会受游戏速度和暂停影响。间隔使用正整数秒；0 表示没有等待间隔。
`sinceScenarioStart(30)` 是“开局已满 30 秒”，达到后持续满足，不会重新计时。
`flagSet` / `flagClear` 等状态条件搭配重复规则时，也会在条件持续满足期间反复执行，
并不只响应状态变化。`once: true` 保持原有的一次性行为。

## 在右下角显示出兵倒计时

使用共享任务倒计时，可以让右下角的显示和出兵使用同一个计时器。
将上例中的 `reinforce_every_30_seconds` 规则替换为以下代码，保留 `assault` 和 `entry` 的声明：

```ts
const resetCountdown = [action.setCountdown(30)];

m.rule('start_countdown', {
  when: event.sinceScenarioStart(0),
  once: true,
  actions: resetCountdown,
});

m.rule('next_wave', {
  when: event.countdownElapsed(),
  once: false,
  actions: [
    action.reinforce(assault, entry),
    ...resetCountdown,
  ],
});
```

开局显示 30 秒倒计时；每次归零后增援并重新倒数 30 秒。
这里使用 `countdownElapsed()` 驱动出兵，无需另外维护 `sinceEnabled(30)` 规则。
所有动作仍由游戏执行，TS 不需要每秒刷新文字。

需要在数字旁显示标题时，先确保当前资源中已有对应 CSF 标签。
例如已加载 `MISSION:TIMER`，从 `map.generated.ts` 导入 `Text`，将上述数组改为：

```ts
const resetCountdown = [
  action.setCountdown(30),
  action.setCountdownTitle(m.text(Text.MISSION_TIMER)),
];
```

标题使用 CSF 标签引用，不能直接传入中文原文。**每次 `setCountdown()` 都会清空标题**，
所以顺序应为先设置时间、再设置标题，并在每轮重新设置。

| API | 行为 |
| --- | --- |
| `action.setCountdown(seconds)` | 覆盖剩余时间并立即启动、显示，同时清空标题 |
| `action.setCountdownTitle(text)` | 设置已有 CSF 标签作为标题，不改变计时状态 |
| `action.stopCountdown()` | 停止并隐藏，保留剩余时间和标题 |
| `action.startCountdown()` | 继续停止的倒计时并恢复显示 |
| `action.extendCountdown(seconds)` | 增加时间并恢复运行，保留标题 |
| `action.shortenCountdown(seconds)` | 减少时间并恢复运行，保留标题；不足时归零 |
| `event.countdownElapsed()` | 正在运行的倒计时归零时满足；停止期间不满足 |

每张地图只有一个共享任务倒计时，单人和多人模式均可使用；不提供按玩家独立计时或自定义显示位置。
秒数为 0～143165576 的整数，按每秒 15 逻辑帧计算；循环出兵使用正数间隔。
如果结束动作没有重新设置时间，游戏会停止并隐藏到期的倒计时；到期本身不会宣告胜负。
重新设置时间会立即改变共享状态，应在同一条结束规则中安排增援和重设，
多条结束规则不保证都能观察到同一次归零。
更新编辑器后，点击“刷新类型”或“应用到地图”同步新的 `sdk.ts`。

## 给某个起点的玩家送小队

使用 `MissionMode.Multiplayer` 声明多人或遭遇战地图，再用
`house: m.playerAt(Waypoint.WP0)` 把小队交给开局占用 0 号起点的玩家，包含 AI。
多人模式省略 `player`，无需预先知道玩家国家、颜色或 Multi 编号。
下面示例要求地图已有 0 号起点以及 `HTNK` 单位类型：

```ts
import { defineMission, MissionMode, event, action, order } from './sdk';
import { map, Unit, Waypoint } from './map.generated';

export default defineMission(map, {
  id: 'start_gift',
  mode: MissionMode.Multiplayer,
}, m => {
  const start = m.waypoint(Waypoint.WP0);
  const gift = m.team('gift', {
    house: m.playerAt(Waypoint.WP0), // 归属：占用 0 号起点的实际玩家
    waypoint: start,
    units: [{ type: m.unit(Unit.HTNK), count: 3 }], // 三辆犀牛坦克
    orders: [order.guard(60)],
  });
  m.rule('give', {
    when: event.sinceScenarioStart(10),
    once: true,
    actions: [action.reinforce(gift, start)], // 出兵位置，与归属独立
  });
});
```

点击“刷新类型”或“应用到地图”会更新包含新 API 的 `sdk.ts`。
归属在游戏运行时按起点占用解析，随机起点也适用；空起点不会生成该小队，也不回退到中立方。
VERA 的 TeamType 归属支持 `<Player @ A>`～`<Player @ Z>` 和 `<Player @ AA>`～`<Player @ AD>`，
对应地图已有的 0～29 号起点。8～29 为扩展起点，需在地图设置中配置相应出生点数量；
仅添加普通任务航点不会使其成为玩家起点。30 以上的航点只能作为增援或移动位置，不能传给 `playerAt()`。
这里的参数是地图起点编号，不是房间座位编号。

外部代码编辑器的类型检查会提示单人模式调用 `playerAt()`、传入不存在或超出 0～29 的航点或多人模式指定固定 `player`；
Rust 还会校验伪造的引用。多人模式写入 `Basic/MultiplayerOnly=yes`，不覆盖原有 `Basic/Player`，
生成规则使用中立所属方执行计时和增援。`winPlayer()` / `losePlayer()` 仅支持单人模式，
目前没有按起点指定玩家胜负的动作。小队行为仍由原生脚本控制，赠送不会自动取消警戒等命令。

当前已用两个独立客户端的回环 UDP 联机运行两局同一 TS 任务，核对起点归属、每两秒增援、空起点不出兵和共享倒计时时序。两端前 300 帧校验及结束状态一致，四份实际录像经严格回放核对增援与计时状态；复跑见[开发文档](../../docs/development.md)。跨机器网络、WOL 传输和联机存档尚未验收。

## 当前 API

- 引用：`m.house`、`m.playerAt`（多人起点玩家）、`m.waypoint`、`m.unit`、`m.object`、`m.text`。
- 资源引用：`m.building`、`m.techno`（含建筑）、`m.country`、`m.sound`、`m.eva`、`m.theme`、`m.movie`、`m.animation`、`m.superWeapon`、`m.warhead`、`m.particleSystem`。刷新类型后使用对应枚举；声音/EVA/音乐、国家胜负、脚本媒体、类型存在事件等已可用于相应动作、命令和事件。资源引用本身不会生成游戏效果。
- 共享定义：`m.taskForce(id, { units, group? })`、`m.script(id, orders)`；`m.team()` 可以使用 `taskForce` / `script` 引用，分别替代内联 `units` / `orders`。
- 小队 AI 配置：`max`、`priority`、`veteranLevel`、`group`、`autocreate`、`full`、`reinforce`、`recruiter`，以及招募、威胁、运输、空降和目标策略字段；未指定时保留原 SDK 默认值。当前运行时不读取 TeamType 的 `TechLevel` / `Prebuild`，SDK 不提供这两个伪配置。
- `VeteranLevel` 枚举：`Rookie`、`Regular`、`Veteran`、`Elite`；`transportWaypoint` 接受地图现有航点引用。
- 前向引用：`m.teamRef`、`m.ruleRef`、`m.tagRef`、`m.taskForceRef`、`m.scriptRef`；构建时解析实际使用的引用，不需要调整声明顺序。`m.rule()` 返回规则引用。
- 定义：`m.team`、`m.flag(name, initial?)`、`m.rule`；`once: true` 执行一次，`once: false` 重复执行。
- House：`m.createHouse(id, { country, color?, credits?, techLevel?, iq?, playerControl?, allies?, basePlan? })` 新建地图 House；`m.configureHouse(m.house(...), patch)` 只更新显式字段。颜色使用生成的 `HouseColor` 枚举，`credits` 每单位为 100 游戏资金；`allies` 是单向关系。`basePlan` 按顺序声明建筑节点或 `BasePlanControl` 控制点，节点需给出地图格坐标；`percentBuilt` 为 0～100。未声明时保留地图原计划。
- AI 小队选择：`m.aiTrigger(id, options)` 声明主/副小队、Country、条件、类型比较、权重、多人/侧别、难度和初始启用状态。`condition` 使用 `AiTriggerCondition`，比较使用 `AiTriggerComparison`；`owner` 缺省代表 `<all>`，`side` 使用原生 0～3 编号，难度缺省为全开。引擎自动招募/出队的实际条件仍由地图工厂、House AI 和游戏状态共同决定。
- Basic 开局：`m.configureBasic(options)` 可设置名称、已有航点的 HomeCell、片头/简报/行动/计分影片、主题、Win/Lose 胜负影片与原生终局选项；未声明字段沿用地图值。HomeCell 写入数字航点编号，Win/Lose 使用 Movie 引用。任务目标/简报文本属于战役目录，不是这两个字段。
- 任务航点：`m.createWaypoint(30..701, x, y)` 新建普通航点并返回可立即引用的 `Waypoint`；不覆盖地图原有航点和 0～29 玩家起点。重建时会复用同一记录，外部移动会作为冲突报告。
- 起点对象归属：`m.assignObjectToPlayer(m.object(...), playerStart)` 通过编辑器托管的零秒一次性触发器，在开局普通帧中把已有对象交给该起点的实际占用玩家。加载时对象先属 Neutral，多个转交规则可能分帧执行；依赖新归属的剧情应留出启动时间。要求地图有对应起点，且对象不能同时有其他 Tag 绑定。
- 规则控制：`enabled`、`owner`（Country）、`difficulties`、`linked`、`transferOnCapture`；动作 `enableRule`、`disableRule`、`forceRule`。
- 状态：`m.globalFlag(0..49)`、`m.localFlag(0..99)`；全局事件 `globalSet` / `globalClear`，动作 `setGlobal` / `clearGlobal`；已有局部变量仍用 `flagSet` / `flagClear` 和 `setFlag` / `clearFlag`。
- 事件：`sinceScenarioStart`、`sinceEnabled`、`countdownElapsed`、`destroyed`、`flagSet`、`flagClear`、`tagNotification`、`bridgeDestroyed`、`nearWaypoint`、`technoExists`、`technoAbsent`；组合使用 `all`、`any`。
- 行为：`moveTo`、`attackWaypoint`、`patrol`、`guard`、`hunt`。
- 动作：`reinforce`、`recruit`、`message`、`setFlag`、`clearFlag`、`winPlayer`、`losePlayer`。
- 倒计时动作：`setCountdown`、`startCountdown`、`stopCountdown`、`extendCountdown`、`shortenCountdown`、`setCountdownTitle`。
- 媒体/地图动作：`playSound`、`playMusic`、`playEva`、`revealMap`、`dropZone`、`createCrate`（使用 `Crate` 枚举）。
- 小队动作：`deleteTeam`（解散并保留成员）、`reinforceDefault`、`chronoReinforce`、`flashTeam`（带符号逻辑帧）。
- 显式绑定：`m.tag`、`m.bindObject`、`m.bindCell`；小队模板可设置 `tag`。

对象引用使用生成目录中的 `Structures:0`、`Units:3` 等实际记录键。
目标已经有手工 Tag 或存在重复物理记录时拒绝绑定；多条规则可通过 `linked` 共用一条 Tag 链。

`event.tagNotification()` 对应原生事件8“任意事件 / 始终满足”。它也注册到常规 Logic 轮询，因此无对象或格子绑定时仍能触发；不能用它等待某个特定通知。与其他条件通过 `linked` 共用一次性 Tag 时，它可能先执行并消耗整条链。只关心进入、销毁或桥梁破坏时，直接使用对应事件；需跨时间锁存时使用下文的重复 Tag 写法。

TeamType 字段是原生 AI 和增援策略，不会改变小队脚本本身。例如 `full` 控制增援时预装运输成员及填满新建矿车的矿仓，`autocreate: true` 交给 AI 自动创建小队，`droppod` / `useTransportOrigin` / `transportWaypoint` 配置增援投放。`veteranLevel` 使用 `VeteranLevel.Rookie`、`Regular`、`Veteran`、`Elite`；只有增援路径会按该等级设置新成员。`TechLevel` 和 `Prebuild` 虽保留原生 INI 默认输出，但 VERA 当前 TeamType 安装器没有读取它们，因此不可配置为有效行为。

`full: true` 在创建成员时预装，载荷仍保留在 Team 成员列表中。运输类型须声明正数 `Passengers`：有这种容量的 Aircraft 时只考虑飞机，否则考虑载具；特遣部队含海军类型时还要求运输类型为海军。全部载荷集中到最后创建的合格运输成员，不会均分到多辆运输车，也不经过普通上车的容量/体积检查。创作时应按实际运输能力安排成员；需要多车分装可拆成多个小队。HTK、LCRF 和 SHAD 的预装及载客快照恢复已有普通帧验证，LCRF 样本使用其两栖能力在陆地入场。

原版 `PDPLANE` 没有声明 `Passengers` 容量，仅将它与步兵放在同一小队并设置 `full: true` 不会预装。下面的 `droppod` 使用专用投放路径。矿车的 `full` 是填矿仓：已验证原版 HARV 的 40 单位矿仓填入矿石类型槽 0，创建时不会直接增加 House 资金；其他正数 `Storage` 类型不一定受当前引擎表示支持。复跑命令见[开发文档](../../docs/development.md)。

在 `defineMission` 回调内，可以让普通增援使用运输机空投步兵：

```ts
const landing = m.createWaypoint(30, 40, 40);
const origin = m.createWaypoint(31, 20, 45);
const paratroopers = m.team('paratroopers', {
  house: m.house(House.Americans), waypoint: landing,
  units: [{ type: m.unit(Unit.E1), count: 4 }],
  orders: [order.guard(2), order.hunt()],
  droppod: true, full: false,
  useTransportOrigin: true, transportWaypoint: origin,
});
m.rule('airdrop', {
  when: event.sinceScenarioStart(10), once: true,
  actions: [action.reinforceDefault(paratroopers)],
});
```

投放目标使用小队默认航点；`action.reinforce(team, at)` 可覆盖它。`transportWaypoint` 指运输机出生格，省略 `useTransportOrigin` 则按所属 House 边缘选择。引擎创建 `PDPLANE` 携带同一批成员，运输机不加入 Team，投放后自行离场。默认边缘与指定运输航点、四名原始成员下降/落地/脚本推进以及途中快照恢复已有普通帧审计。读取快照会按现有引擎语义重置场景随机流，后续随机返航路线和个别帧可与连续运行不同；该审计只保证列明的生命周期及两次独立恢复的观察结果一致，不替代正常 GUI 存档验收。

## 规则启停与条件组合

随机间隔可以直接用 `event.randomTime(seconds)`。它每轮抽取 `floor(seconds / 2)` 到 `floor(seconds / 2) + seconds` 的整数游戏秒，包含两端；例如 `randomTime(30)` 是每轮等待 15～45 秒。固定每 30 秒继续用前面的 `sinceEnabled(30)`。在 `defineMission` 回调中，使用已导入的 `House` / `Unit` 枚举：

```ts
const patrol = m.team('random_patrol', {
  house: m.house(House.Americans), waypoint: m.createWaypoint(30, 40, 40),
  units: [{ type: m.unit(Unit.E1), count: 2 }],
  orders: [order.hunt()],
});
m.rule('random_wave', {
  when: event.randomTime(30), once: false,
  actions: [action.reinforceDefault(patrol)],
});
```

`action.disableRule()` 停用规则；之后 `action.enableRule()` 会重新抽取随机等待并从启用时开始计时。`randomTime(0)` 没有等待间隔，不适合持续刷兵。同一条规则的 `all()` 中不能同时使用 `randomTime()` 和 `sinceEnabled()`，两者共用原生计时器。公共快照会保留已抽取的计时；当前引擎加载时重置场景随机流，因此后续新抽样与连续运行可能不同。

亮度事件读取正在渐变的当前环境亮度。阈值 `100` 对应开局配置 `ambient: 1`（100%），与 `action.setAmbient(100)` 的单位相同；`ambientAtMost()` 和 `ambientAtLeast()` 都包含等值。例如在 `defineMission` 回调中，让亮度降低后显示消息：

```ts
m.configureLighting({ normal: { ambient: 1 } });
m.rule('darken', {
  when: event.sinceScenarioStart(20), once: true,
  actions: [action.setAmbientRate(0.01), action.setAmbientStep(0.05), action.setAmbient(60)],
});
m.rule('dark_enough', {
  when: event.ambientAtMost(80), once: true,
  actions: [action.message(m.defineText('night_notice', '天色暗下来了'))],
});
```

设置目标亮度不会立刻满足当前亮度条件；逻辑轮询先于本帧渐变，达到阈值后由后续轮询触发。多条一次性规则也不保证同一帧执行。随机计时和亮度渐变的普通帧/公共快照复跑见[开发文档](../../docs/development.md)。

## 创建 House

`createHouse()` 返回值可立即用于小队归属，也可在其他 House 的 `allies` 中引用；跨声明引用可用 `m.houseRef(id)`。现有 House 通过 `configureHouse()` 修改，未指定的字段保留地图值。单人入口的 `player` 在构建回调前确定，因此只能选择地图中已有的 House。

`setAttachedBuildingsOnline(false)` 只关闭有耗电或 `Powered` 的绑定建筑；普通发电厂保持在线。该动作以当前活动规则所在的 Tag 为范围，未绑定建筑不受影响。`createBuilding(type, at)` 使用规则所属 Country 选中的 House；若需要归属新建 House，应为它使用能唯一选中该 House 的 Country。

下面示例要求 `Country.Alliance` 尚未被地图中的其他 House 使用，以便原生小队归属能唯一选中新增 House；已有 House 的国家可在任务制作中查看。影片示例使用当前资源目录中的 `Movie.A01_F00e`，实际任务可从生成枚举中选择所需影片。

```ts
import { defineMission, event, action, order, BasePlanControl, AiTriggerCondition, AiTriggerComparison, Difficulty } from './sdk';
import { map, House, Country, HouseColor, Unit, Building, Movie, Waypoint } from './map.generated';

export default defineMission(map, { id: 'new_faction', player: House.Americans }, m => {
  const rally = m.createWaypoint(30, 30, 22);
  m.configureBasic({
    name: 'Border Conflict',
    homeCell: rally,
    win: m.movie(Movie.A01_F00e),
    endOfGame: true,
  });
  const raiders = m.createHouse('raiders', {
    country: m.country(Country.Alliance),
    color: HouseColor.DarkRed,
    credits: 500, // 原生单位；开局资金为 50,000
    iq: 3,
    techLevel: 6,
    allies: [m.house(House.Americans)],
    basePlan: {
      percentBuilt: 25,
      nodes: [
        { kind: 'building', building: m.building(Building.GAPOWR), x: 30, y: 22 },
        { kind: 'control', control: BasePlanControl.Hold, x: 30, y: 22 },
      ],
    },
  });
  m.team('raider_team', {
    house: raiders,
    waypoint: rally,
    units: [{ type: m.unit(Unit.HTNK), count: 2 }],
    orders: [order.hunt()],
  });
  m.aiTrigger('raider_ai', {
    primary: m.teamRef('raider_team'),
    owner: m.country(Country.Alliance),
    condition: AiTriggerCondition.EnemyCredits,
    amount: 5000,
    comparison: AiTriggerComparison.GreaterOrEqual,
    difficulties: [Difficulty.Normal, Difficulty.Hard],
    enabled: true,
  });
  m.rule('victory', {
    when: event.sinceScenarioStart(300),
    once: true,
    actions: [action.winPlayer()],
  });
});
```

House 的 `Allies` 是逐方读取的有向关系；互为盟友需要在双方 House 上分别声明。创建、配置及删除均进入任务归属清单，重建会检查字段冲突；删除仍被小队或外交列表引用的 House 会失败。

```ts
// Difficulty 从 sdk 导入。
const stage = m.ruleRef('stage');
const campaignDone = m.globalFlag(7); // 不设置初值、不重置战役状态
m.rule('activate', {
  when: event.sinceScenarioStart(5), once: true,
  actions: [action.enableRule(stage)],
});
m.rule('stage', {
  enabled: false, difficulties: [Difficulty.Easy, Difficulty.Normal, Difficulty.Hard],
  when: event.all(event.sinceEnabled(30), event.globalClear(campaignDone)),
  once: true, actions: [action.setGlobal(campaignDone), action.winPlayer()],
});
```

`enableRule()` 重置目标的独立计时，遵守难度，也不能复活已经被一次性 Tag 删除的实例。
`forceRule()` 同步执行已启用规则的动作，跳过条件，但不消费 `once`、不自动重置计时；
循环强制触发、超出 32 层或 4096 动作预算的链在构建时拒绝。

`all()` 直接编码原生多事件，并非对 JS 布尔值求 AND。重复 Tag（`once: false`）可锁存部分对象通知，
`once: true` 不保证跨时间锁存；变量条件检查当前状态。
重置遵循各原生事件语义。最多 32 个叶事件，只允许一个 `sinceEnabled()`，
同一规则不能按身份区分多个附着对象。`technoExists()` 按全地图存储记录计数，包含 limbo/零生命，
不按规则所属方筛选。`nearWaypoint()` 绑定移动对象，距离阈值固定为 5 格。

例如先选中过对象、稍后全局条件满足，只执行一次：使用重复规则保留选择通知，执行后禁用自身。
该状态已通过普通帧与无界面快照恢复验证。

```ts
m.rule('selection_then_ready', {
  when: event.all(event.selected(target), event.globalSet(ready)),
  once: false,
  actions: [
    action.setGlobal(done),
    action.disableRule(m.ruleRef('selection_then_ready')),
  ],
});
```

这里 `target` 是地图对象引用，`ready`、`done` 是 `m.globalFlag(...)` 的返回值。

中文消息与倒计时标题已通过实际游戏 GPU 截图验证。可显示的字形取决于游戏使用的
`GAME.FNT`；干净验收安装会保留原安装的松散字体，任务声明的同名字体优先。
试玩会合并 CSF 并检查引用，缺少标签或声明资源时会返回错误。

`any(a, b)` 支持顶层分支，通过托管辅助规则和一个局部标记汇合，
同帧多个分支只执行一次，动作最多延后一个普通逻辑帧。`once: false` 仅接受计时、变量、
资金/损失/电力/环境光和类型存在等可重复轮询条件，逐轮清理标记并重置分支计时。
对象通知分支只用于 `once: true`，仍受附着对象身份与绑定约束。启停动作同时控制分支；
不支持嵌入 `all()`、与 `linked`/显式 Tag/占领转移混用，构建时会说明原因。
辅助项占用正常规则/变量容量，重建和删除时一起清理。

`m.localFlag(n)` 只引用已有 `VariableNames`，保留手工初值；自动 `m.flag()` 会避让这些槽位。
`owner: m.country(Country.Russians)` 使用该国家的第一个运行时 House；与任意地图 House、多人起点玩家不同。

`transferOnCapture: true` 允许规则 Tag 随工程师占领从工程师对象迁移到被占领建筑；需要把 Tag 绑定在工程师对象上。此段从 `map.generated` 导入 `MapObject` 和 `Country`：

```ts
const engineer = m.object(MapObject.Infantry_0);
const entered = m.rule('after_capture', {
  when: event.entered(m.country(Country.Americans)),
  once: true,
  transferOnCapture: true,
  actions: [action.setGlobal(m.globalFlag(0))],
});
m.bindObject(engineer, m.tag('capture_tag', { rule: entered }));
```

真实战役帧验收已确认工程师占领后 Tag 留在建筑，后续工程师进入会触发该规则；详见计划表的 S06 验收记录。
若后续进入需要重复触发，规则设 `once: false`，并为 Tag 选择 `TagRepeat.Repeat`；普通帧验收还验证了重复进入会重置独立规则计时器。

## 共享 Tag 与绑定

```ts
// TagRepeat 从 sdk 导入；target 是 m.object(...) 返回的引用。
const tag = m.tag('objective', { rule: m.ruleRef('objective'), repeat: TagRepeat.Repeat });
m.bindObject(target, tag);
m.rule('objective', {
  when: event.destroyed(target), once: false, linked: m.ruleRef('followup'),
  actions: [action.setFlag(alarm)],
});
m.rule('followup', {
  when: event.destroyed(target), once: false,
  actions: [action.message(m.text(Text.MISSION_TEXT))],
});
```

显式 Tag 必须引用链首，链内 `once` 一致；每条链只创建一个 Tag，避免重复活动实例。
`m.bindCell(x, y, tag)` 根据地图格式编码 CellTag，可用于桥梁通知；已有手工 CellTag 不会被覆盖。
`TagRepeat.Once` 首次满足便执行并解绑整个共享 Tag，其他附着者之后不会再次触发它；
`Repeat` 保留共享 Tag，一个对象消失后，其他附着者仍能触发；
`AllAttachments` 逐次解绑，最后一个引用才执行动作。`Once`/`AllAttachments` 要求 `once: true`，
`Repeat` 要求 `once: false`。小队 `tag` 将 Tag 关联到创建的小队/成员。对象、格子和小队可以共享同一 Tag。

共享对象的三种重复方式已用真实对象销毁路径验证，并在两个对象先后移除之间保存、恢复快照。
恢复与连续运行的 240 帧对象存在状态、动作标记和计时标记一致；该验证不包含 GUI 存档加载。

对象受伤事件是通知。`firstDamagedBySource`、`halfHealthBySource`、`redHealthBySource` 要求伤害来源，友军伤害也算；`firstDamaged`、`halfHealth`、`redHealth` 接受无来源爆炸。黄血事件要求生命从不少于 `floor(Strength/2)` 降至该值以下；红血要求从严格高于 `Strength*ConditionRed` 降至严格低于该值。一击同时跨过两条线时，只发红血通知；恰好停在红血线后再继续受伤，不补发红血通知。

重复 Tag 会锁存这些通知，之后收到其他通知也可能再次执行。若同一对象的多个阶段各处理一次，可将规则 `linked` 在一起，使用 `once: false`，并分别在执行后禁用自身。以下 `target` 为已有地图对象引用：

```ts
const first = m.ruleRef('first_hit'), yellow = m.ruleRef('yellow');
m.rule('first_hit', {
  when: event.firstDamaged(target), once: false, linked: yellow,
  actions: [action.message(m.defineText('hit', '目标受到伤害')), action.disableRule(first)],
});
m.rule('yellow', {
  when: event.halfHealth(target), once: false,
  actions: [action.message(m.defineText('yellow', '目标进入黄血')), action.disableRule(yellow)],
});
```

这里不能把链内规则改成 `once: true`：首次满足就可能解绑整个 Tag，后续阶段将失去绑定。真实攻击、友军攻击、无来源爆炸、跨两线、等于红血线，以及首次受伤后的快照恢复均有普通帧验收；恢复后保留已禁用规则和后续通知顺序，但攻击时刻可能偏移，不宣称逐帧或正常 GUI 加载一致。

格子通知用 `m.bindCell()` 绑定。`crossedHorizontal()` 监听与绑定格同 `ry` 的整行，`crossedVertical()` 监听同 `rx` 的整列，均无需经过绑定格本身。这是地图格子坐标，显示在等角视图中未必是屏幕上的水平/垂直线。例如绑定在(60,50)的行，Americans单位经过(30,50)也会触发：

```ts
const line = m.rule('line', {
  when: event.crossedHorizontal(m.country(Country.Americans)), once: true,
  actions: [action.message(m.defineText('line', '部队已越过警戒线'))],
});
m.bindCell(60, 50, m.tag('line', { rule: line }));
```

同一个Tag同时含横/竖线事件时只初始化横线。`enteredOrOverflown()` 对地面到达格子即时检查，高空单位则每16个原生逻辑帧采样当前格；快速飞过两次采样之间的格子可能不触发，悬停在绑定格上会等到下一次检查。

间谍伪装事件也绑定在格子上，要求进入者为Agent。假设Americans间谍实际使用MakeupKit复制了Russians的E2，随后进入绑定格：

| 条件 | 判断身份 | 结果 |
| --- | --- | --- |
| `entered(m.country(Country.Americans))` | 实际所属方 | 满足 |
| `enteredOrOverflown(m.country(Country.Americans))` | 实际所属方 | 满足 |
| `spyDisguisedAsCountry(m.country(Country.Russians))` | 显示的伪装House | 满足 |
| `spyDisguisedAsInfantry(m.techno(Techno.E2))` | 显示的伪装步兵类型 | 满足 |

间谍自身类型和所属方不会因此变成E2/Russians。未更换身份的原版Americans间谍可按其构造时的E1/Americans伪装匹配；普通ENGINEER进入不会发布这两个伪装通知。

`event.discovered(object)` 监听对象发现通知。TS `revealAt()` 揭示目标格，或己方侦察兵正常接近，都能走到实际通知；对象的历史“已发现”标记不能替代新通知，有来源击杀路径也可能发布同编号。`countryDiscovered()` 读取该Country首个House的发现状态，通常在对象通知后的House轮询阶段满足，不保证同帧执行。地图中没有匹配House时，`countryDiscovered`、`countryUnitsDestroyed`、`countryBuildingsDestroyed`、`countryAllDestroyed`、`countryNavyDestroyed`、`countryLandDestroyed` 六个条件直接满足；选择规则目录中的国家并不自动创建House。

探照灯检测绑定灯所在的建筑，图上的建筑类型须已配置 `HasSpotlight=yes`。灯的初始模式来自地图建筑记录，不能仅凭类型带灯就假定正在扫描。例如在开局一秒后启用扫描并处理首次检测：

```ts
const lightBuilding = m.object(MapObject.Structures_0);
const startLight = m.rule('start_light', {
  when: event.sinceScenarioStart(1), once: false, linked: m.ruleRef('light_detected'),
  actions: [action.setSpotlightBehavior(1), action.enableRule(m.ruleRef('light_detected')),
    action.disableRule(m.ruleRef('start_light'))],
});
m.rule('light_detected', {
  when: event.spotlightDetected(lightBuilding), once: false, enabled: false,
  actions: [
    action.message(m.defineText('light_detected', '探照灯发现了目标')),
    action.disableRule(m.ruleRef('light_detected')),
  ],
});
m.bindObject(lightBuilding, m.tag('light', { rule: startLight, repeat: TagRepeat.Repeat }));
```

示例需从SDK导入 `TagRepeat`。`setSpotlightBehavior()` 操作当前Tag关联的建筑灯。模式1往返扫描，检测灯点附近地面列表中的非友军；它不是绑定被照到的单位，也不以地图视野或隐形检测作为这个事件的条件。模式0关闭，模式2绕建筑持续旋转；模式3在灯点附近选择最近地面非友军并跟踪，目标无效或超出移动半径后退回模式1。只有实际模式1发布检测事件。建筑须可运作；要用 `setAttachedBuildingsOnline(false)` 关闭受控测试建筑，须满足耗电或Powered准入，普通发电厂保持在线。默认构建不把Ares移动探照灯当成可用能力。上述为真实普通帧灯对象及检测验收，GPU光束画面另行验收。

木箱创建与拾取是两个阶段。可用 `Crate` 枚举放置奖励，再监听指定地图对象或全局拾取通知：

```ts
const crateAt = m.createWaypoint(30, 40, 50);
m.rule('place_crate', {
  when: event.sinceScenarioStart(1), once: true,
  actions: [action.createCrate(Crate.Money, crateAt)],
});
m.rule('collector', {
  when: event.pickedUpCrate(m.object(MapObject.Infantry_0)), once: true,
  actions: [action.message(m.defineText('collector', '指定单位拾取了木箱'))],
});
m.rule('any_collector', {
  when: event.anyCratePickedUp(), once: true,
  actions: [action.message(m.defineText('any_collector', '发生了木箱拾取'))],
});
```

示例须从 `./sdk` 导入 `Crate`，并使用生成目录中实际存在的地图对象。创建动作不会触发拾取事件；单位正常移动拾取后才会通知。实际覆盖物必须有 `CrateTrigger=yes`，设为 `no` 时仍有奖励，但两个通知都不发。单人 `Crate.Money` 还受 `[CrateRules]` 中图像对应奖励及 `SoloCrateMoney` 控制，不应假定金额等于多人随机钱箱；随机/多人奖励另有选择和资格规则。

对象事件在奖励前同步执行，全局事件在后续逻辑阶段分发。在对象回调中执行 `changeAttachedOwner(m.country(Country.Russians))` 会改变后续奖励读取的所属方，并可能中断原移动命令。当前引擎验收中，回调销毁载具使其立即退休，因而保留木箱、不发奖励或全局通知；ENGINEER 归零血后仍在死亡动画中且 Alive，继续领取奖励并发布全局事件。不能仅凭零生命推断奖励取消。单人拾取会清除覆盖物、保留运行时槽位；不要把这一点当成可重复领取。以上已用正常移动与拾取前 headless 快照恢复验证；不代表多人、全部死亡类型或 GUI 存档组合已验收。

## 共享成员和行为脚本

多个小队可引用同一份 TaskForce 和 Script；内联写法继续可用。同一个小队中，
`units` 与 `taskForce` 必须二选一，`orders` 与 `script` 也必须二选一。
以下代码放在 `defineMission()` 的构建回调内，要求地图有 `Neutral`、`E1` 和 0 号航点：

```ts
// Unit、Waypoint、House 从 map.generated 导入，order 从 sdk 导入。
const members = m.taskForceRef('members');
const behavior = m.scriptRef('behavior');

for (const id of ['first', 'second']) {
  m.team(id, {
    house: m.house(House.Neutral),
    waypoint: m.waypoint(Waypoint.WP0),
    taskForce: members,
    script: behavior,
  });
}

m.taskForce('members', { units: [{ type: m.unit(Unit.E1), count: 3 }] });
m.script('behavior', [order.guard(5), order.hunt()]);
```

共享定义分别生成一项原生注册记录；小队实例的执行进度仍由游戏各自维护。
新 SDK 输出 v2 构建数据，Rust 继续接受旧 v1 数据；刷新类型会升级已有的编辑器生成 SDK，保留作者文件。
可用 m.aliasObject/m.aliasWaypoint 保存逻辑别名；对象记录指纹变化时拒绝重建，需重新选择。外部工具重排不会自动猜测 UUID 身份。

`m.text` 引用已加载的 CSF 标签；`m.defineText` 直接声明中文，`m.asset` 声明媒体依赖，试玩和战役发布由宿主统一输出。
`action.talkBubble(team, 1)` 可在该类型首个活动小队的首成员处显示气泡，编号为1～3。
行为脚本可用 `order.talkBubble(1)` 在执行脚本的当前小队首成员处显示气泡；`order.talkBubble(0)` 清除，脚本不等待气泡消失。
全局同时只有一个气泡；`talkBubble(team, 0)` 或 `clearTalkBubble()` 清除它。
没有该类型的活动小队时保留原气泡，定义了小队模板不等于已招募或增援出活动小队。
气泡需要安装资源中的 `talkbubl.shp` 和 `palette.pal`，也可用 `m.asset()` 声明相应图形。
`TalkBubbleTime` 由墙钟计时，不能用逻辑帧数判断过期；它也不承载自定义文字。
`action.chronoScreen(15)` 播放两段各15处理步的超时空过渡，中间45步白屏；0为不启动。
场景暂停期间宿主会继续处理后段并自动恢复，原先已锁定的输入仍保持锁定，需作者按剧情解锁。
JS 函数、循环和模块在构建时执行；没有游戏逐帧回调或 `await sleep()`。
游戏时间按每秒 15 逻辑帧解释。脚本没有文件、网络、进程或宿主对象访问权限；
循环和递归有执行上限，但这不是面向恶意代码的进程级内存隔离。

局部标记自动避让已知变量和引用；存在未知扩展操作码时拒绝自动分配，避免覆盖 Mod 私有变量。
生成记录被手工改动时报告冲突；重新构建只维护当前脚本的记录，不覆盖其他任务。
目前每张地图管理一个脚本产物集合；源码可以通过相对导入拆成多个 `.ts` 模块。
使用 `./lib/waves` 这样的无扩展名路径；仅项目虚拟文件可导入，不支持 npm 包或 DOM/Node API。

开发时可以运行以下命令测量初始化、首次构建、重复应用、修改后构建及语法错误的耗时。
可选参数为当前地图的 `map.generated.ts` 路径；统计的是编译和脚本执行部分，不包含地图应用或资源加载：

```powershell
cargo run --locked --offline -p ra2-map-mission-script --example check -- path/to/map.generated.ts
```

## 无窗口命令

```powershell
ra2-map-editor mission-init --game-dir D:/Games/YR --map mission.map --script mission.ts
ra2-map-editor mission-build --game-dir D:/Games/YR --map mission.map --script mission.ts --output compiled.map
```

`mission-init` 不覆盖已有源码。`mission-build` 检查并生成辅助类型文件，输出完整地图，
默认拒绝覆盖输出，显式 `--overwrite` 可替换已有输出；始终拒绝覆盖输入地图。

源码与地图分别保存。发布和原有试玩入口使用已经应用到地图的任务结果；
修改源文件后必须重新点击“应用到地图”。当前版本尚未自动追踪已关闭脚本的外部文件改动；
源码由外部编辑器保存和恢复。切换地图后需重新选择对应脚本，或填写脚本路径。

## 引擎补充能力

`explosionAt` 接收武器引用，读取该武器的 `Damage` 和 `Warhead`，在航点直接造成范围伤害，
并由弹头选择爆炸动画。刷新类型后使用 `Weapon` 枚举；旧版传 `Animation` 的写法需要修改。

```ts
import { action } from './sdk';
import { Weapon, Animation } from './map.generated';

// 在 defineMission 回调中，at 为已定义的航点引用：
action.explosionAt(m.weapon(Weapon.Vulcan), at);
action.playAnimationAt(m.animation(Animation.EXPLOSML), at);
```

两个动作分别选择武器和动画，不能互换。`explosionAt` 不创建飞行弹体；
`playAnimationAt` 的后续伤害由动画自身的 Rules/ART 决定。

环境打击使用已定义航点：

```ts
action.ionBlastAt(at);       // 首次离子冲击更新造成伤害，普通帧中可与动作同帧发生
action.nukeAt(at);           // NukePayload从上空落下，飞行/核爆动画后造成伤害
action.chemicalStrikeAt(at); // 已注册的ChemLauncher从地图边缘飞向目标
action.meteorShower(at, 0);  // 强度仅0～3档；需要METSMALL/METLARGE及对应ART/SHP
action.reduceTiberiumAt(at); // 连续减少航点中心矿石，不会清掉周围整片矿场
```

原版资源未必注册 `ChemLauncher` 或提供陨石图像；这些前置需通过活动规则/美术资源配置补齐后再刷新类型。
`setTiberiumGrowth(false/true)` 控制矿石生长/扩散驱动；加载时禁用可能使初始增长队列为空，重新启用不会凭空补种矿场。
`setVeinGrowth`、`setIceGrowth` 保留场景标志，不能据此认为引擎已经实现藤蔓/冰层生长。
`combatFlash` 只请求C4冲击光照，原版弹头是否发亮由规则决定；它不会直接伤害单位。

固定引擎当前已提供具名接口的编号为事件 **58**、动作 **138**、小队命令 **54**。
能力矩阵在 `capabilities::matrix()`，包括 308 个编号、空洞、模式与特性门控。默认可用事件 58、动作 138、小队命令 54 已暴露；事件 2/3 有条件处理器但缺少生产通知入口，未提供默认 API。Ares 事件、动作和命令保留独立特性标记，由 `enabled` 表示当前构建是否启用。接口覆盖与呈现验收分别记录在扩展计划中。

| 类别 | 接口示例与语义 |
| --- | --- |
| 状态事件 | `randomTime` 每轮由引擎抽样；`creditsAtLeast/AtMost` 使用原始资金；损失计数为历史数量；`ambientAtMost/AtLeast` 使用原始环境光整数 |
| 对象事件 | 发现、攻击、选择、探照灯、首次/半血/红血、销毁/移除和木箱；自动绑定对象。`selected`、`damagedByHouse` 仅单人，后者是实际 House 数组索引 |
| 类型事件 | 建筑/车辆/步兵/飞机建造、建筑存在、间谍伪装；Rust 按真实分类数组解析序号，不能互换类别 |
| 镜头与 HUD | `centerCamera/jumpCamera/homeCamera`、`visibleMapArea`、`radarEvent`、`sidebarTab`、`flashCameo`；镜头速度为 0～4 档，显示范围须在 Map/Size 内 |
| 可见性 | `revealAt/revealZone/concealAt/growShroud/reshroudMap`，保留原生揭示与遮蔽作用范围 |
| 媒体和输入 | 影片、雷达影片、指定/随机航点声音和停止、`lockInput/unlockInput`、`pauseScene`、`chronoScreen`、对话提示 |
| 对象与 House | 附着对象销毁/唤醒/在线离线/卖出、驻军释放、创建建筑、House 对象子集、恢复初始对象、转移所有权 |
| 超武 | 授予、充能比例、计时覆盖与重置、目标/防御位置、铁幕/超时空/闪电风暴；仍遵守游戏目标和发射准入 |
| 环境与 AI | 环境光/颜色、爆炸/动画/粒子/桥梁、矿石与环境开关、生产比例、AI 策略、外交和结束流程 |
| 小队命令 | 友军跟随、建筑选择、集结、铁幕/超时空和功能建筑进入；`idleAnimation(0/1)` 是 Idle1/Idle2；`scout` 仅单人 |

大多数时间使用游戏秒（15 逻辑帧/秒），显式 frames 使用逻辑帧。`pauseScene(n)` 仅单人，单位是 60 个 16ms 桶（约 0.96 秒），必须大于 0；`chronoScreen(n)` 使用两段各 n 服务帧与中间 45 帧，不是现实秒数。`setAmbientStep/Rate` 接受有限非负浮点数，宿主自动编码 binary32 位。

`action.centerCamera(at, velocity)` 和 `order.centerCamera(velocity)` 的速度类型是 `CameraVelocity`（`0 | 1 | 2 | 3 | 4`，默认 1），数值越大移动越快，不是秒数。即使绕过 TS 类型检查，宿主也会拒绝无效档位。`jumpCamera` 立即跳转，`homeCamera` 返回本地基地；镜头仍受地图边界限制。`radarOutage(frames)` 的计时结束后，雷达还要完成开启动画才重新显示小地图。

`action.visibleMapArea(left, top, width, height)` 修改运行时的 `Map/LocalSize`；四个参数是地图矩形字段，不是屏幕像素或菱形格的两个角点。例如在64×64地图中：

```ts
m.rule('shrink_area', {
  when: event.sinceScenarioStart(10), once: true,
  actions: [action.visibleMapArea(20, 20, 24, 24)],
});
m.rule('restore_area', {
  when: event.sinceScenarioStart(14), once: true,
  actions: [action.visibleMapArea(2, 4, 60, 54)],
});
```

恢复参数应使用自己地图原来的范围；引擎仍会按地图边距归一化。范围外的既有对象不会因此被杀死，范围标记和地形通行区会重新计算；既有建筑的移动阻挡仍保留。相同参数的重复动作也会产生一次修订，不会被SDK去重。雷达完成开启动画后，小地图绘制和点击坐标转换会采用新范围；验证夹具的同一位置点击由完整范围的格[62,63]变为缩小后的[64,64]，恢复后回到[62,63]。平地、重复写入和公共快照恢复已验证，高程/桥梁等其他几何组合另行验收。

`grantSuperWeapon(weapon)` 首次授予单次使用且立即就绪的超武，发射后消耗；首次传入 `false` 则授予可重复使用的超武。如果该实例已经活动，重新授予会保留原有的单次标记，不能用 `false` 将活动中的一次性超武转换成永久超武。`setSuperCharge(weapon, 100)` 走原生百分比计时，浮点截断可能留下1帧，须等下一次超武更新才就绪；`setSuperRecharge`/`resetSuperRecharge` 改变后续充能时长，不重启当前倒数，`resetSuperWeapon` 才重置当前充能。

`lightningAt(at)` 创建一次云层打击，不启动全局风暴。`lightningStormAt(at)` 使用规则所属 House、指定航点和 Rules 中的延迟/持续时间；`startLightningStorm(frames)` 则没有 House、由引擎选择位置，仍先等待 `LightningDeferment`，参数是开始后的持续逻辑帧。`stopLightningStorm()` 只请求活动风暴结束，等待已有云层退场；尚在倒数的风暴不会因此取消。

`controlHouseBuildings(target)` 由规则所属 House 接管目标 House 的建筑；传入 `true` 归还此前接管的建筑。`changeAllHouseObjectsOwner(target)` 则把规则所属 House 的建筑、步兵、车辆和飞机整体转交目标 House。这两种操作都不改变规则本身的所属方。

`restoreInitialObjects(target, true)` 修复或补建开局记录中的建筑；省略第二参数时，尝试重建全部四类开局对象。恢复遵循原生放置条件；全部重建不会自动跳过仍存在的对象，不能用作保留当前队伍的补员操作。建筑接管/归还、四类整体转移和摧毁后的两种恢复路径已有真实资源普通帧验证。

`order.guard(n)` 按每秒 15 个逻辑帧计时，完成后推进；`order.waitForever()` 和 `order.mission(...)` 持续停在当前行，不会自行执行后续命令。需要等待一段时间后继续时使用 `guard`。`panic/calm` 调用成员类别自己的回调：适用步兵会增减恐惧值，无畏类型可拒绝；`calm` 不直接重置卧倒姿态或动作时钟。

`order.changeCountry(country)` 改变成员归属，Team 本身仍归原 House；不能靠它切换后续命令的 House 上下文。普通帧已验证两名成员全部转国，原 Team 的下一行仍执行，脚本结束后 Team 清理且成员保留。`deploy()` 则遵循成员类型能力，例如 AMCV 成功展开后会被 GACNST 替代。

`enterGrinder/enterTankBunker/enterBioReactor/enterBattleBunker/enterNearestOccupiable` 找到可用目标时就让相应成员离队并自行前往；命令不等待实际进入，搜索失败也会推进。`enterBattleBunker()` 搜索己方允许该步兵驻守的建筑，`enterNearestOccupiable()` 在全体建筑中按原生准入寻找最近目标。

`action.releaseGarrison()` 只释放当前 Tag 首个匹配建筑的驻军，即使它已空，也不会继续处理第二个建筑。需要分别控制多座建筑时，应使用独立 Tag。真实资源普通帧已验证两种驻军搜索、成员实际进入和释放，以及共享 Tag 下第二座建筑保留驻军。

特殊建筑搜索也都只负责找到目标并移交给成员自己的行为：`enterGrinder()` 成功后 Grinder 会消耗进入者并向建筑所属 House 退款；`enterTankBunker()` 要求成员满足 Bunkerable、武装、非 Hover 等原生条件，成功安装后单位仍留在地图上并与碉堡互相关联；`enterBioReactor()` 的适用步兵会进入建筑 Cargo/Limbo。真实资源普通帧已分别验证 E1/YTNK/INIT 对应的实际目标结果。

## 中文文本、媒体和持久构建记录

```ts
const title = m.defineText('wave_title', '下一波增援');
const intro = m.defineText('intro', '守住阵地，等待援军。');
m.rule('intro', { when: event.sinceEnabled(1), once: true,
  actions: [action.message(intro), action.setCountdown(30), action.setCountdownTitle(title)] });
// source 相对任务入口目录；JS 只声明，文件由宿主读取。
m.asset('soundmd.ini', 'media/soundmd.ini');
m.asset('audio.idx', 'media/audio.idx');
m.asset('audio.bag', 'media/audio.bag');
const alarm = m.assetSound('MissionAlarm'); // 必须在声明的 SoundList 中存在
```

文本标签由任务 ID 和文本逻辑 ID 稳定生成。中文、换行、Emoji 保存在地图元数据中，试玩/发布时合并到 CSF。撤销地图应用也撤销声明；构建阶段不写媒体文件。

影片字幕可通过 `m.asset('subtitlemd.txt', 'media/subtitlemd.txt')` 发布；文件使用带 BOM 的 UTF-16LE。仅此字幕文件名获准使用 `.txt`，它会替换发布目录的整份字幕表，因此应保留仍需使用的其他影片条目。例如下面为 `A01_P01e` 定义在0.2秒出现、持续2秒的字幕：

```text
BeginMovie A01_P01e
<Time=00:00:00:20, Duration=00:00:02:00, Text="收到新的任务指令。">
EndMovie
```

`Time` 和 `Duration` 的四段分别为时、分、秒、百分之一秒。`action.playRadarMovie(m.movie(Movie.A01_P01e), true)` 走战役媒体路径：通讯前导和雷达开启阶段仍推进逻辑，实际播放期间才暂停逻辑与倒计时，影片结束由宿主自动恢复。先前的 `lockInput()` 保留，仍需任务显式 `unlockInput()`；默认 `false` 在播放时继续推进逻辑。

资源源路径不得绝对定位或越过入口目录，符号链接解析后也须在目录内；输出为单层文件名，不得重名或覆盖生成文件。每项最多 256 MiB，总计最多 512 MiB。支持独立媒体/游戏数据文件；媒体 INI 仅接受 soundmd.ini、evamd.ini、thememd.ini，使用 `assetSound/assetEva/assetTheme` 引用其中项目。它们会替换相应活动目录，需保留任务还使用的基础条目。规则和 ART 修改应作为活动资源配置加载，以保证类型序号一致。

Sound 列表的样本选择依赖 `audio.idx` / `audio.bag` 索引。自定义声音须提供匹配的索引/样本包，或引用基础资源中已有的样本；仅声明独立 WAV 不会让声音列表识别它。EVA 按 `evamd.ini` 的 Allied/Russian/Yuri 字段读取物理样本名，先查 BAG，再查同名资源和 `.wav`，因此可声明独立 WAV；样本名须符合引擎的8字符读取边界。音乐也可使用独立 WAV，配套声明 `thememd.ini` 并用 `assetTheme()` 引用。

自定义全局声音和指定航点声音已通过真实客户端的播放队列验证，设备实际拉取的输出包含非零 PCM。可用开发文档中的 `mission_sdk_presentation_fixture` / `profile-audio.json` 重现。`--audio-streams` 变体及 `tools/verify_mission_audio.py` 还通过 Windows WASAPI 回环检测到440 Hz声音、660 Hz音乐和880 Hz EVA 的持续非零输出，无需新增引擎诊断接口。另用 `--campaign-media` 和 shell 战役入口验证了开场影片首帧及跳过、雷达影片实际 GPU 内容、键盘锁定/解锁。雷达影片的暂停/不暂停播放另由普通战役窗口比较了实际画面与独立解码帧、四段字幕和WASAPI音轨时频指纹，不播放对照也通过；严格PCM波形相关度未通过，不宣称逐样本保真。复跑见[开发文档](../../docs/development.md)。原版曲目/语音保真和扬声器听感另行验收。战役媒体有单人模式门控，不能用遭遇战截图替代。

`action.dropZone()` 使用活动规则的 `General.DropZoneAnim`，地图动作加载器会绑定该动画资源；真实资源普通帧已验证信标创建。自定义规则需提供对应动画及 SHP。`action.damageBridgeAt()` 会在航点周围进行 C4 范围伤害，再尝试破坏桥梁，也可能伤及附近对象。

应用结果记录入口路径、模块/媒体内容哈希、SDK 哈希与资源配置身份。任务窗口后台提示过期；试玩和发布在 IO 工作线程检查，发现外部修改、依赖缺失或切换资源时要求重建。旧地图没有持久记录时仍可读取；声明媒体却没有构建记录时拒绝发布。地图移动到其他机器后需要重新选择本地源码并构建。

## 两关战役入口

```ts
import { defineCampaign } from './sdk';
export default defineCampaign({ id: 'MY_CAMPAIGN', entry: 'first.map', missions: [
  { file: 'first.map', source: 'maps/first.map', script: 'first/mission.ts',
    title: '第一关：登陆', briefing: '占领基地。', next: 'second.map' },
  { file: 'second.map', source: 'maps/second.map', script: 'second/mission.ts',
    title: '第二关：决战', endOfGame: true },
] });
```

地图/脚本/战役资源路径相对 campaign.ts；关内 `m.asset` 路径相对该关入口。每关源码可导入共享模块，跨关状态使用 `m.globalFlag(0..49)`，引用不会重置值。
生成地图保存战役根目录、入口及所属关卡。重开地图后，通过任务窗口或 `mission-build` 重建该关时沿用这些记录，保留共享导入与媒体路径，并重新读取战役声明中的跳转；更换/移除关卡脚本关联时要求重新构建整个战役。
`next` 是全局 0 清除的直接目标，`alternate` 是全局 0 设置的目标，默认同 next。原生 Win/Lose 字段是影片，任务简报由战役元数据生成。全部关卡、CSF 与资源验证通过后才原子发布，不生成中间地图文件。已有项目 JSON 入口继续可用。

GUI：“战役目录与任务发布”→“构建并发布 TS 战役…”。CLI 使用同一路径：

```powershell
ra2-map-editor campaign-build --game-dir D:/Games/YR --script campaign.ts --output new-package
```

输出目录必须尚不存在。可复制 [两关声明](two-stage/campaign.ts)、[第一关](two-stage/first.ts)、[第二关](two-stage/second.ts)，并提供对应地图后刷新类型。

## 原生定义、别名和来源定位

```ts
m.source('waves.ts', () => {
  const members = m.nativeTaskForce('MANUAL_FORCE');
  const behavior = m.nativeScript('MANUAL_SCRIPT');
  m.team('reuse', { house: m.house(House.Russians), waypoint: m.waypoint(Waypoint.WP20),
    taskForce: members, script: behavior });
});
const target = m.aliasObject('radar', m.object(MapObject.Structures_0));
const entry = m.aliasWaypoint('landing', m.waypoint(Waypoint.WP20));
```

`nativeTeam/nativeScript/nativeTaskForce/nativeRule/nativeTag` 只引用当前地图/固定定义，不接管或复制。原生规则可启停或删除；其递归预算无法可靠分析，`forceRule(nativeRule(...))` 会报错。新规则的关联链仍只接受本任务规则。
对象别名对同一个记录键保存字段指纹；坐标、类型、所属方或记录重排变化时要求重新选择并更换别名 ID。生成项的逻辑 ID、原生 ID、显式来源模块保存进地图，可由 `native::source_mappings()` 读取；不伪造行号。

## 小队按建筑类型选目标

```ts
// 放入 m.team(...).orders；Building 来自当前地图生成的类型目录。
order.moveToOwnBuilding(m.building(Building.GAPOWR), BuildingSelection.LeastThreat);
order.moveToEnemyBuilding(m.building(Building.GAPOWR), BuildingSelection.Farthest);
order.attackEnemyBuilding(m.building(Building.GAPOWR), BuildingSelection.Nearest);
order.attackWaypointBuilding(targetWaypoint);
```

从 `./sdk` 导入 `order, BuildingSelection`，从 `./map.generated` 导入 `Building`。四种模式为 `LeastThreat`、`MostThreat`、`Nearest` 和 `Farthest`，默认最近。威胁模式读取成员所属 House 在建筑中心格保留的区域威胁，附近敌军会影响评分，并非只比较建筑类型的 `ThreatPosed`；距离模式比较对象的原生 Location，相同评分保留先遍历的候选。

己方移动按领队所属 House 搜索，敌方移动/攻击按首个成员所属 House 搜索；`onlyTargetHouseEnemy` 还会限制为该 House 当前指定敌人的建筑。原生敌方搜索也允许非盟友和盟友被动国家，不能把它当成排除所有盟友的通用筛选。两种移动命令前往建筑附近、成员当前 zone 内的可通行格，不要求站进建筑占地；无匹配建筑或无落脚格就进入下一行，不会等待未来建造。

攻击命令保留建筑对象作为目标，实际开火仍受单位武器和弹药等条件控制；给工程师这个命令不会自动变为占领。`attackWaypointBuilding` 只选择航点格阻挡的建筑，空地或其他对象会使脚本继续。

## 基地集结与跟随友军

`order.gatherAtEnemyBase()` 在领队所属 House 的指定敌人基地外、朝己方基地的方向集结，距离来自规则 `AISafeDistance`；己方没有基地时改用领队位置确定方向。没有指定敌人或敌方没有基地时，直接进入下一行。`order.regroupAtBase()` 则在己方基地外、朝指定敌人基地的方向集合；无指定敌人时选择随机方向。两者搜索可通行的 3×3 区域。

基地位置可通过正常 TS 动作声明，例如以下规则为 Americans 设置备用基地；已有基地时可沿用引擎当前记录。指定敌人由 House 的外交/受击等原生流程产生，并非地图上任意一个非盟友。

```ts
m.rule('home', {
  owner: m.country(Country.Americans),
  when: event.sinceScenarioStart(0), once: true,
  actions: [action.setAlternateBase(homeWaypoint)],
});
// 已经互不结盟时，此动作也会为规则所属 House 增加对目标的敌意并重新选择敌人。
m.rule('enemy', {
  owner: m.country(Country.Americans),
  when: event.sinceScenarioStart(1), once: true,
  actions: [action.makeEnemy(m.country(Country.Russians))],
});
// 放入所需小队的 orders。
order.gatherAtEnemyBase();
order.regroupAtBase();
order.followFriendlies();
```

`regroupAtBase` 需要有效己方基地：在无己方基地的验收任务中，原生查找最后留下 `(0,0)` 目标，成员原地等待，后续行没有执行。不要把它当成无基地时自动返回开局位置的命令。

`followFriendlies` 每帧搜索队外最近的存活 Unit 或 Infantry，不选择建筑或 Aircraft；先扫描载具、再扫描步兵，较近的步兵仍会取代较远载具。友军判断有方向：候选对象的 House 必须把小队所属 House 视为盟友，只设置反向 Allies 不够。跟随移动中的对象会更新目的地，抵达后可以进入下一行；无候选时会等待。因此它不等于永久护送或永久等待命令。路径与攻击目标镜像仍由原生运动和作战逻辑处理，本文验收不包含隧道或攻击镜像组合。

## 小队超时空

```ts
// Building、SuperWeapon 来自 ./map.generated；order、BuildingSelection、Quarry 来自 ./sdk。
const sphere = m.superWeapon(SuperWeapon.ChronoSphereSpecial);
m.rule('grant_chrono', {
  owner: m.country(Country.Americans),
  when: event.sinceScenarioStart(1), once: true,
  actions: [action.grantSuperWeapon(sphere, false), action.setSuperCharge(sphere, 100)],
});
// 在稍后执行的小队 orders 中选择一种目标方式。
order.chronoshiftToBuilding(m.building(Building.NAPOWR), BuildingSelection.Nearest);
order.chronoshiftToQuarry(Quarry.Power);
```

这两条命令使用领队当前所属 House 的超时空，源位置是小队中心，目的地是选中目标的中心格；建筑是落点参考，不是被传送的对象。建筑版本复用建筑类型/选择模式，类别版本使用领队的普通 Quarry 威胁搜索。后者在实测的无武器MTNK/Power组合中也会选中目标，不能仅凭武器为空判断失败。

命令本身不授予超武。没有超武、没有匹配目标或充能离就绪还远时，会进入下一行；达到规则 `AIMinorSuperReadyPercent` 的临近就绪条件时等待。已就绪但缺电也可能持续等待，恢复电力后才发射。`setSuperCharge(..., 100)` 可能保留一帧充能，应让普通帧更新完成后再使用。

两次发射在同一脚本访问内完成，随后脚本推进，不等待成员完成传送。需要等到落地再作战时，应在后面安排适当等待或后续移动。超时空作用于中心周围的普通区域，也可能包含队外对象；有机步兵死亡、落点占地回避及传送延迟仍遵循原生超武效果。正常 SuperWeaponTypes 的类型/序号关系必须保持，不能随意重排 ChronoSphere/ChronoWarp。

## 小队声音、音乐与影片

在小队的 `orders` 中可以直接使用媒体命令：

```ts
orders: [
  order.playSound(m.assetSound('SdkTone')),
  order.guard(4),
  order.playMusic(m.assetTheme('SdkMusic')),
  order.guard(4),
  order.playMovie(m.movie(Movie.A01_P01e)),
  order.setGlobal(m.globalFlag(0)),
  order.waitForever(),
]
```

`Movie` 来自 `./map.generated`。此例的自定义声音、音乐需先用 `m.asset()` 声明并发布相应的 `soundmd.ini`、`audio.idx/audio.bag`、`thememd.ini` 和音频文件；已有资源也可使用生成枚举的 `m.sound()` / `m.theme()`。

声音按居中方式播放，不使用小队位置；音乐切换全局背景曲目。三条命令都会推进脚本，声音和音乐不等待播放结束，需要间隔可加 `guard()`。全屏影片由战役客户端暂停与恢复游戏；遭遇战/多人或缺少影片文件时跳过播放。不要把影片命令当作跨模式通用等待器。

编辑器侧普通帧审计核对三种请求的实际注册序号、唯一性和后续行；Windows回环验证了小队声音和音乐的实际输出。普通战役客户端另验证了三秒影片自然播放、字幕A/B/C/D及返回任务后的消息，无影片对照也通过。影片音轨的两段时频指纹与独立FFmpeg解码匹配，并定位在影片可见期间，next对照没有匹配信号；严格PCM波形比较未通过，不宣称逐样本一致或原版保真。现有隐藏窗口截图入口会自动发送Escape，只用于首帧和跳过后的继续执行；自然播放使用普通窗口核验工具。复跑见[开发文档](../../docs/development.md)。

## 小队动画与紧急出售

```ts
// 将所需命令放入小队 orders；order 来自 ./sdk，Animation 来自 ./map.generated。
order.idleAnimation(0); // 请求步兵 Idle1；1 为 Idle2。
order.playAnimation(m.animation(Animation.CHRONOSK), 3);
order.fireSale(); // 设置小队所属 House 的紧急策略。
// 规则动作可显式选择国家；Country 来自 ./map.generated。
action.fireSale(m.country(Country.Russians));
```

`idleAnimation` 请求步兵执行空闲动作，当前动作和类型决定是否接受。载具/飞机不播放此步兵动作，脚本仍进入下一行；步兵也不会等动作播放结束才推进。移动等正常行为可以随后改变动作，需要显示完整空闲动画时应另安排等待。

`playAnimation` 首次执行时为每名成员创建一个附着动画，跟随该对象的坐标；脚本立即进入下一行，不等待动画结束。`loops` 是打包的16位构造参数，实际循环还取决于 ART 中的 `LoopCount`：原生先把大于1的构造参数截成字节，再与 ART 次数按字节相乘，结果至少1。因此0和1同处理，0不表示无限，256也不等于播放256次。原生字节值255的持续循环等特殊行为仍由动画生命周期决定。

真实资源对照中，`EXPLOSML` 的 ART `LoopCount=0`，构造参数0/1/3/256都只播放一次；`CHRONOSK` 的 ART `LoopCount=1`，参数3确实延长播放，参数256则与1相同。播放时长还受 `LoopStart`、`LoopEnd`、`End` 和速率影响，不能把构造参数当成整张SHP的重复次数。

两种 `fireSale` 都只设置目标 House 的紧急状态4，不重置策略时钟，也不在调用当帧强制出售。AI 在正常策略更新到期后尝试出售建筑，并让成员离开原Team进入Hunt；原Team可能随后清理，后续脚本行未必执行。真人控制的House跳过这条策略更新，不能用它直接出售玩家基地。出售还需要建筑正常加载的建造动画等原生前置；调用返回不等于已经完成退款或移除。

## 侦察与小队成功标记

```ts
// 在 defineMission 回调内；House、Unit 来自 ./map.generated，order 来自 ./sdk。
const scouts = m.team('scouts', {
  house: m.house(House.Americans),
  waypoint: start,
  units: [{ type: m.unit(Unit.ENGINEER), count: 1 }],
  orders: [order.scout(), order.succeed()],
  full: true, max: 1, reinforce: false,
});
m.rule('dispatch_scouts', {
  when: event.sinceScenarioStart(1), once: true,
  actions: [action.recruit(scouts)],
});
```

`start` 是已声明的起点航点，地图需有可招募的对应单位。`order.scout()` 仅支持单人：它按照**当前玩家**的迷雾在小队中心附近选点，不按照小队所属 House 的迷雾选点。首次选定后保持目标，再经普通移动完成；外国小队可能选到自己已经看见的格子。如果当前玩家已揭示整图、附近无候选，则返回原中心。选点本身不保证任意地形都能抵达，通行仍由正常运动处理。

`order.succeed()` 锁存小队成功并进入下一行，不直接使玩家胜利，也不立即解散小队。脚本走到末尾或 `action.deleteTeam(scouts)` 解散时，才向所有以该 TeamType 为 `primary` 的 AITrigger 记录一次尝试/成功；仅作为 `secondary` 引用不会记入，关闭的 AITrigger 也会接收此结果。重复 `succeed()` 仍只在小队销毁时计一次。

需要保留小队时，可在成功后添加 `order.waitForever()`；等待期间尚未结算结果。未调用成功标记就走到末尾，则记录一次未成功尝试。正常结束/解散保留原单位并清除小队导航，玩家结果用 `order.winPlayer()` / `order.losePlayer()` 单独控制。

## 运输脚本的满载条件

`order.load()` 为小队成员下达登车命令；`order.waitLoaded()` 会等到每个运输成员的乘客数达到类型的 `Passengers` 容量。如果只准备 3 名乘客却使用 5 座车辆，这一步会持续等待。人数不足时，可直接从 `load()` 进入移动命令。

普通车辆运输链可用 `load()` → `waitLoaded()` → `moveTo(landing)` → `unload()` → 后续目标命令。综合验收已通过从地图招募 1 辆 HTK 和 5 名 E2、满载后移动、卸载并继续执行脚本的真实资源路径；卸载模式及模板的 `transportsReturnOnUnload` 会影响哪些成员继续留队。

两栖运输也用同一套命令：招募地图上的 LCRF、SAPC 或 YHVR 和 12 名步兵，`load()` → `waitLoaded()` → `moveTo(水中航点)` → `moveTo(对岸陆地航点)` → `unload(UnloadMode.KeepAll)`。水域和完整海岸需先在地图中绘制；原版三种运输船都是 12 座，少于 12 人时应省略 `waitLoaded()`。水中公共快照恢复的即时载荷保持，但后续卸载帧和部分乘员落脚格可能与连续运行不同；不保证恢复后的逐帧轨迹完全相同。

卸载模式决定下车后谁继续执行小队脚本：

| 模式 | 留队成员（未开启返程时） |
| --- | --- |
| `UnloadMode.KeepAll` | 运输车和乘客 |
| `UnloadMode.ReleasePassengers` | 运输车 |
| `UnloadMode.ReleaseTransports` | 乘客 |
| `UnloadMode.ReleaseAll` | 无；空小队会清理，后续脚本行不执行 |

解除成员不会删除或转国这些单位。普通上车会将乘客移出Team，卸载时再加入运输车所在Team，随后才按模式解除成员。可以让乘客下车后继续执行目标命令：

```ts
// 在defineMission回调内；另导入SDK的UnloadMode，landing和objective为航点引用。
// 将此数组传给小队的orders选项。
const landingOrders = [
  order.load(), order.waitLoaded(), order.moveTo(landing),
  order.unload(UnloadMode.ReleaseTransports),
  order.moveTo(objective), order.waitForever(),
];
```

模板开启 `transportsReturnOnUnload:true` 时，运输车走优先的离队返程路径；乘客仍按卸载模式处理。`KeepAll` 会留下乘客，`ReleaseAll` 则使小队变空。返程使用运输车已有的ArchiveTarget，不能只靠开启开关就假定一定返回起点：`load()` 在运输车已满、找不到还有空位的运输成员时直接完成，不记录返程位置；本例满载车辆卸载后只离队，仍在下车点附近。

要让 `load()` 自动记录返程位置，应让车内仍有空位、且全部待上车成员已经上车，并省略 `waitLoaded()`；后者会一直等空座填满。普通帧对照中，地图声明三座HTK、两名ENGINEER完成装载后记录起点(40,56)，到(52,64)卸载后车辆实际回到起点，原乘客继续留在下车点。满载两座对照无返程记录；两种情况均覆盖载客状态恢复，未通过脚本直接改位置或注入返程记录。

间谍小队可将 `spyWaypoint()` 指向目标建筑所在航点：

```ts
const plant = m.createWaypoint(31, 30, 40);
const agent = m.team('agent', {
  house: m.house(House.Americans),
  waypoint: m.createWaypoint(30, 25, 25),
  units: [{ type: m.unit(Unit.SPY), count: 1 }],
  orders: [order.spyWaypoint(plant), order.waitForever()],
  full: true,
  max: 1,
  reinforce: false,
});
```

引擎仍按原生规则检查间谍身份和建筑的 `Spyable` 属性。真实资源普通帧验收已验证招募地图 SPY、下达 Capture 并实际渗透电厂；成功进入会消耗间谍，不会占领或损坏电厂。可复现报告见 `target/mission-sdk-spy-audit-final-3/report.json`。

## 脚本跳转与成员移除

```ts
const behavior = m.script('branch', [
  order.jump('finish'),
  order.waitForever(), // 被跳过
  order.label('finish'), // 标签不占原生行号
  order.next(),
  order.setGlobal(m.globalFlag(10)),
  order.guard(2),
  order.killMembers(),
]);
```

`jump()` 的行号随命令插入自动重算，`next()` 在正常小队更新中推进。`changeScript()` 按最终小队引用和脚本列表计算真实注册序号；它替换脚本后停在光标 -1，不会自动开始新脚本。

`killMembers()` 经无攻击来源的伤害链处理并计入损失；`removeMembers()` 经 Limbo/UnInit 直接移除，不计入损失。固定引擎的真实步兵验收中，前者发布事件 48（`destroyedOrRemoved`）和 29（`destroyedAny`），但不会发布要求攻击来源的事件 7（`destroyed`）；后者三者均不发布。通知受对象类别和实际原生调用路径限制，不能将 `destroyedOrRemoved()` 当作任意删除操作的通用回调。

## House 状态条件

资金、损失、工厂、建筑类型及最近建造条件使用规则 `owner` 对应的 House。应显式选择地图实际存在的 House 对应国家：

```ts
// 在 defineMission 回调内；导入生成的 Country、Building 及 SDK 的 event、action。
m.rule('funds_ready', {
  owner: m.country(Country.Americans),
  when: event.creditsAtLeast(5321), once: true,
  actions: [action.message(m.defineText('funds_ready', '资金已达到 5321'))],
});
m.rule('factory_missing', {
  owner: m.country(Country.Americans),
  when: event.buildingAbsent(m.building(Building.NAHAND)), once: true,
  actions: [action.message(m.defineText('factory_missing', '兵营已不存在'))],
});
```

`configureHouse({credits: 10})` 的地图单位代表1000游戏资金；`creditsAtLeast(1000)` / `creditsAtMost(1000)` 则直接比较1000，两端均包含等值。`buildingsLost()` / `unitsLost()` 比较累计损失，达到后即满足；`order.killMembers()` 计入损失，`removeMembers()` 不计入。

`noFactories()` 检查非limbo工厂建筑，不是生产队列是否空闲。`buildingExists()` / `buildingAbsent()` 读取当前active计数，与 `all()` 的后续条件组合时不会记住曾经存在/不存在；`buildingBuilt()` 则可锁存最近记录的建筑类型。运行时 `createBuilding()` 创建并被发现的建筑也会更新该记录，不能用它断定玩家通过工厂完成了生产。

`lowPower(country)` / `fullPower(country)` 选择该Country首个House，与规则owner独立，读取引擎保留的电力状态；开局发电量可能在首次建筑/House处理后才更新。没有匹配Country House时，这两个条件都直接满足。

最近建造事件分别使用对应原生类型数组：

```ts
// 另导入生成的 Techno 枚举。
m.rule('vehicle_ready', {
  owner: m.country(Country.Americans),
  when: event.vehicleBuilt(m.techno(Techno.MTNK)), once: true,
  actions: [action.message(m.defineText('vehicle_ready', '灰熊坦克已出厂'))],
});
// 对应类别：event.infantryBuilt(m.techno(Techno.E1))
//           event.aircraftBuilt(m.techno(Techno.ORCA))
//           event.buildingBuilt(m.building(Building.GAPOWR))
```

移动单位的生产开始时已经构造了limbo对象；达到生产进度并成功交付后才更新最近类型记录。取消生产、出厂失败退款均不表示建成，不会得到该类型的建成通知。建筑生产完成后还需正常放置；上面的建筑发现路径也可更新最近记录。

同类别后续建成另一类型会覆盖House最近记录。需要在此前类型匹配后等其他条件成立，可使用 `once:false` 的 `event.all(event.vehicleBuilt(...), otherCondition)` 并在执行动作中 `disableRule` 自身；事件20/21/22能够锁存匹配结果，区别于建筑当前计数32/57。

上述House状态条件单独使用时由owner实际House轮询，没有匹配House便不会获得该轮询；与计时/变量等逻辑条件组合后会增加其他评估入口，部分条件遇到无House owner会直接满足。应使用实际House对应的owner，避免依赖这个空值边界。

国家消灭条件的参数另选该Country的首个House，与规则owner独立：

```ts
// owner提供实际轮询入口；条件观察Russians。
m.rule('enemy_land_gone', {
  owner: m.country(Country.Americans),
  when: event.countryLandDestroyed(m.country(Country.Russians)), once: true,
  actions: [action.message(m.defineText('enemy_land_gone', '敌方陆军已消灭'))],
});
```

| 条件 | 原生统计口径 |
| --- | --- |
| `countryUnitsDestroyed()` | 在场Unit载具（包括海军）和Infantry步兵两类计数分别 <= 0 |
| `countryBuildingsDestroyed()` | 跟踪建筑数 <= 0 |
| `countryAllDestroyed()` | 上面两个条件同时满足 |
| `countryNavyDestroyed()` | 跟踪 `Naval=yes` 且 `Passengers=0` 的Unit数 <= 0 |
| `countryLandDestroyed()` | 跟踪车辆数减海军数 <= 0，并且跟踪步兵数 <= 0 |

Unit/Infantry/Aircraft是原生对象类别，不能只按是否飞行判断：夜鹰SHAD属于Unit，ORCA属于Aircraft。`countryUnitsDestroyed()` / `countryAllDestroyed()` 都不统计Aircraft；活步兵进入盟友运输车的limbo后也不再计入在场数，因而可能满足这两个条件，普通运输中的步兵却仍阻止 `countryLandDestroyed()`。卸载回地图后，在场数恢复；这些消灭条件与其他条件组合时会重新读取当前计数，不锁存曾经清空的状态。

有载客容量的海军运输工具（如LCRF）不计入海军数，却仍阻止陆军消灭。动作 `destroyHouseObjects(country, HouseObjectSubset.Naval)` 则按类型 `Naval` 标志筛选，也会摧毁海军运输工具；`Land` 子集包含非Naval的Aircraft类。子集动作跳过limbo，不能把动作集合与同名事件的统计口径视为相同。没有匹配Country House时，以上五种条件都直接满足；仍需为规则提供实际评估入口。

## 小队离场通知

`event.teamLeftMap()` 接受前向小队引用。观察规则应显式指定对应实际 House 的 owner，下面的 `House.Americans` 须已存在于地图：

```ts
m.rule('patrol_left', {
  when: event.teamLeftMap(m.teamRef('patrol')),
  owner: m.country(Country.Americans),
  once: true,
  actions: [action.message(m.defineText('patrol_left', '巡逻队已离场'))],
});
const patrol = m.team('patrol', {
  house: m.house(House.Americans),
  waypoint: m.createWaypoint(30, 40, 60),
  units: [{ type: m.unit(Unit.PDPLANE), count: 2 }],
  full: true, max: 1, reinforce: false,
  orders: [order.guard(4), order.mission(UnitMission.Retreat)],
});
m.rule('dispatch_patrol', {
  when: event.sinceScenarioStart(1), once: true,
  actions: [action.recruit(patrol)],
});
```

此例招募地图已有的两架 PDPLANE；使用 `Country`、`Unit` 的生成枚举和 SDK 的 `UnitMission`。引擎在指定 TeamType 的小队成员清空且离场标记置位时发出23通知，第一名成员消失时不会提前触发。该标记不保证几何出界：正常步兵 AI 也可能置位，所以场内 `removeMembers()` 仍可通知；载具移除、`deleteTeam()` 直接解散和持续等待不等同于离场。owner 国家没有对应 House 时，收到任意23广播就会满足条件，绕过 TeamType 筛选。

独立普通帧审计覆盖上述路径及同型飞机等待对照；headless 快照恢复保留列明的小队和成员状态。恢复会重置现有场景随机流，飞机选择返航边缘后的离场帧可能变化，不能据此保证与连续运行逐帧一致。

## House 显示名与增援方向

已有 House 用 `configureHouse()`，新建 House 用 `createHouse()`，均可设置文本引用和增援方向：

```ts
import { HouseEdge } from './sdk';

const support = m.createHouse('support', {
  country: m.country(Country.Alliance),
  uiName: m.defineText('support_name', '友军指挥部'),
  edge: HouseEdge.West,
});
m.configureHouse(m.house(House.Americans), {
  uiName: m.defineText('player_name', '前线指挥部'),
  edge: HouseEdge.North,
});
// support 可作为 m.team(..., { house: support, ... }) 的所属方。
```

`Country`、`House` 来自当前地图生成的 `map.generated.ts`，示例国家须存在于活动目录。`uiName` 要求 `Text` 引用，可用 `defineText()` 或 `text()`；CSF 标签最多 31 个 ASCII 字节，客户端显示内容截取前 20 个 UTF-16 单元。试玩和发布检查该文本依赖。当前战役会保存该CSF名称，但正常战役外交键、任务简报和独立结算没有House名称列表；不保证声明后会在画面中出现。Skirmish/LAN/WOL中人类使用房间昵称、电脑使用本地化电脑名，Neutral/Special使用类型名，地图UIName不替换这些来源。四种模式的实际名称来源已通过28项检查，未改引擎添加显示入口。

方向有 `North`、`East`、`South`、`West`、`Automatic`、`Air`。四个方位影响读取 House/Edge 的增援路径；`Automatic` 恢复按所属方起点选择，`Air` 虽解析为 4，但飞机入场同样回退到起点边缘。显式 `useTransportOrigin:true` 的 `transportWaypoint` 优先，不会被 `edge` 覆盖。此配置不修改地图起点，也不保证所有单位增援方式都读取它。

固定 House 的小队在生成时使用该 House 的有效 Country；原生增援选择该国家的第一个 House。若指定的是同国的后续 House，构建会报错，避免单位误归属；可以调整 House 的 Country 或在多人任务中用 `playerAt()` 选择起点玩家。修改 House Country 后，小队编码随之更新。未指定字段保留地图原值，删除声明恢复被接管的原值。

`RatioTeamAircraft`、`RatioTeamInfantry`、`RatioTeamUnits` 当前仅有解析、保存和动作写入，未发现实际生产消费路径，因此不提供无效果的初始配置字段。

## 开局光照

普通、闪电风暴和心灵控制器的地图光照均可在 `defineMission()` 内配置，单人和多人任务都可用：

```ts
m.configureLighting({
  normal: { ambient: 0.8, red: 1, green: 0.9, blue: 0.85, ground: 0.05, level: 0.008 },
  ion: { ambient: 0.4, red: 0.3, green: 0.4, blue: 0.75 },
  dominator: { ambient: 1.5, red: 0.85, green: 0.2, blue: 0.3 },
  dominatorAmbientChangeRate: 0.003,
  nukeAmbientChangeRate: 3,
});
```

分量使用地图原始比例值，`1` 表示 100%；与运行时 `action.setAmbient(80)` 的百分比单位不同。允许负分量与大于 1 的亮度，宿主检查原生读取后的有限值和整数编码边界，不做额外亮度钳制。引擎先读单精度，再将 Ambient/RGB 乘 100、Ground/Level 乘 1000，加 0.01 后向零截断；例如 `blue:-0.25` 最终是 -24，而不是 -25。地图原值和最终显示亮度还会受地形高度、灯光及渲染钳制影响。

`dominatorAmbientChangeRate` 使用原生比例值，乘 1000 后量化成逻辑帧间隔，`0.003` 对应 3 帧；`nukeAmbientChangeRate` 直接使用正整数逻辑帧，两项必须至少产生 1 帧。核爆光照分量由引擎固定，当前没有可用的地图键，不开放 `nuke` 配置。配置光照不会启动风暴或超武；运行时动作仍使用 `action.startLightningStorm()`、`action.setAmbient()` 等接口。

每任务配置一次；仅托管显式字段，省略项沿用地图值，删除配置恢复接管前原值，手工改动已托管字段后重建会报告冲突。真实资源验证覆盖三组光照、核爆与心灵控制器的快慢淡变、模拟返回普通光照、公共快照恢复，以及 Skirmish/LAN/WOL 开局加载；固定客户端的平地 GPU 样本另已验证普通明暗、RGB、风暴、核爆/心灵控制器效果和淡变快慢，侧栏保持原色。

心灵控制器结束后，模拟 ambient 和 RGB 恢复普通值，但原生路径不立即刷新全部格子，已绘制格子可能保留 `dominator.ground` 的亮度影响。因此当它与 `normal.ground` 不同时，不保证结束帧像素立即等同开局；若希望避免这一差异，可让两者使用相同 Ground 值。这里保留引擎行为，高程、点光源、战役画面和其他组合不属于上述 GPU 验收范围。

## 玩家结果动作

开局场景标志也可由 TS 托管。在单人任务中使用：

```ts
m.configureSpecialFlags({
  inert: false,
  fogOfWar: true,
  harvesterImmune: false,
  destroyableBridges: true,
  tiberiumGrows: true,
  tiberiumSpreads: false,
});
```

`inert` 冻结直接／范围伤害；`harvesterImmune` 让自动选敌排除 Rules 的 `HarvesterUnit` 类型，不保证免受手动攻击或直接伤害。`tiberiumGrows:true` 把矿石密度增长调度间隔改为 `Growth×0.3`，false 保持普通速度；关闭增长需用 `configureBasic({ tiberiumGrowthEnabled: false })`。`tiberiumSpreads` 控制向邻格扩散。多人房间会覆盖这六项，SDK 在多人任务中拒绝它们；多人任务仅可调用 `m.configureSpecialFlags({ initialVeteran: true })`，使成功放置的非 MCV 随机起始单位直接为精英，不提升地图预放对象。`MCVDeploy` 在当前固定引擎中只有解析字段、未发现有效执行入口，暂不开放。

每个任务只配置一次，省略字段保留地图现值；删除配置恢复接管前的值，重建会检查手工冲突。这里设置开局值，运行中的矿石总开关仍用 `action.setTiberiumGrowth()`。

单人战役可以在 `defineMission()` 内配置结算评级：

```ts
m.configureRanking({
  parTime: { easy: 20 * 60, normal: 15 * 60, hard: 10 * 60 },
  underParTitle: m.defineText('rank_fast', '优秀指挥官'),
  underParMessage: m.defineText('rank_fast_note', '你提前完成了任务。'),
  overParTitle: m.defineText('rank_slow', '任务完成'),
  overParMessage: m.defineText('rank_slow_note', '下次争取更快完成。'),
});
```

`parTime` 是整数秒，允许 0～359999（99:59:59）；`normal` 对应地图的 `ParTimeMedium`。用时大于基准时使用 `overPar` 文案，相等时仍使用 `underPar`。文案也可引用 `m.text(Text.现有标签)`，CSF 标签须在 30 ASCII 字节以内，中文写在文本内容中。每个任务只调用一次；省略字段保留地图现值，移除配置会恢复接管前的字段，手工修改已接管字段后重建会报告冲突。发布会检查显式评级文本依赖。评级用于胜利结算页，需保持 `skipScore:false`，自身不会触发胜利。

```ts
m.rule('victory', {
  when: event.globalSet(ready), once: true,
  actions: [action.winPlayer()],
});

// 比较本地玩家国家与指定 Country；不匹配会反向设置胜负。
action.winCountry(m.country(Country.Americans));
action.loseCountry(m.country(Country.Russians));
```

`winCountry(country)` 同国胜、异国负；`loseCountry(country)` 同国负、异国胜。二者以及 `winPlayer()` / `losePlayer()`、小队脚本的 `order.winPlayer()` / `order.losePlayer()` 都把结果写到当前本地 Player，触发器 owner 和 Team 所属 House 不改变结果对象。国家动作和小队结果命令按单人模式校验。

`action.winPlayer()` / `losePlayer()` 立即设置结果并跳过 `SavourDelay` 等待；Country 动作、`order.winPlayer()` / `losePlayer()` 会等待规则中的 `SavourDelay`。`action.endScenario()` 在没有既有结果时设置胜利并等待该时间，已有结果则恢复其暂停计时器。真实资源普通帧验收覆盖上述九种路径和三个战役难度，检查结果归属、胜负、输入锁定与等待帧数。另在普通可见客户端核验九条路径及无胜负对照：胜利出现结算页，失败出现重试提示；结果 EVA 实际输出，胜利点击 Continue 后进入第二关并保留全局标记。客户端核验使用当前默认菜单配置，复跑命令见[开发文档](../../docs/development.md)。测试 EVA 使用自定义音调，不代表原版语音保真或 Win/Lose 影片验收。


战役迷雾可以使用 `m.configureSpecialFlags({ fogOfWar: true })`。它使已经探索、当前没有己方视野的区域变暗；未探索区域仍为黑色。`action.revealAt(waypoint)` 是局部探索，不提供持续单位视野，因此可与迷雾配合。编辑器侧四份TS对照任务已经通过真实ALL1战役GPU验收（35项），复跑见[开发文档](../../docs/development.md)。该证据覆盖列明平地样本；多人须使用开局选项，不接受这个单人SpecialFlags配置。

## 购买补给箱建筑，攻击拆开领取车辆

`m.crateLottery()` 用原生任务触发器实现“建筑栏购买 → 选点放置 → Ctrl 强制攻击拆毁 → 在指定航点随机获得车辆”。**不再产生可拾取箱子，也不修改普通箱子的奖励表。**不修改引擎，不依赖随机箱子开关，目前仅支持单人任务。

```ts
import { defineMission } from './sdk';
import { map, House, Building, Unit } from './map.generated';

export default defineMission(map, { id: 'lottery', player: House.Americans }, m => {
  m.crateLottery('supply', {
    ticket: m.building(Building.GAPILL),
    name: '补给箱（攻击拆开）',
    rewards: [m.unit(Unit.HTK), m.unit(Unit.FV)],
    rewardAt: m.createWaypoint(30, 48, 60), // 改为本地图上可通行的奖励到达格
    cost: 500,
    strength: 20,
    prerequisites: [],
  });
});
```

放置后，选中有武器的己方单位，按住 **Ctrl** 点击建筑所在格强制攻击。建筑消失后短暂延迟发奖；奖励通过原生超时空小队增援到达 `rewardAt` 附近，固定归任务 `player`。该位置与建筑放置位置独立，也可传入 `m.waypoint(Waypoint.WP20)`。请设置空旷且适合奖励车辆的地面；原生增援无法放置时不保证发奖成功，不会自动退款或转金钱。

同时最多一座补给箱建筑，存在及结算期间暂时隐藏该建筑生产，发奖后恢复购买；就绪未放置、取消生产或一直不拆毁都不发奖。`ticket` 的整个类型改作专用补给箱，不能在地图上预放，也不要用其他触发器创建它；建造国家限定为玩家 Country，不能与其他 House 共用该 Country。原生没有新建建筑的逐实例销毁通知绑定，实际检测为玩家该类型建筑“存在 → 不存在”，脚本移除等失去建筑的情况也会触发。不是逐座并发抽奖。

示例沿用 GAPILL，**外观和侧栏图标仍是机枪碉堡**。木箱建筑外观需活动 Mod 提供建筑 ART、SHP 与 Cameo；SDK 不生成美术或克隆新建筑。建议选普通小型建筑，避开围墙、工厂、升级组件。`strength` 默认 20；`cost` 是原始 credits，最终价格受游戏倍率影响；`prerequisites` 省略保留前置，空数组清除；`name` 默认“补给箱（攻击拆开）”。卖出、修理、武器、死亡爆炸、残骸与掉箱均禁用。

每任务一个抽奖配置，奖池 1～64 种车辆。原生随机计时器竞争选出一项，同秒到期按 Tag 遍历顺序决胜，**不保证等概率**，不提供独立权重。奖励副本设置 `CrateGoodie=no`，普通车型的资格、`Powerups` 和 `CrateRules` 保持原样；这套发奖不走原生箱子补矿车分支。旧版使用者补上 `rewardAt` 后重新构建，托管清单会恢复之前改过的全局箱子字段。删除声明可恢复建筑与移除辅助触发器，手改托管字段会报告冲突。


### 只修改抽中的车辆属性

用 `m.rewardUnit(id, { base, name, rules })` 创建独立的奖励车辆，再把返回值放入 `crateLottery.rewards`。普通生产的原车辆保持原有属性；同一基础车型可生成多个奖励版本。`rules` 使用原生 RULES 字段名，值为字符串、数字或 boolean，boolean 转换成 `yes` / `no`；未写的字段继承基础车辆的当前规则。

完整可构建的[奖励单位属性 demo](reward-unit/mission.ts)包含生命、装甲、移动、武器、升级、免疫、隐身、探测、运输、弹药、采矿、展开、子机、外观和声音等分组，适用条件与使用步骤见[说明](reward-unit/README.md)。主配置直接启用，特殊机制以注释形式供按需选用。

奖励副本以稳定 ID 注册为地图自己的 VehicleTypes，沿用基础单位的 Image；SDK 管理其 `TechLevel=-1`、`AllowedToStartInMultiplayer=no` 和 `CrateGoodie=no`，不进入普通生产栏、开局随机兵种或普通箱子奖池。返回值只用于奖励池，不能作为普通小队单位引用。首次使用或资源切换后，先重新加载场景并刷新 SDK 类型。删除声明并构建会移除托管副本。
