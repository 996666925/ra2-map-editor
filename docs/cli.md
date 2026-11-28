# 命令行与批处理

[返回 README](../README.md) · [使用指南](user-guide.md) · [Wasm 扩展命令](#wasm-扩展命令)

以下示例从仓库根目录执行。开发时使用 `cargo run --locked -- <参数>`，Windows 发布版本使用 `target/release/ra2-map-editor.exe <参数>`。

## 启动编辑器

```powershell
# 打开磁盘地图；--map 也接受 MIX 内地图名，如 XMP03T4.MAP
cargo run --locked -- --game-dir 'D:/Games/YR' --map 'D:/Maps/mission.map'

# 新建地图
cargo run --locked -- --game-dir 'D:/Games/YR' --new-map 80x80 --players 8 --theater TEMPERATE

# 指定试玩程序和优先字体
cargo run --locked -- --game-dir 'D:/Games/YR' --playtest-exe 'D:/Games/YR/vera20k.exe' --font 'C:/Windows/Fonts/msyh.ttc'
```

| 常用参数 | 说明 |
| --- | --- |
| `--game-dir PATH` | 游戏资源目录；也可通过 `RA2_DIR` 指定，未指定时在窗口中选择 |
| `--profile FILE` | 加载 Mod 资源配置 |
| `--map PATH_OR_NAME` | 打开磁盘地图或 MIX 内地图；不能与 `--new-map` 同时使用 |
| `--new-map WxH` | 新建空白地图，尺寸限制见 [使用指南](user-guide.md#空白地图) |
| `--players N` | 新地图的起点数量，2～120，默认 2；须与 `--new-map` 一起使用 |
| `--theater NAME` | 新地图环境，须与 `--new-map` 一起使用 |
| `--playtest-exe PATH` | 用于独立试玩的 VERA20k 可执行文件 |
| `--font PATH` | 优先使用指定字体 |

查看完整参数（包含资源调试和窗口烟测参数）：

```powershell
cargo run --locked -- --help
```

## Mod 资源配置

复制 [Mod profile 示例](../examples/profiles/yr.example.json)，修改 `game_dir` 和 `playtest_exe`，再加载：

```powershell
target/release/ra2-map-editor.exe --profile my-mod.json --map mission.map
```

配置中的相对路径以 **profile 文件所在目录** 为基准。`version` 和 `schema` 必须使用示例中的受支持值，`name` 不能为空。该配置声明编辑器使用的资源和 schema，不代表支持任意引擎扩展。

### 地图附加素材

在「资源与地图来源 → 地图附加素材」中添加额外目录或 MIX，用上下按钮调整优先级，然后点击「保存资源配置并刷新」。可用「导入资源配置…」复用已有 JSON 配置，导入的相对路径以配置文件所在目录为基准。配置保存在地图旁的 .assets.json 文件，打开、拖入、CLI 校验、导出、试玩均自动读取，仍需选择基础 YR 游戏资源目录。

例如，`Campus.yrm.assets.json` 可以写成：

```json
{
  "directories": [
    { "path": "CustomAssets", "base": "map" },
    { "path": "SharedAssets", "base": "game" }
  ],
  "archives": [
    { "path": "CustomAssets/custom.mix", "base": "map" }
  ],
  "ini": {
    "rules": "rules-custom.ini",
    "art": "art-custom.ini"
  },
  "ares": "auto"
}
```

相对路径以地图所在目录为基准。开启 Ares 模式后，补充 ART 可通过自定义 art 入口配合 #include 引用基础 ART。共享地图时请同时带上该配置文件和素材目录。编辑器内的试玩和高清预览会把同一套素材装配到独立会话目录。

加载顺序：编辑器 resources/overrides → high_priority 目录 → directories → 游戏散文件 → archives → 游戏归档 → low_priority 目录 → 编辑器 resources/fallbacks。同组列表从上到下，第一个同名文件优先。普通素材按文件名搜索，游戏相对路径的子目录（如 Taunts）也会保留；额外归档在 MIX 列表中显式添加，不依赖固定名称。回退目录只补当前地图环境实际已挂载资源中的缺项；切换环境会重新判断。原版游戏归档和地形归档的内部加载规则继续交给 VERA。

路径 base 可为 map（地图所在目录，默认）、game（当前游戏目录）或 editor（当前编辑器目录），也支持绝对路径。ini 支持 rules、art、ai、sound、eva、theme 以及 temperate、snow、urban、newurban、desert、lunar 六种地形 INI 入口；这是完整入口替换，相关 include 文件仍按上述顺序搜索。ares 支持 auto、on、off，解析后的同一模式传递给试玩和高清预览。

新增的自定义 MIX 应显式加入归档列表。保存、另存为、CLI publish 保留素材配置，尽量使用相对输出地图位置的路径（包括同盘兄弟目录）。发布保留引用，不自动复制素材；共享时请同时带上配置和素材目录。

外部资源适配全部位于编辑器工程，引擎源码不作修改。编辑器使用引擎现有的 INI 入口接口选择 rules/art/ai/sound/eva 的真实文件名，原版 INI 仍可被 include 独立引用；试玩通过 config.toml 的 [ini] 传递相同入口。地形入口通过独立后缀文件适配。

资源视图按文件内容、配置顺序和地图环境缓存在系统临时目录；复用前核对实际文件清单、大小和内容，缺文件或损坏会重建。子目录文件与符号链接目标内容也参与版本校验，循环目录链接会报错。缓存持有期间通过共享文件锁保护，回收只处理未被使用的旧视图。试玩散文件使用独立副本，大型只读归档优先硬链接或符号链接，无法链接时复制。资源配置使用本文 JSON 格式，未知字段会报错。

## 无窗口批处理

`validate`、`export`、`publish` 均无需窗口或 GPU，支持一次处理多张地图。资源通过 `--game-dir`、`--profile` 或 `RA2_DIR` 提供。

| 命令 | 用途 |
| --- | --- |
| `validate` | 校验地图和已知任务 / 媒体依赖，输出诊断报告 |
| `export` | 使用游戏资源导出完整地图 PNG |
| `publish` | 输出带新生成 `PreviewPack` 的 `.map` 及版本信息附属文件 |

```powershell
# 校验并写入 JSON 报告
target/release/ra2-map-editor.exe validate --game-dir 'D:/Games/YR' --report target/validation.json mission.map

# 批量导出 PNG，最长边限制为 4096 像素
target/release/ra2-map-editor.exe export --game-dir 'D:/Games/YR' --output target/png --max-edge 4096 mission.map skirmish.map

# 按预设发布地图
target/release/ra2-map-editor.exe publish --preset examples/presets/publish.example.json mission.map
```

使用发布预设前，请先复制 [发布预设示例](../examples/presets/publish.example.json) 和它引用的资源配置，并调整路径。预设内的 `game_dir`、`profile`、`output` 相对 **预设文件所在目录** 解析；profile 内的路径仍相对 profile 自身解析。显式命令行选项覆盖预设中的对应值。

### 常用批处理选项

| 选项 | 说明 |
| --- | --- |
| `--preset FILE` | 加载版本 1 的 JSON 预设 |
| `--report FILE` | 保存版本 1 的 JSON 批处理报告；报告也会输出到标准输出 |
| `--output DIR` | 输出目录；`export` / `publish` 必需，也可由预设提供 |
| `--max-edge N` | PNG 最长边，默认 4096；0 表示原始尺寸，允许 0～16384 |
| `--strict` | 将警告也视为校验失败 |
| `--no-dependencies` | 跳过已知任务 / 媒体依赖检查 |
| `--overwrite` | 允许替换已有输出产物 |
| `--` | 后续参数全部作为地图路径或名称处理 |

默认拒绝覆盖已有输出，即使使用 `--overwrite` 也不会覆盖输入地图。多张地图的输出名称必须唯一。批处理不能同时使用指向不同资源目录的 `--profile` 和 `--game-dir`。

### 退出码

| 退出码 | 含义 |
| --- | --- |
| `0` | 全部地图通过 |
| `2` | 至少一张地图校验、加载或输出失败 |
| `1` | 参数、初始化或报告写入等错误 |

可用 `validate --help`、`export --help`、`publish --help` 查看批处理帮助。

## Wasm 扩展命令

编辑器“扩展系统”与 `create-vera-app` 创建的项目都包含 `package.json`。安装 Node.js >=22.15（含 npm），在扩展项目根目录安装依赖、检查和构建：

```sh
cd my-extension
npm install
npm run check
npm run build
```

新建时统一创建联合扩展项目，根目录 `package.json` 通过 `"vera": { "kind": "extension" }` 标识项目，`rules/` 和 `ui/` 各自保留 `vera.json` 与源码入口，依赖统一安装在根目录。`npm run build` 一次输出 `dist/rules.wasm`、`dist/ui.wasm`，`npm run build:rules` 和 `npm run build:ui` 可分别构建。两次编译全部成功后才更新联合构建的最终产物。`shared/` 用于双方共享的协议、常量和纯数据结构。

`vera.json` 的 `kind` 指定模块类型：`rules` 使用逻辑 ABI，输出 `dist/rules.wasm`；`ui` 使用本机界面 ABI，输出 `dist/ui.wasm`。编辑器创建的联合项目会分别设置 `rules/src/main.ts`、`ui/src/main.ts`，并在模块配置中设置 `out`，根 `package.json` 提供 npm 构建脚本。

编辑器不附带 Node.js 或编译器。`wasm-build` 使用系统 Node.js 调用项目内的 `@ra2-map-editor/mission-wasm-build` 构建器，需要先在扩展项目根目录执行 `npm install`。命令按项目配置构建一个或两个模块，校验后输出 `<extension-project>/dist/rules.wasm`、`dist/ui.wasm` 中对应的产物；联合项目两者全部校验成功后才发布。构建期间若项目配置发生变化，命令会拒绝替换原有产物，需重新构建。也可用 [独立构建工具](../crates/mission-wasm-build/README.md)。

`wasm-check`、`wasm-embed`、`wasm-extract` 和 `wasm-remove` 默认操作 `rules`，用 `--kind ui` 操作界面模块。校验分别使用引擎的逻辑沙箱和客户端沙箱，并遵守地图模块限制。两个模块存放在独立地图区块中，嵌入、提取或移除其中一个不会修改另一个模块。内嵌命令按当前引擎设置最低版本门槛，并保留地图原有的更高要求；最后一个模块移除时，若最低版本仍是编辑器设置的值，则恢复原值。地图输出携带原地图的资源配置，保存期间若输出文件被其他程序修改则拒绝覆盖。提取命令要求使用新的输出文件名。

```powershell
# 按项目配置构建；联合项目一次生成两个 Wasm
ra2-map-editor wasm-build my-extension

# 逻辑模块
ra2-map-editor wasm-check rules.wasm
ra2-map-editor wasm-embed input.map rules.wasm output.map
ra2-map-editor wasm-check output.map
ra2-map-editor wasm-extract output.map extracted-rules.wasm

# 界面模块，可与逻辑模块嵌入同一张地图
ra2-map-editor wasm-check --kind ui ui.wasm
ra2-map-editor wasm-embed --kind ui output.map ui.wasm both-modules.map
ra2-map-editor wasm-check --kind ui both-modules.map
ra2-map-editor wasm-extract --kind ui both-modules.map extracted-ui.wasm
ra2-map-editor wasm-remove --kind ui both-modules.map rules-only.map
ra2-map-editor wasm-remove rules-only.map without-modules.map
```
