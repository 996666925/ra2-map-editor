# TypeScript 任务制作

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
原生 TeamType 的归属只支持 `<Player @ A>`～`<Player @ H>`，对应地图已有的 0～7 号起点；
普通航点（如 20）以及扩展起点 8 以上不能传给 `playerAt()`，但仍可作为增援或移动位置。
这里的参数是地图起点编号，不是房间座位编号。

外部代码编辑器的类型检查会提示单人模式调用 `playerAt()`、传入普通航点或多人模式指定固定 `player`；
Rust 还会校验伪造的引用。多人模式写入 `Basic/MultiplayerOnly=yes`，不覆盖原有 `Basic/Player`，
生成规则使用中立所属方执行计时和增援。`winPlayer()` / `losePlayer()` 仅支持单人模式，
目前没有按起点指定玩家胜负的动作。小队行为仍由原生脚本控制，赠送不会自动取消警戒等命令。

## 当前 API

- 引用：`m.house`、`m.playerAt`（多人起点玩家）、`m.waypoint`、`m.unit`、`m.object`、`m.text`。
- 定义：`m.team`、`m.flag(name, initial?)`、`m.rule`，规则必须 `once: true`。
- 事件：`sinceScenarioStart`、`sinceEnabled`、`destroyed`、`flagSet`、`flagClear`。
- 行为：`moveTo`、`attackWaypoint`、`patrol`、`guard`、`hunt`。
- 动作：`reinforce`、`recruit`、`message`、`setFlag`、`clearFlag`、`winPlayer`、`losePlayer`。

对象引用使用生成目录中的 `Structures:0`、`Units:3` 等实际记录键。
目标已经有 Tag 或存在重复物理记录时拒绝绑定，同一对象暂时只能绑定一条脚本规则。
这里尚未提供自定义对象别名或跨外部工具重排的 UUID 身份恢复。

`m.text` 引用已加载的 CSF 标签，自定义文本和媒体继续通过现有战役资源配置发布。
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
