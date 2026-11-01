# 开发与验证

[返回 README](../README.md) · [引擎依赖说明](../vendor/README.md)

构建环境和首次运行步骤见 [快速开始](../README.md#快速开始)。以下命令从仓库根目录执行，构建和验证产物写入 `target/`，不加入版本控制。

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
| `crates/mission-script/` | TypeScript 转译、任务 SDK 与脚本执行 |
| `crates/app/` | 工作台界面与编辑协调 |
| `docs/` | 使用、命令行与开发文档 |
| `examples/` | 资源配置、发布预设和任务脚本示例 |
| `tools/` | 格式化与兼容验证工具 |
| `vendor/vera20k/` | 固定版本的引擎子模块 |

## 测试与静态检查

```powershell
cargo test --locked --workspace
cargo clippy --locked --workspace --no-deps --all-targets -- -D warnings
./tools/format_workspace.ps1 -Check
```

格式化时去掉 `-Check`。使用该脚本可只处理编辑器 workspace，避免 `cargo fmt --all` 遍历本地引擎依赖。

## UI 交互录像

补全与表单快捷键的 `egui_kittest` 测试包含控件名称检查和缺字检查。安装 `ffmpeg` 并加入 PATH 后，可选择录制补全交互过程，MP4 写入 `target/ui-tests/recordings/`：

```powershell
$env:KITTEST_RECORD = '1'
cargo test --locked -p ra2-map-app --features ui-test-video completion::tests -- --test-threads=1
Remove-Item Env:KITTEST_RECORD
```

## 真实资源验证

窗口测试需要本机游戏目录；地图、截图与报告只写入本地 `target/`。

```powershell
# 编辑、撤销 / 重做、恢复、保存、重开与截图
cargo run --locked -- --p2-smoke-test --game-dir 'D:/Games/YR' --new-map 80x80

# 随机地图真实资源验证，输出地图、预览图和报告
cargo run --locked -p ra2-map-scene --example random_map_audit -- 'D:/Games/YR' target/random-map-audit
```

窗口烟测的其他入口和输出目录参数见 `cargo run --locked -- --help`。格式校验和静态预览不能替代游戏运行时验证，当前功能边界见 [README](../README.md#当前范围)。

## 引擎版本管理

编辑器使用 `vendor/vera20k` 子模块固定的引擎提交，切换编辑器版本后运行：

```powershell
git submodule update --init --recursive
```

引擎是独立 workspace；子模块提交与 `Cargo.lock` 共同构成可复现构建输入。初始化、恢复和升级的完整流程见 [引擎依赖说明](../vendor/README.md)。
