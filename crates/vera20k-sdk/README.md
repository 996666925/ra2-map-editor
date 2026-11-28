# @ra2-map-editor/mission-sdk

VERA20k AssemblyScript 扩展开发 SDK，提供游戏逻辑、客户端界面及单位、建筑、日志和计时器接口。

## 安装

```sh
npm install @ra2-map-editor/mission-sdk@0.1.3
```

本版本基于 `@vera20k/sdk@0.1.0`，使用 AssemblyScript `0.28.20`。

## Rules：游戏逻辑

在 `rules/src/main.ts` 中从主入口导入接口：

```ts
import {
  Building,
  BuildingType,
  Cell,
  House,
  Unit,
  UnitType,
  onScenarioStart,
} from "@ra2-map-editor/mission-sdk";

onScenarioStart((): void => {
  const owner = House.fromIndex(0);
  Unit.spawnByName(UnitType.RhinoTank, owner, Cell.at(20, 20));
  Building.placeByName(BuildingType.AlliedPowerPlant, owner, Cell.at(22, 20));
});
```

该示例在场景开始时，为索引为 `0` 的阵营创建犀牛坦克和盟军发电厂。

| 接口 | 常用操作 |
| --- | --- |
| `Unit` | `spawnByName`、`setMission`、`heal`、`setOwner`、`findNear` |
| `Building` | `placeByName`、`setOnline`、`sell`、`findNear` |
| `UnitType` | 单位 INI 名称，如 `RhinoTank`；`resolve` 解析类型 |
| `BuildingType` | 建筑 INI 名称，如 `AlliedPowerPlant`；`resolve` 解析类型 |

类型名称按当前游戏规则解析，也可以传入 Mod 自定义名称。创建失败返回 `NO_HANDLE`；返回 `Status` 的操作应检查状态码。

### 日志与计时器

```ts
import {
  console,
  onScenarioStart,
  setTimeout,
} from "@ra2-map-editor/mission-sdk";

function showMessage(): void {
  console.log("已经过 15 个模拟帧");
}

onScenarioStart((): void => {
  setTimeout(showMessage, 15);
});
```

| 接口 | 说明 |
| --- | --- |
| `console.log/info/debug/warn/error(message)` | 将字符串写入任务日志 |
| `console.assert(condition, message)` | 条件为假时写入错误日志 |
| `setTimeout(callback, delayFrames)` | 延迟指定帧数执行一次，返回计时器句柄 |
| `setInterval(callback, intervalFrames)` | 每隔指定帧数执行，返回计时器句柄 |
| `clearTimeout(id)`、`clearInterval(id)` | 取消计时器 |

计时器单位是**模拟帧，不是毫秒**。小于 `1` 的间隔按 `1` 帧处理。在场景启动后注册计时器，回调使用顶层函数或不捕获局部变量的函数。

## UI：客户端界面

在 `ui/src/main.ts` 中使用 `/client` 入口：

```ts
import { Client, onStart } from "@ra2-map-editor/mission-sdk/client";

onStart((): void => {
  Client.log("UI 模块已启动");
});
```

通过 `Client.get` 读取客户端可见的对象属性。自定义游戏逻辑请求通过 `Client.sendMessage` 发送，由 Rules 的 `onMessage` 接收、校验并执行。

Rules 与 UI 独立运行。共享源码用于协议、枚举和常量；UI 中不要调用 Rules 的状态读写接口或模拟帧计时器。

## 类型检查与构建

`create-vera-app` 生成的项目提供以下命令：

```sh
npm run check
npm run build
```

手动配置项目时，在 `tsconfig.json` 中继承 SDK 的类型配置：

```json
{
  "extends": "@ra2-map-editor/mission-sdk/tsconfig.base.json",
  "include": ["rules/src/**/*.ts", "ui/src/**/*.ts", "shared/**/*.ts"]
}
```

该配置提供 `i32`、`u32` 等 AssemblyScript 类型。TypeScript 用于类型检查，Wasm 由 `@ra2-map-editor/mission-wasm-build` 调用 AssemblyScript 编译。

扩展运行环境不提供 DOM、`window`、`document` 或浏览器网络接口。
