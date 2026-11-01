# YR Map Editor

为 [VERA20k](https://github.com/YuriPlanet/vera20k) 制作的《红警 2：尤里的复仇》地图编辑器，使用 Rust、egui 和 wgpu 构建。

支持从地形与对象编辑、任务制作，到保存、试玩和地图发布的完整流程。编辑器需要用户自行提供合法的游戏资源，仓库不包含游戏素材。

[快速开始](#快速开始) · [使用指南](docs/user-guide.md) · [命令行与批处理](docs/cli.md) · [开发与验证](docs/development.md) · [任务脚本](examples/missions/README.md)

## 可以做什么

| 方向 | 主要能力 |
| --- | --- |
| 地图创建 | 六种环境；可配置种子、地貌、水域、山地、资源和植被的随机地图；2～30 个玩家起点 |
| 地形绘制 | 子块与完整模板、框选铺地、填充、高度、LAT 过渡、坡道、悬崖、高台、道路、水岸和桥梁 |
| 对象编辑 | 建筑、车辆、步兵、飞机、树木、污迹、覆盖物和起点；SHP/VXL 外观预览与拖动连刷 |
| 区域操作 | 按层选择、复制、剪切、粘贴、移动、删除、克隆，以及整笔撤销和重做 |
| 任务制作 | House、触发器、Tag/CellTag、变量、特遣部队、脚本、小队、基地节点和 AI 触发器；TypeScript 任务脚本 |
| 地图设置 | 名称、尺寸、可玩区域、基础光照和箱子规则 |
| 保存与交付 | 后台保存、备份、自动恢复、内嵌预览、PNG 导出、独立 VERA20k 试玩、战役目录和任务包发布 |
| Mod 与自动化 | 资源配置、发布预设，以及无需窗口或 GPU 的地图校验、图片导出和批量发布 |

六种环境为 `TEMPERATE`、`SNOW`、`URBAN`、`NEWURBAN`、`DESERT`、`LUNAR`；随机地图支持其中除月球外的五种。

## 快速开始

### 1. 准备环境

- Rust **1.98.0**（含 Cargo），版本由 [rust-toolchain.toml](rust-toolchain.toml) 固定。
- Git 和对应平台的编译工具；Windows 需要 **MSVC C++ 构建工具**。
- 本机《尤里的复仇》或相应 Mod 的游戏资源目录。
- 如需试玩，另行准备 `vera20k.exe`。

### 2. 初始化并启动

克隆时使用 `git clone --recurse-submodules <仓库地址>`，进入仓库根目录后运行：

```powershell
git submodule update --init --recursive
cargo run --locked
```

首次启动后，在窗口中选择游戏资源目录，再新建或打开地图。磁盘上的地图也可以直接拖入窗口。

引擎通过 `vendor/vera20k` 子模块固定版本。切换编辑器版本后，再运行一次上述子模块初始化命令；源码 ZIP 不包含子模块，请通过 Git 克隆。详见 [引擎依赖说明](vendor/README.md)。

### 3. 编辑、保存和试玩

1. 在“文件 → 新建地图…”创建空白地图，或选择“新建随机地图…”。
2. 从顶部工具栏选择工具，在左侧选择素材和参数，在画布中绘制；右侧提供地图导航、对象属性和图层控制。
3. 按 `Ctrl+S` 保存。每笔编辑可用 `Ctrl+Z` 撤销，表单中的有效输入会在保存前提交。
4. 点击“试玩”，选择遭遇战或单人任务并指定 `vera20k.exe`。试玩使用当前地图快照，包含尚未保存的修改。

地形工具、对象属性、任务制作、恢复与导出等详细说明见 [使用指南](docs/user-guide.md)。

### 常用启动命令

```powershell
# 打开游戏 MIX 归档内的地图
cargo run --locked -- --game-dir 'D:/Games/YR' --map XMP03T4.MAP

# 打开磁盘地图
cargo run --locked -- --game-dir 'D:/Games/YR' --map 'D:/Maps/mission.map'

# 新建 80×80、8 个起点的温带地图
cargo run --locked -- --game-dir 'D:/Games/YR' --new-map 80x80 --players 8 --theater TEMPERATE

# 构建发布版本
cargo build --locked --release
```

Windows 可执行文件位于 `target/release/ra2-map-editor.exe`。也可通过 `RA2_DIR` 环境变量指定游戏目录，或用 `--profile` 加载 Mod 资源配置。更多参数和无窗口命令见 [命令行与批处理](docs/cli.md)。

## 常用操作

| 操作 | 快捷键或鼠标 |
| --- | --- |
| 绘制、放置、选择 | 左键；地形、资源和对象支持拖动连续绘制 |
| 平移 | 中键拖动、右键拖动或空格＋左键拖动 |
| 缩放 | 鼠标滚轮，以鼠标位置为中心 |
| 适应地图 / 原始像素比例 | `Home` / `1` |
| 定位选中内容 | `F` |
| 地形画笔 / 框选铺地 | `B` / `G` |
| 围合悬崖 / 矩形高台 | `C` / `H` |
| 新建 / 保存 / 另存 | `Ctrl+N` / `Ctrl+S` / `Ctrl+Shift+S` |
| 撤销 / 重做 | `Ctrl+Z` / `Ctrl+Y` 或 `Ctrl+Shift+Z` |
| 复制 / 剪切 / 粘贴 | `Ctrl+C` / `Ctrl+X` / `Ctrl+V` |
| 克隆 / 删除 | `Ctrl+D` / `Delete` |
| 取消当前绘制或放置 | `Esc` 或右键 |
| 帮助 | `F1` |

## 文档导航

| 想了解的内容 | 文档 |
| --- | --- |
| 新建地图、随机生成、地形与对象编辑 | [使用指南](docs/user-guide.md) |
| 地图设置、任务制作与表单输入 | [地图与任务设置](docs/user-guide.md#地图与任务设置) |
| 保存、备份、自动恢复和图片导出 | [保存与恢复](docs/user-guide.md#保存与恢复)、[预览与图片导出](docs/user-guide.md#预览与图片导出) |
| 试玩参数、分辨率和多显示器 | [在 VERA20k 中试玩](docs/user-guide.md#在-vera20k-中试玩) |
| 用 TypeScript 编写任务、API 和示例 | [任务脚本说明](examples/missions/README.md) |
| 启动参数、Mod 配置和批量校验、导出、发布 | [命令行与批处理](docs/cli.md) |
| 项目结构、测试、格式检查与真实资源验证 | [开发与验证](docs/development.md) |
| 初始化、恢复或升级引擎子模块 | [引擎依赖说明](vendor/README.md) |

## 当前范围

地图预览使用静态场景，已覆盖常见 SHP/VXL 外观、共享像素深度、House 换色与基础地图光照。完整动画、建筑损伤状态、局部光源及部分桥梁和对象阴影尚未完全实现。

对象放置包含占地、等高、占用与步兵子格检查；完整通行性及自定义 Mod Foundation 仍待补齐。保存会保留未知字段和未修改的原文，但静态预览与格式校验不代表已完整验证所有游戏运行时行为。
