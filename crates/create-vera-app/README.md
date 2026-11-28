# create-vera-app

VERA20k AssemblyScript 扩展项目脚手架。

## 快速开始

使用 Node.js 22.15 或更新版本：

```sh
npm create vera-app@0.1.3 -- my-extension
cd my-extension
npm install
npm run check
npm run build
```

构建产物为 `dist/rules.wasm` 和 `dist/ui.wasm`，可导入地图编辑器的扩展系统。

## 项目结构

```text
my-extension/
  package.json
  tsconfig.json
  rules/
    vera.json
    src/main.ts
  ui/
    vera.json
    src/main.ts
  shared/
    protocol.ts
```

- `rules/src/main.ts`：游戏逻辑，使用 `@ra2-map-editor/mission-sdk`。
- `ui/src/main.ts`：客户端界面，使用 `@ra2-map-editor/mission-sdk/client`。
- `shared/protocol.ts`：两侧共用的消息类型、枚举和常量。
- `rules/vera.json`、`ui/vera.json`：模块入口与声明配置。

从对应的 `main.ts` 开始编写扩展，在项目根目录运行开发命令。

## 开发命令

| 命令 | 作用 |
| --- | --- |
| `npm run check` | 生成声明并检查类型 |
| `npm run build` | 构建 Rules 和 UI |
| `npm run build:rules` | 构建 Rules |
| `npm run build:ui` | 构建 UI |
| `npm run build -- --debug` | 调试构建 |

## 命令行选项

```text
create-vera-app <directory> [--packages-dir <directory>]
```

| 参数 | 说明 |
| --- | --- |
| `<directory>` | 项目路径，例如 `my-extension` |
| `--packages-dir <directory>` | 使用指定目录中的本地 npm 安装包 |
| `--help`、`-h` | 查看帮助 |

### 本地安装包

测试本地版本时，将以下安装包放到 `./packages/`：

```text
create-vera-app-0.1.3.tgz
ra2-map-editor-mission-sdk-0.1.3.tgz
ra2-map-editor-mission-wasm-build-0.1.3.tgz
```

```sh
npm exec --package ./packages/create-vera-app-0.1.3.tgz -- create-vera-app my-extension --packages-dir ./packages
cd my-extension
npm install
npm run check
npm run build
```

`--packages-dir` 将 SDK 和构建器依赖指向本地安装包。目录中若有匹配版本的引擎 SDK、AssemblyScript 或 TypeScript 安装包，也会使用；其余依赖按模板中的版本从 npm 安装。
