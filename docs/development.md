# 开发与验证

[返回 README](../README.md) · [引擎依赖说明](../vendor/README.md)

构建环境和首次运行步骤见 [快速开始](../README.md#快速开始)。以下命令从仓库根目录执行，构建和验证产物写入 `target/`，不加入版本控制。

## 项目结构

| 路径 | 职责 |
| --- | --- |
| `src/` | 命令行与原生窗口启动 |
| `crates/document/` | 地图文档、序列化、事务与历史 |
| `crates/assets/` | 游戏资源读取与图像准备 |
| `crates/voxel/` | GPU / CPU 体素与静态深度适配 |
| `crates/scene/` | 地图场景、编辑算法、拾取与图片合成 |
| `crates/render/` | GPU 渲染、图集与视口 |
| `crates/io/` | 保存、恢复、导出、发布与试玩 |
| `vendor/vera20k/sdk/typescript/` | 引擎内置 TypeScript/AssemblyScript SDK 源码 |
| `crates/mission-wasm-build/` | 项目内 AssemblyScript 构建器，调用引擎 TSSDK |
| `crates/create-vera-app/` | AssemblyScript 任务项目模板 |
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
python tools/check_i18n.py
```

推送分支或提交 PR 时，[Checks 工作流](../.github/workflows/checks.yml) 会在 Windows 上执行上述四项检查。CI 使用固定版本的 Rust 和引擎子模块，不需要游戏资源或 GPU；需要真实资源的忽略测试仍按下文手动执行。全目标 Clippy 包含审计示例，可检查引擎升级后的接口适配。

格式化时去掉 `-Check`。使用该脚本可只处理编辑器 workspace，避免 `cargo fmt --all` 遍历本地引擎依赖。

体素适配层通过 `#[path]` 直接编译部分引擎源码，这些模块不受 Clippy 的 `--no-deps` 排除。它们在模块入口沿用上游源码的 lint 边界，由引擎工作区负责检查；编辑器自身的适配代码仍接受上述严格检查。排查本地通过而 CI 失败的问题时，可先执行 `cargo clean -p ra2-map-voxel`，再运行全目标 Clippy，避免已有构建缓存掩盖这些模块的诊断。

界面本地化、消息编写和新增语言的步骤见 [语言资源说明](../crates/app/locales/README.md)。语言切换、持久化、翻译占位符与中英文布局测试包含在应用测试中。

## 对象编号分配性能

使用合成地图测量已有 100、1,000、3,000 条车辆记录时新增单条记录的耗时，无需游戏资源或 GPU：

```powershell
cargo run --locked -p ra2-map-document --example record_allocation_audit
```

输出 CSV，每种规模取 7 次中位数。每次从同一文档 fork，计时只包含 `add_record`，不含地图准备、fork、连刷占用检查和渲染。对比时使用相同构建配置；默认开发配置的结果不代表发布版本或完整界面性能。

## 折线悬崖资源检查

折线悬崖的真实素材检查可单独运行：

```powershell
cargo run --locked -p ra2-map-scene --example authoring_audit -- 'D:/Games/YR' target/cliff-polyline-audit ALL --polyline-only
```

检查六种环境，包含八方向开放崖壁、左右高侧、多段转角，以及顺逆时针与左右高侧的全部闭合组合；不支持悬崖的环境须拒绝生成。折点包含直角和斜向尖角，两类均覆盖四种旋转与两种高侧，逐一对照闭合轮廓的原生转角模板，并检查转角图像的不透明像素不会渲染成背景空洞，输出 `-detail.png` 局部图。右侧逆时针、左侧顺时针须生成坑，其余组合生成高台，外侧地面保持不变。保存 `.map`、CPU 静态预览和 `report.json`，验证保存重开、引擎地形重载及整笔撤销重做。开放折线逐格核对：只放置完整标准悬崖模板，未写入坡道或额外地面，模板以外的地形在引擎重载后保持不变。闭合轮廓另做像素覆盖检查，核对内部没有新增背景空洞。此检查不包含游戏内单位通行验证。交互回归通过 `cargo test --locked -p ra2-map-app -p ra2-map-scene --lib polyline` 运行，另覆盖低于 4 层挖坑须先抬高地面、外侧与基准不符时拒绝生成，以及 127 层外侧生成 123 层坑底的边界。

## 编辑后的场景刷新

文档提供事务目标与快照差异，普通任务段、已注册脚本/特遣部队/小队的已知字段，以及地图标题和作者等元数据可复用静态视觉场景。后台仍更新地图原文快照，保持标题、导出与编辑数据一致；GPU 实例、图集和视口绑定一并复用，诊断和任务标签按文档版本更新。仅 INI 的提交、撤销和重做不重建对象、小地图等空间缓存。

对象记录继续使用增量更新。地形、覆盖物、光照、House/国家、玩家/会话模式、规则、资源变化和未知扩展继续完整刷新；任务 ID 与类型、House 或安装规则段同名时也保守回退。光照预览配置变化不会复用旧视觉场景。

`LoadTimings.visual_reused` 标记完整视觉场景复用；此路径的 GPU 准备耗时和实例上传字节均为零。CPU 回归覆盖快照差异、撤销/重做、混合修改、资源刷新以及与完整重建的像素比较。以下测试另需本机 GPU，只使用合成资源，核对实际 GPU 和视口对象在任务编辑及历史操作间保持一致，光照编辑则重新绑定：

```powershell
cargo test --locked --workspace --lib mission_refresh_preserves_gpu_and_viewport_bindings_through_history -- --ignored
```

## 表单候选与连刷占用缓存

任务参数、AI 触发器和 CSV 引用控件共享 egui Context 内的候选列表。对象类型和国家候选按资源目录与语言缓存；引用和航点还依赖文档身份及 `edit_revision()`。修订计数覆盖未提交修改、取消、编辑检查点恢复、失败事务回滚、撤销/重做和结构替换，不能只以提交后的 `state_id` 判断有效性。CSF、声音等候选继续直接借用资源缓存，未知 Mod 值仍可自由输入。

文档的 `record_iter()` 和 `record()` 提供包含未提交修改的借用视图，编号分配、航点检查与占用索引无需克隆整张对象表。连刷开始时建立原有对象的占用索引，在当前笔中共享；新增占用由手势维护，Shift 回撤恢复该手势，取消或结束后丢弃索引。下一笔从当前文档重新建立，因此撤销、重做、重叠开关和新的资源配置会被重新读取。

以下 CPU 性能审计使用 1,000 / 3,000 条合成记录及候选，比较每次重建与缓存复用，输出 `target/step5-caches.json`：

```powershell
cargo test --locked --workspace --lib synthetic_candidate_and_occupancy_profile -- --ignored --nocapture
```

每项取 7 组、每组 20 次的平均耗时中位数。计时仅包含候选查询或单次放置校验，不包含地图准备、文档写入、整笔连刷和渲染；缓存首次建立的成本仍然存在。

## UI 交互录像

补全与表单快捷键的 `egui_kittest` 测试包含控件名称检查和缺字检查。安装 `ffmpeg` 并加入 PATH 后，可选择录制补全交互过程，MP4 写入 `target/ui-tests/recordings/`：

```powershell
$env:KITTEST_RECORD = '1'
cargo test --locked -p ra2-map-app --features ui-test-video completion::tests -- --test-threads=1
Remove-Item Env:KITTEST_RECORD
```

## 真实资源验证

框架材质与六环境地图数据校验、普通 / 框架 PNG 对照，以及共享深度的 GPU / CPU 像素对照：

```powershell
cargo run --locked -p ra2-map-scene --example framework_audit -- 'D:/Games/YR' target/framework-audit
cargo run --locked -p ra2-map-render --example framework_gpu_audit -- 'D:/Games/YR' target/framework-audit/TEMPERATE.map target/framework-gpu-audit
```

审计检查框架材质的透明覆盖和几何、视图切换不修改地图，以及框架、光照和换色组合的 CPU / GPU 一致性。本机有 `marble.mix` 时也会报告各环境兼容的 FA2 原生框架模板数量。

悬崖编辑刷新审计对比 80×80、256×256 地图的缓存更新与完整重载耗时，覆盖放置、撤销和重做，并检查普通 / 框架像素、地形升高后的单位位置及旧 GPU 场景仍可使用。报告中的耗时包含场景构建和 GPU 准备，不包含 UI 调度及屏幕呈现：

```powershell
cargo run --locked -p ra2-map-render --example cliff_refresh_audit -- 'D:/Games/YR' target/cliff-refresh-audit
```

桥梁审计覆盖各环境的双方向混凝土桥与木桥、短跨度、临水悬崖和低桥，检查桥头两侧保持原有悬崖高度、端点吸附、保存重开及撤销重做，并导出地图、PNG 与报告。包含新城市环境木桥模板排列不同的情况；不支持悬崖的环境检查拒绝放置：

```powershell
cargo run --locked -p ra2-map-scene --example bridge_audit -- 'D:/Games/YR' target/bridge-audit
```

保留悬崖填高地和完整悬崖升降包含在 `authoring_audit` 中。`height-fill-before` / `height-fill-after` 输出同一崖线填高前后的地图和 PNG；审计还检查崖顶高度拾取、填充未跨过原边界、已有崖壁未改动、完整模板升高后的保存重开与撤销重做。可附加环境名称只检查该环境：

```powershell
cargo run --locked --offline -p ra2-map-scene --example authoring_audit -- 'D:/Games/YR' target/height-authoring-audit SNOW --height-only
```

该工具填充起点所在的同高度连通平地，遇到地图边缘或需改高的对象 / 覆盖物占地时整笔拒绝；不承诺自动修复原本破损的悬崖，也不代替游戏内通行验收。

`--height-only` 仅运行高度专项，省略则继续执行完整创作审计。`subtile-cliff-before-repair` / `subtile-cliff-repaired` / `subtile-cliff-filled` 复现旧子块画笔所有悬崖子块等高的状态，检查只修正子块高度后再填高内部；保留三阶段地图和 PNG。合成回归另覆盖零层地面、单独放置的不完整模板、重复涂刷不叠高、画笔预览与提交一致、修正时保留图块与冰字节、占用和高度越界拒绝。

环境参数 `ALL` 配合 `--height-only` 可检查全部环境。专项还逐一取样每个标准悬崖模板的每个有效子块，验证只含低侧的模板也返回正确崖顶；`cliff-height-census.json` 记录各模板的原始相对高度。库回归另检查平整保留崖壁高度、修正工具参数独立、单格 / 多格子块预览坐标、异常元数据拒绝，以及文档编辑和资源重载后的预览缓存更新。

FA2 风格刷平默认按下取高、拖动预览、松手整笔提交。地面升降与刷平共用 `flatten_tools` 的可塑地面表面图；普通升降按点击高度选取同高格，顶点模式按点击顶点的高度方向筛选。四 / 八方向顶点约束对应陡坡 / 普通坡面，闭合崖角的地面表面分开；Ctrl+Shift 强制塑形会允许重写结构地形，仍保护对象与覆盖物。该实现参考 FA2sp 的选格和坡面规则，未宣称逐图块结果相同；底层 `terrain_tools::height` 仍保留供其他调用方使用的游戏随机地图角点接口，界面连续地面升降不再调用该规则。

升降工具支持自定义层数和目标高度；`height_edit.rs` 缓存包含 LAT 的验证计划，预览和松手提交共用该计划。目标模式在 LAT 后核查全部可编辑选中格，避免跳过的高格意外降低；冲突时写入前拒绝。普通悬崖点击直接进入完整模板路径，Ctrl 八方向扩展相连同组悬崖；顶点和强制模式使用表面图。`fill_edit.rs` 支持矩形与 Ctrl 同类地表连通填充，缓存包含文档编辑版本和资源身份。放置高度、Alt 线段回撤、Ctrl+Shift 关闭 LAT 和刷平模式方向键临时升降均使用相同的文档事务；滚轮保持缩放地图。

真实资源 `authoring_audit -- ... ALL --height-only` 输出各环境的地图 / PNG、保存重开、引擎重载和撤销重做结果。除旧高度专项外，新增 `fa2-common-steep` / `vertex` / `force` / `ctrl-cliff-group` / `subtile-fixed-base` / `ctrl-ground-fill` 等用例。`fa2-flatten-drag-from-cliff` / `fa2-flatten-complete-interior` 继续检查从崖顶拖入内部和完整刷平。库回归另覆盖修饰键捕获、Alt 回撤、残缺模板拒绝、崖顶上限、背面负基准、方向键改高、滚轮缩放不改地图的真实画布事件；日志保存到 `target/fa2-common-cliff-tests.log` 和 `target/fa2-common-cliff-input-test.log`。这些检查证明本项目结果可保存、可撤销，不构成 FA2 双 GUI 同图对照。

体素适配直接编译固定版本引擎的 GPU 光栅器和 WGSL，在资源线程按最多 32 个朝向批量生成静态图，每批只提交、读回一次，供预览、拾取与地图图集共享。`UseBuffer` 等上游不支持的 GPU 路径直接复用 CPU 结果；车体、炮塔、炮管相交合成需要几何深度，因此整体走一次 CPU 路径，避免 GPU 绘制后再完整执行 CPU 光栅化。

使用真实资源验证 GPU 的 32 朝向像素、合成深度和 CPU 回退（测试会要求实际 GPU 执行，不能以回退代替）：

```powershell
$env:RA2_DIR = 'D:/Games/YR'
cargo test --locked -p ra2-map-assets --lib retail_voxel_gpu -- --ignored
```

建筑 SHP 炮塔在地图场景中保留独立绘制层：`TurretAnimYSort` 使用引擎的 lepton 排序键，`TurretAnimZAdjust` 与动画 ART 的 Y 偏移共同进入像素深度；建筑主体继续使用自身的深度形状。VXL 炮塔和炮管分别保留自身的裁剪框及深度起点，不合入建筑深度。SHP 物理帧为 32 向帧表值加 ART `Start`；`ShouldUseCellDrawer`、`AltPalette` 和 `UseNormalLight` 控制选色及换色。透明度按引擎选择 25/50/75% 档位，CPU 和 GPU 都进行 RGB565 掩码混色；GPU 在每个透明绘制项前读取已有画面，重叠透明炮塔按顺序累积。`Shadow=yes` 的非 Flat 炮塔使用所选物理帧加 SHP 总帧数的一半绘制阴影，保持独立裁剪，阴影不参与对象拾取。

缩略图、选择边界与放置提示保留合成图；放置/粘贴根据目标格的正负投影选取预解码的坐标相位，避免原始像素偏移与引擎 lepton 换算出现一像素差异。主体和炮塔均补偿 15 级以上地形的投影高度。CPU 回归覆盖新建地图有效起点高度区间、起始帧、选色/透明/阴影、正负排序、主体及邻近建筑/车辆遮挡、36 级高程、Flat 动画、地图两侧的偏移和移动后的撤销/重做。以下检查另外要求实际 GPU，只用合成资源，对比地图内规则覆盖后的画布像素、拾取与图片导出：

```powershell
cargo test --locked -p ra2-map-render --lib shp_turret_depth_and_y_sort_match_gpu_picking_and_export -- --ignored --test-threads=1
```

截图与报告写入 `target/shp-turret-depth-gpu/`，含 VXL 深度、起始帧、0/4/36 级高度、三档透明、重叠透明炮塔与独立阴影。该检查验证编辑器各路径一致及与引擎坐标/混色/深度函数的对应关系，不代表已完成原版游戏整屏像素对照。受损外观及其他建筑动画的完整独立排序仍属于既有静态场景范围。

传统预览的真实资源检查覆盖取景范围、起点及 PreviewPack 像素往返；高清导出检查会启动支持 `--map-picture` 的引擎并实际使用 GPU：

```powershell
cargo run --locked -p ra2-map-io --example radar_audit -- 'D:/Games/YR' target/radar-audit 'D:/Maps/test.map'
$env:RA2_DIR = 'D:/Games/YR'
$env:VERA20K_EXE = 'D:/Games/VERA20k/vera20k.exe'
cargo test --locked -p ra2-map-io --lib retail_engine_hd_preview_exports_the_current_snapshot -- --ignored
```

窗口测试需要本机游戏目录；地图、截图与报告只写入本地 `target/`。

```powershell
# 编辑、撤销 / 重做、恢复、保存、重开与截图
cargo run --locked -- --p2-smoke-test --game-dir 'D:/Games/YR' --new-map 80x80

# 随机地图真实资源验证，输出地图、预览图和报告
cargo run --locked -p ra2-map-scene --example random_map_audit -- 'D:/Games/YR' target/random-map-audit
```

随机地图审计还对比 2 / 30 人内陆湖泊和山地的区域大小 0 / 100，检查参数保存、地形变化和同种子复现；群岛覆盖 2 / 9 / 30 人的科技建筑开关，检查中立归属，并用固定的 2 人样本验证实际生成。扩展群岛可能没有起点空闲岛屿，不能要求每张图都有科技建筑；无起点区域的放置、占地和禁用时随机流另由合成回归覆盖。

窗口烟测的其他入口和输出目录参数见 `cargo run --locked -- --help`。格式校验和静态预览不能替代游戏运行时验证，当前功能边界见 [README](../README.md#当前范围)。

## AssemblyScript Wasm 任务验证

需要 Node.js >=22.15（含 npm）。以下命令在编辑器仓库根目录安装依赖并验证构建器；用户扩展项目需在各自根目录执行 `npm install`。编辑器不附带 Node.js 或编译器，构建时使用系统 Node.js 调用扩展项目内的 `@ra2-map-editor/mission-wasm-build`。

```powershell
npm ci
npm test --prefix crates/mission-wasm-build
cargo test --locked --offline -p ra2-map-io mission_project -- --test-threads=1
cargo test --locked -p ra2-map-app wasm_mission_ui -- --test-threads=1
node crates/mission-wasm-build/project-build.mjs crates/mission-wasm-build/engine-check/fixture target/assemblyscript-engine-check.wasm
cargo run --locked -- wasm-check target/assemblyscript-engine-check.wasm
cargo run --locked --manifest-path crates/mission-wasm-build/engine-check/Cargo.toml -- target/assemblyscript-engine-check.wasm --frames
# UI 模块分别检查客户端 ABI 和生产预算下的生命周期
node crates/mission-wasm-build/project-build.mjs crates/mission-wasm-build/engine-check/ui-fixture target/assemblyscript-ui-engine-check.wasm
cargo run --locked -- wasm-check --kind ui target/assemblyscript-ui-engine-check.wasm
cargo run --locked -p ra2-map-io --example wasm_ui_smoke -- target/assemblyscript-ui-engine-check.wasm --expect-log "ui lifecycle start=1 render=600 events=2 frame=599"
```

默认脚手架创建联合工程：根目录仅有一份 npm 依赖，`rules/` 与 `ui/` 使用独立 SDK 配置和入口，根 `package.json` 通过 `"vera": { "kind": "extension" }` 标识项目。根构建命令在临时目录完成两种产物后再发布；编辑器 `wasm-build` 还使用两个实际引擎 loader 校验 ABI，并在配置漂移时拒绝发布。地图中的“构建并嵌入”把两种模块合并为一次可撤销事务，成功后分别建立文件监听。单模块项目按 `vera.json` 构建对应模块。

任务项目使用 `@ra2-map-editor/mission-sdk@0.1.3` 和 `@ra2-map-editor/mission-wasm-build@0.1.3`，上游引擎 SDK 为 `@vera20k/sdk@0.1.0`。仓库使用根目录 npm workspace 和统一锁文件，包通过 `npm pack` 生成。`project-build.mjs` 调用包内 `vera20k build`，只在临时编译目录处理模块解析和计时器生命周期适配，不修改引擎源码或已安装包。引擎验收沿用生产导入白名单、预算及内存限制，不在测试时放宽。UI 验收运行真实 `ClientModule`，按启动、事件、渲染顺序执行 600 帧并核对回调计数；probe 只提供日志和帧号，不替代游戏中的节点绘制、玩家视野、网络消息或存储验证。

## 引擎版本管理

当前固定的 VERA20k 版本、提交与接入说明见 [vendor/README](../vendor/README.md)。引擎和编辑器均不再提供 Ares 编译开关。原生任务 schema 支持 Ares，运行时遵从地图资源设置。

地图资源的 Auto / On / Off 传给资源加载和试玩。INI、include、扩展 CSF 跟随 AssetManager::ships_ares()，文档及后台操作保留打开时的解析选择。未选择资源叠加时，Auto 忽略仅安装在游戏根目录的 ares.mix。

静态车辆预览支持引擎已解析的 Ares `WaterImage`：水格且非桥上时选择替代体素，SHP 车辆保持原显示分派。体素几何来自替代类型，`UseBuffer` 来自实际类型；画布、像素拾取、导出、选择边界与粘贴提示共享选择。普通地面、桥上与无效引用保留原模型。此路径沿用编辑器已有的静态朝向和普通炮塔范围，不模拟卸矿、伪装、部署或索引炮塔的动态选择。

水面模型的 `TurretOffset` 取自生产 Rules owner 的替代类型 ART 段，文件名仍使用该类型的规则 `Image`。场景重建使用同一份覆盖物注册表，因此低桥等 `Land=Road` 覆盖物会改变模型选择。粘贴提示按即将写入的地形和覆盖物选择模型；水面复制到陆地、陆地复制到水面及同时粘贴桥面都有与提交结果对照的回归。

地图可在原文规则段设置 `SuperWeaponPerBuilding=yes` 与 `BuildLimit`，由共享引擎解析并在试玩中执行；它们是建筑类型规则，不是 `[Basic]` 或多人对话框字段，编辑器不把客户端的数量滑块写进地图。详见引擎[规则说明](../vendor/vera20k/docs/superweapon-limit.md)。

YR 与 Ares 使用相同构建；自动化测试通过运行时模式覆盖两种 INI 方言和试玩设置。INIs 的 include、继承、迭代键和覆盖策略由引擎解析器拥有。编辑器不重写这些规则；带继承/迭代键的地图保留只读预览和原文导出。常规物理键值地图按引擎选择有效行，空字段仍保留作为任务构建的显式清空请求。

编辑器使用 `vendor/vera20k` 子模块固定的引擎提交，切换编辑器版本后运行：

```powershell
git submodule update --init --recursive
```

引擎是独立 workspace；子模块提交与 `Cargo.lock` 共同构成可复现构建输入。初始化、恢复和升级的完整流程见 [引擎依赖说明](../vendor/README.md)。
