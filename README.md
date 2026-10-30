# YR Map Editor

使用 Rust、egui 和 wgpu，为 [VERA20k](https://github.com/YuriPlanet/vera20k) 制作的《红警 2：尤里的复仇》地图编辑器。

## 功能

- 六种地图环境：TEMPERATE、SNOW、URBAN、NEWURBAN、DESERT、LUNAR。
- 可配置的随机地图生成：五种地貌、随机种子、水域、山地、矿区、植被与中立科技建筑。
- 地形子块与完整模板绘制、填充、高度调整、LAT、坡面、悬崖、高台、道路、水岸与桥梁工具。
- 建筑、车辆、步兵、飞机、树木、污迹、覆盖物及玩家起点编辑；SHP/VXL 外观预览。
- 按层选择区域，复制、剪切、粘贴、移动、删除与对象克隆；整笔撤销和重做。
- House、触发器、Tag/CellTag、变量、特遣部队、脚本、小队、基地节点与 AI 触发器编辑。
- 地图尺寸与可玩区域调整、基础地图光照、整图 PNG 导出及 PreviewPack 生成。
- 后台保存、备份、自动恢复、独立 VERA20k 试玩、战役目录与任务包发布。
- Mod 资源配置，以及无需窗口的地图校验、图片导出和批量发布。

编辑器需要用户自行提供合法的游戏资源，仓库不包含游戏素材。

## 构建与运行

需要 Rust 1.98.0（含 Cargo）、对应平台的编译工具和 Git。Windows 还需要 MSVC C++ 构建工具。`rust-toolchain.toml` 固定工具链版本。

克隆仓库时使用 `git clone --recurse-submodules <仓库地址>`；已有 checkout 在根目录初始化引擎子模块：

```powershell
git submodule update --init --recursive
cargo run --locked
```

启动后在窗口中选择游戏目录，也可以使用命令行：

```powershell
# 打开 MIX 内地图或磁盘地图
cargo run --locked -- --game-dir 'D:/Games/YR' --map XMP03T4.MAP
cargo run --locked -- --game-dir 'D:/Games/YR' --map 'D:/Maps/mission.map'

# 新建地图
cargo run --locked -- --game-dir 'D:/Games/YR' --new-map 80x80 --players 8 --theater TEMPERATE

# 构建发布版本
cargo build --locked --release
```

Windows 可执行文件位于 `target/release/ra2-map-editor.exe`。也可通过环境变量 `RA2_DIR` 指定游戏目录，或使用 `--profile` 载入资源配置。完整参数见 `--help`。

VERA20k 通过 `vendor/vera20k` Git 子模块固定版本；切换编辑器版本后再次运行 `git submodule update --init --recursive`。源码 ZIP 不包含子模块，需要通过 Git 克隆。初始化与更新方法见 [引擎依赖说明](vendor/README.md)。

## 基本操作

顶部工具栏常驻选择、框选、吸管、对象、矿石等工具，并可直接进入填充、悬崖、高台、坡道、水域和道路。左侧集中显示工具参数和素材，右侧用于地图导航、对象属性和显示图层。地图文件也可以直接拖入窗口。

左侧地形按“地表、悬崖、水岸、道路 / 桥”分组。画悬崖可以直接选择“围合悬崖”描绘轮廓，或用正面 / 背面悬崖画线，用矩形高台扩建，再插入坡道入口。自动工具无需挑选地形素材；“手工拼贴地形…”可转入完整模板编辑。填充模式只列出 1×1 素材，选择其他素材后仍保持填充。

| 操作 | 快捷键或鼠标 |
| --- | --- |
| 绘制、放置、选择 | 左键点击；地形和资源支持拖动连续绘制 |
| 平移 | 中键拖动、右键拖动或空格＋左键拖动 |
| 缩放 | 鼠标滚轮，以鼠标位置为中心 |
| 适应地图 / 原始像素比例 | `Home` / `1` |
| 定位选中内容 | `F` |
| 地形填充 / 围合悬崖 / 矩形高台 | `G` / `C` / `H` |
| 新建 / 保存 / 另存 | `Ctrl+N` / `Ctrl+S` / `Ctrl+Shift+S` |
| 撤销 / 重做 | `Ctrl+Z` / `Ctrl+Y` 或 `Ctrl+Shift+Z` |
| 复制 / 剪切 / 粘贴 | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` |
| 克隆 / 删除 | `Ctrl+D` / `Delete` |
| 取消当前绘制或放置 | `Esc` 或右键 |
| 帮助 | `F1` |

新建地图宽高各为 20～492，宽＋高不超过 512；最大正方形为 256×256，初始高度为 0～14，玩家起点支持 2～30 个。区域选择时使用 `Shift` 添加、`Alt` 减选。

在“文件 → 新建地图…”下方选择“新建随机地图…”，可生成群岛、大陆、海中岛屿、内陆湖泊或山地地图。随机生成支持温带、雪地、城市、沙漠和新城市，支持 2～30 名玩家；宽高分别选择四档尺寸，窗口会显示由档位和玩家数决定的实际尺寸。超过 8 人时按玩家数量扩大地图面积，并为全部玩家生成起点、安全空地和资源。可以调整水域规模、崎岖度、通行开阔度、植被密度、起点分散程度和资源丰度，并开关矿区、宝石及中立科技建筑。

生成复用 VERA20k 的随机地图管线：随机地块生长与水域整形、岸线拼接、区域划分与起点选择、矿区扩散、山地和地表过渡、树林散布。9～30 人的群岛地图生成多个大岛，让多名玩家共享岛屿。它在后台生成真实地形和对象，完成后成为可编辑、待保存的新地图。每次打开窗口会更新随机种子，也可手工指定或点击“换一个”；相同版本、游戏资源、参数和种子可复现结果。生成器会自动跳过缺少玩家起点的候选地图，最多尝试 16 次；地图的 `[RandomMap]` 节保存最终种子和完整参数，`EditorRequestedSeed` 保留最初输入的种子。水域等强度数值不等于最终占地百分比；若极端参数仍无法容纳全部起点，会提示调整参数或更换种子。

一笔操作形成一次撤销。后台预览更新期间保留当前地图画面，准备完成后切换。对象属性、任务参数和光照在结束输入后自动生效：文本按回车或移开焦点，数值拖动松手后生效，勾选和下拉选择无需额外确认。每次完成的编辑可撤销；无效输入留在表单中提示，可修正或取消本次输入。地图尺寸裁切仍需检查预览并确认应用；保存地图继续使用 Ctrl+S。

保存会生成地图预览并重开校验，保留最多三份 `.yr-editor-*.bak` 备份。空闲后自动生成恢复快照；Windows 默认位置为 `%LOCALAPPDATA%/ra2-map-editor/recovery/`。恢复后需要手动保存。

点击“试玩”选择遭遇战或单人任务，并指定 `vera20k.exe`；也可以通过 `--playtest-exe PATH` 配置。试玩使用当前快照，包含尚未保存的修改，输出写入独立目录。支持 30 人的 VERA20k 可试玩最多 1 名玩家加 29 个电脑的地图。

## Mod 配置与批处理

资源配置和发布预设示例位于 `examples/`，复制后修改游戏目录和输出路径：

- [Mod profile](examples/profiles/yr.example.json)
- [发布预设](examples/presets/publish.example.json)

以下示例使用 Windows 发布版本，三个批处理命令均无需窗口或 GPU：

```powershell
target/release/ra2-map-editor.exe --profile my-mod.json --map mission.map
target/release/ra2-map-editor.exe validate --game-dir 'D:/Games/YR' --report target/validation.json mission.map
target/release/ra2-map-editor.exe export --game-dir 'D:/Games/YR' --output target/png --max-edge 4096 mission.map
target/release/ra2-map-editor.exe publish --preset examples/presets/publish.example.json mission.map
```

批处理支持多张地图，默认拒绝覆盖已有输出；使用 `--overwrite` 允许替换输出。退出码：`0` 全部通过、`2` 至少一张地图失败、`1` 参数或初始化等错误。各命令的完整选项见对应的 `--help`。

## 项目结构

| 路径 | 职责 |
| --- | --- |
| `src/` | 命令行与原生窗口启动 |
| `crates/document/` | 地图文档、序列化、事务与历史 |
| `crates/assets/` | 游戏资源读取与图像准备 |
| `crates/voxel/` | CPU 体素与静态深度适配 |
| `crates/scene/` | 地图场景、编辑算法、拾取与图片合成 |
| `crates/render/` | GPU 渲染、图集与视口 |
| `crates/io/` | 保存、恢复、导出、发布与试玩 |
| `crates/app/` | 工作台界面与编辑协调 |
| `examples/` | 资源配置和发布预设示例 |
| `tools/` | 格式化与兼容验证工具 |
| `vendor/vera20k/` | 固定版本的引擎子模块 |

构建和验证产物写入 `target/`，不加入版本控制。

## 检查

```powershell
cargo test --locked --workspace
cargo clippy --locked --workspace --no-deps --all-targets -- -D warnings
./tools/format_workspace.ps1 -Check
```

格式化时去掉 `-Check`。使用该脚本可只处理编辑器 workspace，避免 `cargo fmt --all` 遍历本地引擎依赖。

真实资源窗口测试需要本机游戏目录，地图、截图与报告只写入本地 `target/`：

```powershell
cargo run --locked -- --p2-smoke-test --game-dir 'D:/Games/YR' --new-map 80x80

# 随机地图真实资源验证，输出地图、预览图和报告
cargo run --locked -p ra2-map-scene --example random_map_audit -- 'D:/Games/YR' target/random-map-audit
```

## 当前范围

地图预览使用静态场景，已覆盖常见 SHP/VXL 外观、共享像素深度、House 换色与基础地图光照。完整动画、建筑损伤状态、局部光源及部分桥梁和对象阴影尚未完全实现。对象放置包含占地、等高、占用与步兵子格检查，完整通行性及自定义 Mod Foundation 仍待补齐。

保存会保留未知字段和未修改的原文。静态预览与格式校验不代表已经完整验证所有游戏运行时行为。
