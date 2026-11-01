# 命令行与批处理

[返回 README](../README.md) · [使用指南](user-guide.md) · [任务脚本命令](../examples/missions/README.md#无窗口命令)

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
| `--players N` | 新地图的起点数量，2～30，默认 2；须与 `--new-map` 一起使用 |
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

## TypeScript 任务命令

`mission-init` 创建任务源码和辅助类型文件，`mission-build` 转译、执行并校验任务后输出完整地图。运行不需要 Node.js 或 Deno。

这两个命令的参数和输出规则见 [任务脚本说明](../examples/missions/README.md#无窗口命令)，不适用上面的批处理选项表。
