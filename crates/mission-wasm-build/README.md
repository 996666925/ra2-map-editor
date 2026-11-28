# @ra2-map-editor/mission-wasm-build

VERA20k AssemblyScript 扩展构建工具，提供 `ra2-mission-project` 命令。

## 安装

使用 Node.js 22.15 或更新版本。在扩展项目中安装：

```sh
npm install --save-dev @ra2-map-editor/mission-wasm-build@0.1.3 assemblyscript@0.28.20
```

## 构建

在扩展项目根目录运行：

```sh
npx ra2-mission-project .
```

输出路径：

```text
dist/
  rules.wasm
  ui.wasm
```

调试构建：

```sh
npx ra2-mission-project . --debug
```

单独构建 Rules 或 UI：

```sh
npx ra2-mission-project rules dist/rules.wasm
npx ra2-mission-project ui dist/ui.wasm
```

## 项目配置

`create-vera-app` 可生成以下结构的项目：

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
  shared/protocol.ts
```

在根 `package.json` 中添加扩展项目标识：

```json
{
  "vera": {
    "kind": "extension"
  }
}
```

两个模块分别使用各自的 `vera.json`。以下为 `rules/vera.json`：

```json
{
  "kind": "rules",
  "entry": "src/main.ts",
  "vars": [],
  "notify": []
}
```

`ui/vera.json` 将 `kind` 改为 `ui`。`entry` 相对于模块目录；UI 的 `vars` 和 `notify` 保持为空。

依赖和 npm 脚本放在根 `package.json` 中；根目录不放 `vera.json`，模块目录不放 `package.json`。

## 命令格式

```text
ra2-mission-project PROJECT [OUTPUT] [--debug]
```

| 参数 | 说明 |
| --- | --- |
| `PROJECT` | 扩展项目根目录，或含有 `vera.json` 的模块目录 |
| `OUTPUT` | 单模块构建的输出文件路径，相对于当前工作目录 |
| `--debug` | 启用调试构建 |

构建整个扩展时省略 `OUTPUT`，产物写入项目根目录的 `dist/`。构建单模块时必须传入 `OUTPUT`；独立模块的 `vera.json` 使用 `kind: "rules"` 或 `kind: "ui"`。

## 使用注意

- 构建前会生成声明。单独构建联合项目的一侧时，也会生成另一侧的声明，但只更新指定的 Wasm 文件。
- 两侧编译成功后才更新联合构建的产物。编译失败时保留已有 Wasm，生成的声明可能已更新。
- 检测到构建期间配置发生变化时，构建会终止；修改完成后重新运行命令。
- 不要同时运行写入相同输出路径的构建。两个文件的替换不是原子操作，进程中断也可能留下不完整的产物组合。
- 供 INI 调用的命名动作，应在项目源码中用 `namedAction("Name", handler)` 注册。名称使用字符串字面量，保留 `namedAction` 的导入名称；在 npm 辅助包中注册的动作不会被收集到动作元数据中。
