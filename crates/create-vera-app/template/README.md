# VERA20k AssemblyScript 扩展项目

## 安装与构建

安装 Node.js >=22.15（含 npm），在项目根目录运行：

```sh
npm install
npm run check
npm run build
```

编辑器不附带 Node.js 或 AssemblyScript 编译器。安装依赖后，在编辑器“扩展系统”窗口打开项目并点击“构建”；编辑器使用系统 Node.js 调用项目内的 `@ra2-map-editor/mission-wasm-build` 构建器。打开地图后，可用“构建并嵌入”导入两个模块。

## 项目结构

- `package.json`：管理依赖和脚本，以 `"vera": { "kind": "extension" }` 标识项目。
- `rules/vera.json`、`rules/src/main.ts`：游戏逻辑模块，输出 `dist/rules.wasm`。
- `ui/vera.json`、`ui/src/main.ts`：本机界面模块，输出 `dist/ui.wasm`。
- `shared/protocol.ts`：两个模块共享的协议和常量，不依赖宿主 API。
- `tsconfig.json`：检查 rules、ui 和 shared 源码的类型。

`npm run check` 执行 `vera20k generate rules && vera20k generate ui && tsc --noEmit -p .`，先生成声明，再检查类型。`npm run build` 执行 `ra2-mission-project .`，生成两个 Wasm；也可用 `npm run build:rules`、`npm run build:ui` 单独构建，对应命令为 `ra2-mission-project rules dist/rules.wasm` 和 `ra2-mission-project ui dist/ui.wasm`。

项目使用 `@ra2-map-editor/mission-sdk@0.1.3`、`@ra2-map-editor/mission-wasm-build@0.1.3`、`@vera20k/sdk@0.1.0`、`assemblyscript@0.28.20` 和 `typescript@5.9.3`。

## 模块分工

rules 负责游戏逻辑；ui 从 `@ra2-map-editor/mission-sdk/client` 导入客户端接口，只在本机运行，没有浏览器 DOM。界面读取使用 `Client.get`，修改游戏状态需通过 `Client.sendMessage` 向 rules 发送请求，由 rules 校验并执行。
