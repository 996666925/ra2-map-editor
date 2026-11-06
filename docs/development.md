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

体素适配直接编译固定版本引擎的 GPU 光栅器和 WGSL，在资源线程按最多 32 个朝向批量生成静态图，每批只提交、读回一次，供预览、拾取与地图图集共享。`UseBuffer` 等上游不支持的 GPU 路径直接复用 CPU 结果；车体、炮塔、炮管相交合成需要几何深度，因此整体走一次 CPU 路径，避免 GPU 绘制后再完整执行 CPU 光栅化。

使用真实资源验证 GPU 的 32 朝向像素、合成深度和 CPU 回退（测试会要求实际 GPU 执行，不能以回退代替）：

```powershell
$env:RA2_DIR = 'D:/Games/YR'
cargo test --locked -p ra2-map-assets --lib retail_voxel_gpu -- --ignored
```

窗口测试需要本机游戏目录；地图、截图与报告只写入本地 `target/`。

```powershell
# 编辑、撤销 / 重做、恢复、保存、重开与截图
cargo run --locked -- --p2-smoke-test --game-dir 'D:/Games/YR' --new-map 80x80

# 随机地图真实资源验证，输出地图、预览图和报告
cargo run --locked -p ra2-map-scene --example random_map_audit -- 'D:/Games/YR' target/random-map-audit
```

窗口烟测的其他入口和输出目录参数见 `cargo run --locked -- --help`。格式校验和静态预览不能替代游戏运行时验证，当前功能边界见 [README](../README.md#当前范围)。

TS SDK 的逻辑整链与呈现输入可分别生成；输出目录须不存在：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_audit -- 'D:/Games/YR' target/new-sdk-audit
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' target/new-sdk-presentation
```

House 对象生命周期可单独验收：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_object_lifecycle_audit -- 'D:/Games/YR' target/new-sdk-object-lifecycle
```

该审计通过 TS 构建和普通帧验证建筑接管/归还、四类对象整体转移/摧毁、仅建筑及全部开局对象恢复，并检查其他 House 对象未受影响。输出 `report.json`，任一检查失败返回非零退出码；不需要修改引擎。

小队行为审计使用真实资源、TS 招募及普通帧，核对 Power 类别攻击、展开、散开、转向、恐慌/取消、计时与永久等待、持续 Mission 及转国后的 Team 生命周期：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_team_behavior_audit -- 'D:/Games/YR' target/new-sdk-team-behavior
```

报告包含逐行帧号、恐惧值、朝向、位置与转国时的 Team 所属方。只读观察现有存档状态，不为审计增加引擎接口；不代表 GUI 呈现验收。

House状态条件使用以下独立普通帧审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_house_conditions_audit -- D:/rc2/yl target/new-sdk-house-conditions
```

三份强类型TS任务、五组运行覆盖真实木箱拾取后的资金等值阈值、杀死/移除载具的损失差异、工厂摧毁/重建、建筑active计数的重新评估、最近建筑记录锁存及创建发电厂后的缺电恢复。House顺序与Country不同，分别核对规则owner和参数Country；另对比无House owner的单独规则与含逻辑计时条件的规则。active计数和最近类型通过现有Serialize存档表示只读选择，不新增引擎接口或运行时状态注入。

变化组各含第40帧headless快照恢复，立即核对列明状态，后续比较消息名称顺序和最终House/对象结果。新建对象的ID不作连续/恢复相等要求，原始对象身份仍逐项保留；完整逐帧及回调状态写入报告。现有恢复重置场景随机流，移动/拾取帧及瞬态分配序号可能改变，另报告 `continuous_ticks_and_callback_states_match`。输出目录须不存在；这不代表GUI加载、多人网络、空矿仓资金语义或全部模拟内部状态已验收。

四类最近建造事件另有真实生产审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_built_events_audit -- D:/rc2/yl target/new-sdk-built-events
```

六份强类型TS任务、十一组运行覆盖车辆MTNK→HTK、步兵E1→ENGINEER、飞机ORCA→BEAG、建筑GAPOWR→NAHAND，以及车辆取消和空闲对照。地图只声明生产所需Owner/Prerequisite/TechLevel、400/600成本和BuildSpeed=0.1；通过正常编码PRODUCE逐步扣费，移动产品由正常发布器生成PLACE，建筑使用正常编码PLACE。headless宿主通过已有 `FrameEffects.with_production_strip` 提供单项Factory链接，不调用测试完成器或直接写生产进度；这并不验证GUI侧栏保留链接和点击行为。

报告跟踪生产开始时原始limbo实体、付费进度、成功交付、House真实类型序号、计分记录、错误owner阴性、首类型匹配后等待全局条件的锁存，以及车辆出厂后的普通Move。两次车辆生产留出出口动画时间；原车移走后再生产下一辆。建筑组等待首座建筑完成正常Construction且施工场联系释放，再生产第二项，不跳过建造动画。四类生产和取消组各在第80帧付费未完成时headless快照恢复，立即比较列明Factory/历史/经济/实体状态，之后核对消息顺序、原始产品ID及最终列明结果；完整时刻和回调状态差异另报。输出目录须不存在，不代替GUI存档、多人网络或所有内部状态等价验收。

任务 README 的代码与完整示例使用以下审计：

```powershell
$readmeAudit = 'D:/rc2/new-sdk-readme'
cargo run --locked --offline -p ra2-map-io --example mission_sdk_readme_audit -- 'D:/rc2/Command & Conquer Red Alert II' $readmeAudit
$readmeTs = @((Get-ChildItem -LiteralPath $readmeAudit -Recurse -Filter mission.ts).FullName)
$readmeTs += Join-Path $readmeAudit 'campaign/campaign.ts'
node target/ts-bootstrap/package/lib/tsc.js --noEmit --strict --exactOptionalPropertyTypes --noUncheckedIndexedAccess --isolatedModules --target ES2015 --module commonjs $readmeTs
```

工具要求新输出目录，使用32×64地图、Americans/Russians、0/1/20/21/22航点、建筑/工程师及手工Script/TaskForce作为明确前置。资源目录的静态MIX只读硬链接，松散文件复制；仅在隔离安装的CSF中增加三个说明文本，供已有标签示例使用。所有枚举仍由实际场景目录生成，不把参数降为字符串或any。当前43个TS代码块各保留原文、README行号和转换后的入口，片段只补导入、defineMission回调和文中省略的前置引用；小队orders属性片段放回真实team参数中。

47个类型检查入口包含43个README代码块、两关任务与战役入口及一个同国House反例。TypeScript仅用于这项外部验收，产品构建与运行仍不依赖Node或tsc。四份完整单人/多人任务另走真实Worker，检查无变化重建、精确撤销/重做及手工定义保持；故意把新House改为已占用Country的任务通过TS类型检查，但必须被Rust归属校验拒绝，失败构建不改地图。

README中的战役入口结合仓库两关任务源码和上述真实地图，经现有战役构建、资源审计及发布；`campaign-report.json`须通过并保留完整发布包。43个片段的类型检查不代表都经过地图几何/外部媒体审计，更不代表每个片段都在客户端运行；需要放入actions/orders的裸表达式也不会因类型检查而自动成为游戏行为。实际运行、GPU和音频验收仍以各项专项报告为准。

随机等待、重新启用及当前环境亮度阈值使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_clock_ambient_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-clock-ambient
```

八份强类型 TS 任务分别覆盖 randomTime 的0/1/4秒、初始停用→启用→中断→重新启用、初始亮度94/95/96与阈值95，以及由100降至80再升至110。每份执行连续运行和两次独立第20帧公共 headless 快照恢复，各观察0～400帧。已有存档字段仅用于读取实际计时锚点、时长和启用状态，未注入计时器、随机数、通知或光照状态。输出目录须不存在。

随机组核对所有抽样的闭区间/15帧单位、每次实际触发前到期、重抽锚点、0秒逐帧触发、1/4秒不同抽样、停用后不触发和重新启用重置。帧末receipt的tick比动作执行时的binary_frame大1，报告按真实计时字段核对；当前计时恢复后不变，但现有快照读取会重置场景随机流，后续新抽样与连续运行可不同，完整差异另报。两次独立恢复的全部列明观察及消息仍须一致。

亮度组检查两个比较都包含等值、错误方向不触发、条件不修改光照；一次性Tag移除可能使另一个同时成立条件晚一帧轮询，不以同帧消息作为接口承诺。渐变组核对目标先变化、当前逐步达到90/80/110后才收到消息，且逻辑先轮询再渐变，因此通知在到达阈值的下一帧出现。快照位于当前95/目标80的待完成阶段，恢复即时状态及完整列明观察应与连续运行相等。本项不替代GUI加载、GPU亮度、网络或全部模拟内部状态验收。

普通增援的 `full` 预装与矿仓填充使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_full_reinforcement_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-full-reinforcement
```

十八份强类型 TS 任务分别以 `full: false/true` 覆盖 HTK、两栖 LCRF、原版零载客容量 PDPLANE、地图显式声明 `Passengers=2` 的飞机对照、SHAD、LCRF/HTK 混编、多辆 HTK、超容量输入与 HARV。全部从 TS 触发普通增援，依次记录 0～180 帧，每份另在第 32 帧通过公共 headless 快照恢复，共三十六组。输出目录须不存在。

报告检查全部 TaskForce 成员实际出生、正确原始载荷进入 limbo 且仍保留在 Team、选中的运输成员和完整载荷链、两辆运输车不均分的分支、矿车 40 单位槽 0 填充与 House 资金不变，以及原始身份/所属/生命/无损失。全程检查载荷链及矿仓保持，恢复后列明的完整逐帧观察与连续运行一致。原版 PDPLANE 不预装是保留的阴性对照；显式容量飞机只验证地图变体。超容量输入用于核对创建路径不走普通上车容量门槛，创作时仍应配置合适成员数量；其他正数 Storage 类型可能被当前引擎表示拒绝。LCRF 在陆地使用其两栖能力，本项不替代水面/空中运输行程、卸载、GUI 存档、网络或全部内部状态等价验收。没有修改引擎或直接写入对象、乘客、矿仓、成员及计时状态。

运输行程与卸载模式分别使用以下独立审计。

两栖运输的实际水面行程另用以下审计，要求新输出目录：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_water_transport_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-water-transport
```

三份强类型 TS 任务分别招募原始 LCRF、SAPC、YHVR 和 12 名 ENGINEER，不覆盖类型容量；水域及完整海岸通过编辑器现有 shore_tools 生成并写入地图。真实战役加载器解析地形，普通帧执行装船、满载等待、水中航点、对岸航点、KeepAll 卸载及后续标记。每份连续运行 2400 帧，并另做两次独立水中公共 headless 快照恢复，保存完整逐帧列明状态。

必须通过满载穿越真实 Water 格、乘员在水中始终处于载荷内、对岸全员下船、KeepAll 精确重入及原始身份/所属/生命保持；两次独立恢复必须可重复。连续运行与恢复的精确时序和最终落脚格另报：当前样本卸载相差 2 帧、两名乘员最终格不同，不能将成功到岸写成逐帧等价。此项不替代 GUI 加载、联机或原版 SAV 验收，不修改引擎来通过对照。

陆地运输的卸载模式与返程审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_transport_modes_audit -- D:/rc2/yl target/new-sdk-transport-modes
```

八份强类型TS任务、十六组运行覆盖四种UnloadMode、满载两座HTK的KeepAll/ReleaseAll返程开关，以及三座HTK装两人、跳过waitLoaded的KeepAll/ReleaseAll返程对照。任务通过TS招募地图原始HTK和两名ENGINEER，正常Load/WaitLoaded/Move/Unload；检查实际载客移动、乘客limbo及离队/重入、卸载格、保留成员集合、空小队清理和后续脚本是否执行，全程保持原始身份/归属/健康及旁观建筑，无击杀损失。

返程开关使用正常Load留下的ArchiveTarget：仍有空位且没有待上车成员的分支记录原位置；满载分支提前结束而不记录位置，后续卸载虽离队却无法返回起点。报告同时核对这两个分支、自动返程到达原位置和返程记录清除，不直接注入位置、乘客或成员列表。每组在实际装载完成后经公共headless快照恢复，立即比较列明状态，最终严格比较原始对象坐标及Team成员/脚本状态；完整阶段时刻/状态差异另报，恢复可能使下车时刻偏移。不代替GUI存档、飞机/海军运输或全部内部状态相等验收，输出目录须不存在。

建筑类型目标、区域威胁选择和实际移动/攻击使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_building_targets_audit -- D:/rc2/yl target/new-sdk-building-targets
```

十三份强类型TS任务各执行连续运行和第100帧公共headless快照恢复，共二十六组。己方/敌方建筑移动分别覆盖四种BuildingSelection，另有两种类型不存在的移动、按类型攻击、航点建筑攻击及空航点对照。地图放置两座同类型建筑，远端站立敌军正常发布区域威胁；仅通过既有Serialize表示读取House威胁，检查远端威胁确实更高。威胁步兵移开接近路线、测试电厂声明Crewed=no，避免碾压和随机逃生兵干扰身份与最终坐标比较。

报告核对选中的建筑对象或接近格、实际到达、武器伤害、后续行、原始成员/归属/健康及旁观对象。快照时非空目标组正在移动或攻击，立即比较列明对象与Team状态；最终状态严格比较并另列全过程选择/完成时刻差异。不增加引擎接口，不注入目标、威胁、伤害、导航或成员；不代替指定敌人/盟友被动国家筛选、GUI存档、多人网络或全部内部状态验收。输出目录须不存在。

基地集结、返回基地与跟随友军使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_gather_follow_audit -- D:/rc2/yl target/new-sdk-gather-follow
```

十三份强类型TS任务各执行连续运行和第120帧公共headless快照恢复，共二十六组。通过TS正常SetAlternateBase动作声明基地、MakeEnemy动作建立指定敌人，覆盖双方基地集结、无指定敌人、无己方/敌方基地、随机方向返回、静止步兵/移动载具跟随、较近步兵胜过较远载具，以及单向外交准入和无候选等待。规则AISafeDistance在地图声明为8，不直接写基地、敌人、目标或位置；跟随目标只接受一次正常编码Move命令。

报告核对实际House基地/敌人/双向同盟状态、选择的Cell或Object、目标移动时的跟随、真实到达或等待、后续脚本行、原始身份/类型/归属/健康和旁观对象坐标；快照立即及最终严格比较列明对象、完整Team序列化表示、House状态和标记，完整阶段时刻差异另列。返回基地缺少己方基地的对照保留原生(0,0)目标且不推进，无候选跟随也不推进；无敌人或敌方基地的敌方集结则会推进。不能用存活建筑存在替代可用基地中心的前置检查，也不代替隧道、攻击镜像、GUI存档、多人网络或全部内部状态验收。输出目录须不存在。

小队超时空的建筑目标与类别目标使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_team_chrono_audit -- D:/rc2/yl target/new-sdk-team-chrono
```

十一份强类型TS任务各执行连续运行与公共headless快照恢复，共二十二组。覆盖建筑最近/最远、缺少类型、没有超武、充能远离/临近就绪、已就绪缺电及创建电厂后恢复发射，另有Power类别的有武器/无武器和不存在Factories类别对照。武器对照使用地图声明的SDKChronoProbe零伤害武器，不猜原安装的武器名；无武器Power搜索仍可成功，按实际行为记录。

所有发射来自TS招募的原始两辆MTNK、小队Script56/57和TS授予/充能动作；观察同一帧的ChronoSphere→ChronoWarp、正确源区和建筑中心格、实际Teleport锁存/落地/原运动器恢复、电力恢复和后续行。只读已有序列化表示；不写准备状态、电力、目标、位置或运动器。发射组在两名成员真实传送锁存时恢复，其余组在第120帧恢复；立即和最终严格比较列明对象/完整Team/超武/电力/放置动画/标记，全过程时刻/阶段差异另列。不代替正常GUI存档、多人网络、全部内部状态或原版像素一致，输出目录须不存在。

小队空闲/附着动画和紧急出售使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_team_animation_strategy_audit -- D:/rc2/yl target/new-sdk-team-animation-strategy
```

十四份强类型TS任务各执行连续运行与公共headless快照恢复，共二十八组。Idle1/Idle2读取真实Doing与Stage推进，MTNK作为空接收者对照；附着动画覆盖两名原始MTNK、EXPLOSML/CHRONOSK、构造参数0/1/3/256、真实移动和自然结束，并核对绑定对象、绝对坐标、绘制标志和跨参数的实际生命周期。每份报告另存当前安装的动画循环/帧数/速率前置，不猜测ART默认值。

紧急出售覆盖Script30的AI/真人House、Action9的AI和不调用的AI。报告核对状态4、未重置策略时钟、到期才出售、三份正常出售通知、三座原始建筑退休、精确退款、原始MTNK离队Hunt、Team清理及无损失。共用审计加载器现与生产headless入口一致，绑定建筑建造/废墟资源；不新增引擎接口或直接写策略、动作、计时器、动画或退款。

恢复点位于实际Idle/附着动画执行时或紧急状态锁存后，立即比较全部列明字段。最终结果比较只排除House的strategy_timer和对象stage.timer，原始身份、归属、健康、坐标、动作、导航、Team、动画、资金、损失与标记仍比较；完整时钟值、全字段最终比较和阶段比较原样保留，避免把现有恢复重置随机流后的未来时钟差异当成全程等价。可在命令末尾追加单个夹具名，例如 `sale_script_ai`。输出目录须不存在，不代替GUI像素、音频、网络或全部内部状态验收。

侦察、成功标记和小队结束反馈使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_scout_success_audit -- D:/rc2/yl target/new-sdk-scout-success
```

九份强类型TS任务各执行连续运行与公共headless快照恢复，共十八组。覆盖当前玩家/外国小队的迷雾差异、局部/整图揭示、首次选点保持及实际移动，以及成功后永久等待、脚本末尾、重复成功、未成功末尾和显式解散。两份关闭的AITrigger分别以原小队为primary/secondary，核对只在Team销毁时记录一次主队结果，等待期间和副队引用不计数。

报告通过已有公开视野查询和Serialize表示只读观察迷雾、完整Team及AITrigger计数，无视野/目标/随机流/位置/统计注入；核对原始身份、归属、健康、实际到达、导航释放、旁观建筑和无玩家胜负。侦察组在目标首次选定时恢复，其余组在第100帧成功锁存或未成功等待期间恢复；立即和最终严格比较列明状态，另列全过程阶段/时刻比较。不代替任意地形可达性、全部随机结果、正常GUI存档、多人网络或所有内部状态验收，输出目录须不存在。

国家消灭条件及运输统计使用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_destroyed_conditions_audit -- D:/rc2/yl target/new-sdk-destroyed-conditions
```

六份强类型TS任务覆盖四类对象分阶段摧毁、只剩Aircraft仍满足单位/全部消灭、两栖海军运输工具LCRF及仅将其Passengers声明为0的计数对照、两名ENGINEER进入盟友HTK和同布局不装载对照。LCRF使用其正常两栖能力在陆地活动；零容量变体只验证计数分类，不代替原版战斗舰艇水面行为。规则owner与条件Country故意不同，House顺序也与Country索引不同。

装卸使用普通编码Enter/Unload命令，命令发送House经TS配置 `playerControl:true`，否则原生DoList不会执行该House的玩家命令。报告分别检查命令准入、真实乘客数、原始身份/健康/归属、limbo、在场和跟踪计数、通知次序、卸载后组合条件重新评估及旁观House不变。计数从现有Serialize存档表示只读读取；没有注入计数、乘客、生命周期或事件，也没有新增引擎接口。

五组含headless快照恢复：四组在40帧恢复，装载组等两名真实乘客全部上车后恢复。立即比较列明状态，随后严格比较最终House与原始对象身份/健康/坐标和通知名称顺序；回调时刻/列明状态另报。可在命令末尾追加单个夹具名（例如 `allied_transport`）定向复跑。输出目录须不存在，不代替GUI存档、海战、多人网络或全部模拟内部状态验收。

小队离场通知有独立普通帧审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_team_left_map_audit -- D:/rc2/yl target/new-sdk-team-left-map
```

六份TS任务、九组运行覆盖两架PDPLANE的真实Retreat及同型等待对照、步兵/载具成员移除、直接解散和步兵持续等待。检查原始成员身份、健康、归属、损失、最后一名成员清空前不通知，以及已有owner的TeamType筛选和无House owner的绕过行为。离场标记通过现有Team存档表示只读观察，不新增引擎接口。飞行动作在同一次对象访问中越界并退休，帧末无法采到存活的界外对象；报告保留实际帧末几何观察，不声称取得退休瞬间坐标。

飞机离场、步兵移除、直接解散各含第40帧headless快照恢复，检查即时列明状态和后续通知顺序/最终结果。现有场景随机流在读取快照后重置，飞机返航路线和离场时刻可变化，另以 `continuous_ticks_match` 报告；不代表GUI存档或所有内部状态相等。输出目录须不存在，失败返回非零退出码。

驻军审计验证 `enterBattleBunker` / `enterNearestOccupiable` 的实际进驻，以及共享 Tag 首个匹配建筑的释放和重复释放；报告保留驻军列表变化与动作执行帧：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_garrison_audit -- 'D:/Games/YR' target/new-sdk-garrison
```

特殊建筑命令另有真实资源普通帧审计，覆盖 Grinder 退款/消耗、坦克碉堡双向链接和生化反应炉 Cargo：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_special_building_entry_audit -- 'D:/Games/YR' target/new-sdk-special-building-entry
```

空投审计覆盖默认边缘与指定运输航点、四名原始成员的运输/下降/落地、脚本推进及运输机离场。两种入口各执行连续运行和两次第40帧快照恢复，并比较两次恢复的1400帧实体/小队/脚本观察：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_paradrop_audit -- 'D:/Games/YR' target/new-sdk-paradrop
```

报告同时保留连续运行与恢复之间的逐帧比较。引擎现有快照读取器会重置 Scenario 随机流，运输机之后随机选择的返航格和下游帧可不同；`continuous_and_restored_observations_match=false` 不表示本次 SDK 编码失败。整体通过仍要求三次运行均完成原始成员投放、落地和脚本，恢复时机在投放前且载荷/运输关系保留，两次独立恢复的全部1400帧观察一致。此审计未改引擎，也不代表 GUI 存档入口已验收。

超武状态、超时空两次点击及风暴生命周期有独立审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_superweapon_audit -- 'D:/Games/YR' target/new-sdk-superweapon
```

审计经 TS 构建、真实战役资源和普通帧，检查充能百分比/计时覆盖与重置、目标与防御位置设置/清除、一次性授予/消耗/重新授予。超时空通过两个正常玩家命令选择源点和终点，分别连续运行及在两次点击间保存恢复，核对普通步兵的伤害、坦克/超时空兵的原始身份、偏移落点及原移动器恢复。风暴覆盖玩家发射、直接指定航点、无 House 全局风暴和单次云层，要求真实云/闪电动画及目标受伤，核对倒数、开始、停止请求与云层最终退场。全局停止时机按安装资源的真实延迟生成，未改 Rules 时长。

产物含 `report.json`、状态各阶段快照、逐帧 `observations.json` 和超时空 `between-clicks.bin`。检查只读现有公开查询/存档序列化，不扩展引擎诊断；失败返回非零退出码。此入口验证列明的模拟状态，不代表 GUI、像素/音频、全部超武或所有存档组合已验收。

`mission_sdk_effects_audit` 用地图 Rules 声明两种隐式分配的武器和持续烟雾粒子，在真实资源加载与普通帧中验证 `explosionAt` 的40/80点实际伤害、`playAnimationAt` 的动画构造/退休，以及同格粒子停止、远处继续生成、存量自然排空。粒子另运行一次中途快照恢复，立即核对完整系统/粒子序列化状态。输出包含四个强类型 TS 任务、五组运行和逐帧观察；失败返回非零退出码。

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_effects_audit -- D:/rc2/yl target/new-sdk-effects
```

输出目录须不存在。这些检查覆盖模拟状态与 headless 恢复，不代替 GPU、音频或正常 GUI 存档加载。

`mission_sdk_environment_audit` 覆盖离子冲击及运行中快照、真实核弹飞行/NUKEBALL、补齐地图规则前置的化学导弹、中心矿石减少、矿石增长开关、藤蔓/冰层/TrainCrate标志、污迹身份清除及三档C4光照请求：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_environment_audit -- D:/rc2/yl target/new-sdk-environment
cargo run --locked --offline -p ra2-map-io --example mission_sdk_environment_audit -- D:/rc2/yl target/new-sdk-environment-meteor --meteor-fixture
```

第二条额外验证陨石强度0/3、空中移动、落地伤害与退休。它在输出目录生成独立活动资源配置，复用原版归档，单独写入ART；陨石借用EXPLOSML图像并使用`IsMeteor`，验证机制而非原版陨石美术。不会修改输入游戏目录。原版安装资源的可用性另存`prerequisites.json`，缺少`ChemLauncher`或陨石素材不能计为原版资源效果通过。输出目录均须不存在；报告保留逐帧观察、伤害时序和运行边界，失败返回非零退出码。C4只核对呈现请求，不等同于GPU闪光；藤蔓/冰层只核对标志，不等同于生长机制。

`mission_sdk_audit` 验证两关战役、运输、铁幕、选择通知、脚本标签跳转/下一行、成员杀死/移除与无界面快照恢复；
Script17 同时核对目标脚本 ID 与 -1 光标，成员生命周期核对实际移除、损失及事件7/48/29的差异。

受伤通知使用独立审计，从正常编码攻击命令与TS武器爆炸动作触发，不直接写生命值或注入通知：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_damage_notifications_audit -- D:/rc2/yl target/new-sdk-damage-notifications
```

五份TS任务生成八组运行：敌我攻击各含连续/恢复，无来源60点爆炸含连续/恢复，另有240点跨两线和75点红血边界。地图规则将目标Strength固定为300、ConditionRed设25%、移速为0以避免受伤移位导致后续定点爆炸落空；武器与真实伤害接收链均由生产加载器安装。House顺序刻意与Country不同，核对 `damagedByHouse` 的实际来源House；主要观察规则处理后禁用自身，另留一个不禁用的 `latch_probe` 证明重复Tag锁存会在后续通知重入。输出逐帧位置/生命/标记、消息顺序、首伤快照及报告。快照立即检查生命/格子/归属/标记，后续核对处理一次的通知顺序和血量序列；`continuous_ticks_match` 单列攻击时刻偏移，不将报告通过解释为全部内部状态、逐帧或GUI存档相等。

格子越线、伪装身份及高空采样另用以下审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_cell_notifications_audit -- D:/rc2/yl target/new-sdk-cell-notifications
```

七份TS任务产生十组运行，覆盖整行/整列远端绑定、同Tag横线优先、SPY默认身份、真实MakeupKit攻击复制身份、非Agent对照，以及JUMPJET的自然起飞/移动/悬停。全部由普通编码攻击/移动命令推进；横线、已伪装间谍和飞行中另有公共headless快照恢复，对比通知消息的时刻、顺序及列明的成员状态。报告同时保留帧末 `tick/binary_frame` 与本轮访问前的 `visit_binary_frame`，16帧条件检查后者，避免把提交后的177帧误判为不是176帧采样。共享审计加载器按正式客户端使用 `SizeW+SizeH` 的格子数组范围；报告逐组检查该范围，防止将逻辑地图宽高误作整行/列扫描边界。这里不覆盖网络同步、GUI加载或像素呈现。

对象发现与建筑探照灯使用以下独立审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_discovery_spotlight_audit -- D:/rc2/yl target/new-sdk-discovery-spotlight
```

九份TS任务、十三组运行覆盖TS揭示/正常侦察移动/无揭示对照，以及探照灯扫描、关闭、持续旋转、跟踪、友军、离线。地图规则声明HasSpotlight和受控运动参数；目标格来自独立生产加载的初始灯点，随后在地图中正常创建单位，不写运行时位置。离线组额外声明Powered=yes，遵守发电厂的离线准入。探照灯内部状态通过现有Serialize存档表示选择读取：`support/saved_field.rs` 只选择结构体路径，跳过其他不适合JSON的复合映射键，没有新增引擎诊断接口。

三组发现任务另验证没有匹配House的Country在六个发现/消灭条件中直接满足，与已有House的阴性对照区分。两种发现路径、扫描、跟踪各含通知前headless快照恢复；报告比较列明的对象/灯状态、通知时刻及最终结果，输出逐帧观察、灯阶段状态、几何前置和快照。无GPU光束、正常GUI加载、网络或原版像素一致声明；输出目录须不存在。

木箱拾取使用以下独立审计：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_crate_pickup_audit -- D:/rc2/yl target/new-sdk-crate-pickup
```

五份TS任务各运行连续与拾取前headless快照恢复两组，覆盖普通拾取、`CrateTrigger=no`、回调转移所属、回调销毁步兵/载具。由TS创建木箱，再经正常编码Move和普通帧实际拾取，不注入通知、生命、位置或资金。安装原始木箱图像/标志/奖励前置另存 `stock-prerequisites.json`；受控地图只覆盖通知开关、木箱Money映射及4321固定金额，关闭载具武器以避免战斗干扰。报告核对回调顺序、实际Alive状态、资金接收者、覆盖物及单人保留槽位，并保留逐帧观察和拾取前快照。恢复比较列明的即时状态、通知时刻与最终结果，不扩大为全部模拟内部状态、原版跨House奖励一致或GUI存档验收；输出目录须不存在。

`mission_sdk_presentation_fixture` 只准备地图、资源安装、
字体覆盖记录、0/30/60/90 帧截图配置及 `profile-audio.json` 音频观察配置，生成成功不代表呈现已验收。使用当前子模块客户端
的隐藏窗口截图入口读取实际 GPU 输出：

```powershell
cargo build --manifest-path vendor/vera20k/Cargo.toml --locked --offline --bin vera20k --target-dir target/vera-client
$sdkClient = (Resolve-Path target/vera-client/debug/vera20k.exe).Path
$sdkContract = (Resolve-Path vendor/vera20k/src/app/diagnostics/tactical_capture/contract.v2.json).Path
$sdkFixture = (Resolve-Path target/new-sdk-presentation).Path
Push-Location $sdkFixture
try {
    foreach ($sdkTick in @(0, 30, 60, 90)) {
        & $sdkClient --tactical-capture map-observe-v1 --profile "$sdkFixture/profile-$sdkTick.json" --contract $sdkContract --output "$sdkFixture/capture-$sdkTick"
        if ($LASTEXITCODE -ne 0) { throw "截图失败：$sdkTick" }
    }
} finally { Pop-Location }
```

每个截图目录包含 `capture.json` 与 1024×768、BGRA8 格式的 `frame.bgra`。核对实际画面中的
中文消息、倒计时递减、环境变暗与镜头跳转；字体查表存在仍需检查其字形和游戏输出。
此入口使用固定诊断呈现时钟，不覆盖影片或扬声器听感。

### TS 战役开场影片的自然播放验收

开场 Intro 与小队中途播放分别验收。下面两份任务使用同一短片和静音背景音乐，仅 Intro 字段有无不同；普通窗口验收需要可见且无遮挡的交互桌面：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' D:/rc2/new-sdk-intro --intro-media
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' D:/rc2/new-sdk-intro-control --intro-media-control
$sdkFfmpeg = (Get-Command ffmpeg).Source
python tools/verify_mission_intro_media.py --client target/vera-client/debug/vera20k.exe --fixture D:/rc2/new-sdk-intro --control-fixture D:/rc2/new-sdk-intro-control --ffmpeg $sdkFfmpeg --python-deps target/sdk-audio-tools --output D:/rc2/new-sdk-intro-window
```

准备器通过真实 Worker 构建/无变化重建，核对解析后的 Basic/Intro，发布四段 UTF-16LE 字幕、静音背景和原片参考。`intro-runtime.json` 独立推进150个普通帧，检查原建筑、两条消息和逐帧倒计时；该headless加载不播放Intro，不能证明开场播放时逻辑暂停。GUI工具经单人→新战役→苏军正常按钮进入，之后不发Escape或任何游戏输入，要求自然播完再显示任务倒计时与两条规则消息；实际RGB与独立FFmpeg解码逐帧相关，音轨通过已有WASAPI时频指纹核对。严格PCM波形结果另报，不作为像素/时频检查的替代。

当前 `D:/rc2/mission-sdk-intro-media-desktop-final/report.json` 明确 `passed=false`：系统的 `Windows 默认锁屏界面` 遮挡所属窗口（先行尝试为 `Backstop Window`），正常前台激活失败，未采到实际开场画面或音轨。两份任务的构建/重建及各5项普通帧检查通过，独立解码和资源发布输入核验通过；这些结果仅完成准备与逻辑核验。需在可交互桌面上用新输出目录复跑，不能将无前台/隐藏窗口捕获冒充普通自然播放通过。

可见性、雷达、侧栏和平滑镜头使用两组独立输入，通过已有客户端入口验收，不需要新增引擎诊断。将下面的游戏目录替换为实际安装路径，`$sdkClient` 沿用上文构建的客户端：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' target/new-sdk-visibility --visibility
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' target/new-sdk-camera --camera-pan
python -B tools/verify_mission_visibility.py --client $sdkClient --fixture target/new-sdk-visibility --pan-fixture target/new-sdk-camera --contract vendor/vera20k/src/app/diagnostics/tactical_capture/contract.v2.json --output target/new-sdk-visibility-capture
```

验证工具仅依赖 Python 标准库，输出原始 GPU 捕获、PNG 预览和 `report.json`。可见性输入运行到600帧，以1024×768核对局部/区域/整图揭示、遮蔽、迷雾生长、侧栏页和选定图标闪烁、雷达中断及恢复。中断90逻辑帧后仍需等待雷达开启动画。事件回看通过独立 `settings/VERA.INI` 把正常命令附加到 J 键，测试不会修改用户配置。目标选在地图内部，避免镜头边缘约束干扰中心点断言。

镜头输入使用现有战役诊断要求的800×600和中心光标，先锁定/解锁输入，再以原生4档平移；30/60/90/150帧分别核对起点、活动中间帧、终点和返回基地。`--visibility` 准备器还经生产 headless 加载器逐帧读取已有 LocalSize/修订号，要求显示范围只在17游戏秒改变一次并保持稳定，报告为 `local-size-report.json`。这些检查不覆盖 LocalSize 的全部 GPU 裁剪效果、所有 zone 几何、原版像素一致性或 GUI 存档组合。

只有镜头输入改变时，可用 `--reuse-visibility PREVIOUS_OUTPUT` 复用完整可见性捕获；工具会核对客户端、源脚本、contract、各 profile、配置、地图和帧数据指纹。资源安装须保持不变；复用来源写入报告。失败捕获目录会保留，后续复跑使用新输出目录。

对话气泡可通过以下独立任务和对照任务验收，`$sdkClient` 沿用上文客户端：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-talk --talk-bubbles-fixture
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-talk-control --talk-bubbles-fixture-control
python -B tools/verify_mission_talk_bubbles.py --client $sdkClient --fixture target/new-sdk-talk --control target/new-sdk-talk-control --output target/new-sdk-talk-capture
```

准备器从 TS 招募原有 E1，保留有序气泡请求与原成员身份的 `talk-runtime-report.json`。`--talk-bubbles-fixture` 在输出目录生成三帧 SHP 并经 `m.asset()` 发布，控制任务发布相同资源但不请求气泡；不会改动游戏安装或引擎。`talk-resources.json` 独立记录原安装是否提供图形，省略 `-fixture` 可准备使用原图的任务，但自动像素识别器只接受测试图形。验证工具仅依赖 Python 标准库，读取12次800×600隐藏 GPU 捕获，核对三帧图形、无活动目标小队的保留语义、两种清除及阴性对照，输出原始帧和 PNG 预览。图形逐像素识别与空白/错误帧自检结合，不依赖整幅画面是否变化。地图设置 `TalkBubbleTime=60` 以保持各采样点的气泡；该结果不证明原版美术、自然墙钟超时或 GUI 存档恢复。

自然超时另用单次请求验证，不执行清除或替换。地图声明 `TalkBubbleTime=0.5`，原生读取为30个16ms桶（480ms），在46帧首见和120帧过期时核对实际气泡像素及同帧消息的宿主时间戳：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-talk-expiry --talk-bubbles-fixture-expiry
python -B tools/verify_mission_talk_bubbles.py --client $sdkClient --fixture target/new-sdk-talk-expiry --control target/new-sdk-talk-control --expiry --output target/new-sdk-talk-expiry-capture
```

定时暂停的自动恢复必须走普通宿主时钟。以下任务使用现有 `campaign-sov1-opening-realtime-unlocked` 入口，`pauseScene(2)` 应增加约1920ms墙钟间隔；对照任务保留相同逻辑标记而不暂停：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-scene-pause --scene-pause
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-scene-pause-control --scene-pause-control
python -B tools/verify_mission_scene_pause.py --client $sdkClient --fixture target/new-sdk-scene-pause --control target/new-sdk-scene-pause-control --output target/new-sdk-scene-pause-capture
```

准备器生成 `SOV01UMD.MAP`，经普通帧检查Action110的计时暂停状态。验证工具默认独立运行两对任务（可用 `--repeats 1`），检查第46/61帧标记的墙钟间隔、15帧倒计时差、原有输入锁在恢复后保留、91帧显式解锁后的正常键鼠输入与GPU输出。附加延时允许1920±256ms，用于容纳服务边界两侧呈现时间，负对照的逻辑标记间隔必须小于1秒。保留完整receipt、原始帧和报告；不将此结果扩大到嵌套暂停、音频保真、存档、暂停影片或超时空画面。

超时空画面用独立变体验收，控制任务显式调用 `chronoScreen(0)`，核对零时长不启动：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-chrono --chrono-screen
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/rc2/yl target/new-sdk-chrono-control --chrono-screen-control
python -B tools/verify_mission_chrono_screen.py --client $sdkClient --fixture target/new-sdk-chrono --control target/new-sdk-chrono-control --output target/new-sdk-chrono-capture
```

准备器的 `chrono-runtime-report.json` 记录TS动作后完整有序命令：两段各15次Draw、中间45次White、一次暂停/恢复，暂停期间16次公开重放服务输入保持游戏逻辑帧不变。GPU验证使用现有精确步进入口读取45/55/75/100帧，核对起始场景、过渡变白、白屏和保留的侧栏；这些采样在暂停开始前结束。自动恢复另用普通墙钟入口运行两次，核对151帧后续剧情标记、181帧解锁、倒计时和键鼠输入，以及正常战术画面返回；与零时长对照比较的附加服务时间须为512±256ms。没有手工向实时客户端注入恢复或服务输入。结果不扩大为原版全屏像素一致、音效保真、嵌套/重启效果或存档恢复。

雷达影片暂停使用已安装的真实 Bink 媒体，并以不暂停播放作对照。需要完整影片资源；下面目录中的 `A01_P01e.BIK` 来自 `movmd03.mix`，共45帧、15fps：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' target/new-sdk-radar-paused --radar-paused
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' target/new-sdk-radar-unpaused --radar-unpaused
python -B tools/verify_mission_radar_pause.py --client $sdkClient --fixture target/new-sdk-radar-paused --control target/new-sdk-radar-unpaused --output target/new-sdk-radar-capture
```

准备器通过TS声明并发布UTF-16LE字幕表，在影片0.2/0.9/1.6/2.3秒分别输出不同字幕；四段文本仅来自实际视频字幕回调。普通帧报告只证明Action100/117的唯一请求，实际暂停在宿主完成通讯前导、流准备和雷达开启后生效。验证工具默认两对独立实时运行，确认暂停任务的四次字幕回调保持同一逻辑帧及倒计时，而不暂停任务持续推进；影片结束后241帧剧情继续、301帧显式解锁，键鼠准入正常，雷达退出影片阶段。两者的逻辑标记墙钟间隔差须在2500～3500ms，容纳3秒影片的呈现边界。工具开启普通INFO日志并逐次保存影片准入来源、完整receipt和最终GPU帧；这不是播放期间像素匹配、影片音轨或菜单/跳过/嵌套/存档组合验收。

音频配置通过 TS 声明并打包 `soundmd.ini`、`audio.idx/audio.bag` 和非零 PCM，分别触发全局声音与航点声音。沿用上述变量及工作目录运行：

```powershell
Push-Location $sdkFixture
try {
    & $sdkClient --tactical-capture map-observe-v1 --profile "$sdkFixture/profile-audio.json" --contract $sdkContract --output "$sdkFixture/capture-audio"
    if ($LASTEXITCODE -ne 0) { throw '音频捕获失败' }
} finally { Pop-Location }
```

检查 `capture.json` 的 `observations.audio`：两项输出均应有 `submitted/started/completed`、有限且非零的 PCM、`source_ended=true`，最终 `settled=true` 且未截断。观察点为设备实际拉取的 Player 输出，在设备混音和重采样前；这不等于操作系统回录或扬声器听感。

`mission_sdk_save_audit` 可单独检查客户端存档是否包含可准入的完整资源、局部视图及战术视图，并输出倒计时和全局变量：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_save_audit -- path/to/save.bin target/new-save-report.json
```

检查器必须与写存档客户端使用一致的引擎、依赖锁定版本和相关特性。编辑器根目录与独立引擎 workspace 的 `Cargo.lock` 可能不同；出现 layout mismatch 时保留拒绝结果，使用写入方的构建环境运行同一检查器，不绕过布局检查。报告只代表存档 envelope 准入和局部视图结构检查，不代替完整 GUI 资源、对象层级和 GPU 恢复验收。

当前客户端另有战役保存/加载验收，使用 TS 同时声明持续小队、倒计时、重复规则及中途镜头平移。一个入口在平移中调用生产保存与加载 owner；另一入口从游戏菜单载入解锁存档，并通过相同的隐藏战役客户端捕获入口运行：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-camera-restore --camera-restore
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-camera-restore-menu --camera-restore-menu
python -B tools/verify_mission_save_restore.py --client $sdkClient --fixture D:/rc2/new-sdk-camera-restore --menu-fixture D:/rc2/new-sdk-camera-restore-menu --output D:/rc2/new-sdk-save-restore
```

捕获会在两个任务各自的 `logs/ra2.log` 中记录保存/读取，并保留实际大于 1 MiB 的存档、GPU帧和完整逐帧 receipt。验收比较存档点、输入锁定/解锁、光标/拖动、小队/脚本游标、倒计时、重复规则时刻、相机到达目标及侧栏像素。菜单加载组另检查加载时旧存档锁定与解锁状态的交替。连续对照使用同一脚本与普通客户端输入；两个独立启动产生的 Scenario RNG、消息墙钟期限和整幅地形像素哈希不冒充相等。存档规则摘要因加载后真实 TypeLoad 资产绑定可变化，报告保留原始摘要与公开即时字段比较，不把旧摘要差异当成地图身份错误。本路径通过客户端普通游戏菜单状态和生产加载 owner，但仍不模拟用户点击存档浏览列表，也不覆盖联机加载或原版 `.SAV` 兼容性。

生成的 `profile-save-load.json` 保留了 Ctrl+Shift+M/N 的诊断复现：第30帧保存、第60帧加载。固定引擎的 tactical capture 适配器没有等待异步加载完成，当前会在恢复检查处失败，不能当作现成的 GUI 存档验收命令。先前临时修正适配器后曾验证60→30恢复并继续到90帧、GPU 同帧一致，产物仍保留在本地；该诊断改动现已撤回，后续从正常客户端入口验收，不为测试修改引擎。

战役媒体使用同一准备器的 `--campaign-media` 变体。游戏资源必须包含实际影片；某些精简安装的 `movies01.mix` / `movies02.mix` 只有空目录，不能用于影片验收。准备器生成的 `movie-inventory.json` 记录已安装媒体的来源、容器尺寸和音轨，但容器解析成功仍需实际解码与 GPU 验收。

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/Games/YR' target/new-sdk-campaign-media --campaign-media
$sdkCampaignFixture = (Resolve-Path target/new-sdk-campaign-media).Path
Push-Location $sdkCampaignFixture
try {
    & $sdkClient --shell-capture campaign-all1-opening-tick-420 --width 800 --height 600 --cursor-x 400 --cursor-y 300 --shell-capture-timeout-seconds 60 --output "$sdkCampaignFixture/capture-420"
    if ($LASTEXITCODE -ne 0) { throw '战役影片捕获失败' }
} finally { Pop-Location }
```

此变体安装 `ALL01UMD.MAP` 并声明 Americans 开局 House，通过真实战役入口运行：开场播放 `A01_P01e`，1/2秒锁定/解锁输入，3秒请求同一雷达影片，开局请求 Drok，6秒切换 INTRO。`shell-capture` 会呈现开场影片首帧后按 Escape 跳过；它不验证开场影片完整自然播放。侧栏影片经过 EVA 前导及开启/播放/关闭流程；420帧可观察影片画面，另用新目录捕获720帧核对结束状态。音频设备和呈现耗时会影响影片在游戏帧中的位置，应同时核对 receipt 和实际图像。

`capture.json` 的 `opening` 记录 `locked_arrow_probe` / `unlocked_arrow_probe`。先前曾临时投影音乐 owner 的名称和 Player 状态，观测到 Drok→INTRO/Playing；该诊断改动已撤回，当前源码的 receipt 不含 `current.music`。既有记录不等于音乐/EVA/影片的非零 PCM 或扬声器听感。此变体生成的 tactical profiles 仍按遭遇战运行；单人媒体有模式门控，验收时须使用上述 shell 战役入口。活动字体决定中文字形，不能用媒体安装的字体替代先前的中文呈现验收。

小队Script25/26/27的媒体链可用以下两份任务及Windows回环验证。输出目录须不存在；准备器会硬链接静态MIX，建议输出与完整游戏安装位于同一盘，避免跨盘复制全部影片资源。

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-team-media --team-media
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-team-media-control --team-media-control
$env:RUST_LOG = 'info'
python -B tools/verify_mission_audio.py --team-media --client $sdkClient --fixture D:/rc2/new-sdk-team-media --output D:/rc2/new-sdk-team-media-audio --python-deps target/sdk-audio-tools
python -B tools/verify_mission_audio.py --team-media --client $sdkClient --fixture D:/rc2/new-sdk-team-media-control --output D:/rc2/new-sdk-team-media-control-audio --python-deps target/sdk-audio-tools
```

两组都经TS招募原始E1并执行声音→音乐→影片/next→全局标记。`team-media-runtime-report.json` 核对真实安装声音/音乐序号、进程ART影片目录与SDK顺序、唯一请求、脚本顺序、后续标记及原对象不变；不直接写成员或播放状态。回环工具检查440/660Hz的实际输出与先后、普通宿主时钟、脚本后续消息及正常键鼠恢复，保存PCM、receipt与运行日志。含影片组必须记录一次全屏首帧呈现，next对照必须没有该记录。现有战役截图入口会自动在首帧后发送Escape，故不能用这条入口证明三秒影片自然播完，也不比较播放期间像素或影片音轨；无须为验收修改引擎。

全屏小队影片自然播放另用普通可见客户端核验，需要 Windows 桌面保持前台且窗口无遮挡：

```powershell
python -B tools/verify_mission_movie_window.py --client $sdkClient --fixture D:/rc2/new-sdk-team-media --control-fixture D:/rc2/new-sdk-team-media-control --output D:/rc2/new-sdk-team-media-natural
```

工具只启动自己拥有的客户端，固定1000×736客户区，正常点击“单人→新战役→苏军”；此后22秒不发送键鼠，最后用窗口关闭消息退出。只捕获前台客户端区域，保存时间戳、完整PNG、四段字幕白色掩码和输入指纹。检查三秒可见影片、画面变化、四个依次出现的字幕掩码及返回游戏后的第三行消息；next对照须无影片、仍有后续消息。字幕A/B/C/D及消息文字需查看对应PNG确认，像素检测不充当OCR。窗口失去前台或尺寸改变会失败；`GetDC(hwnd)`可能返回Vulkan旧帧，工具读取屏幕上实际显示的客户区。仅临时调整任务夹具自己的`settings/VERA.INI`，结束后恢复原文件；原先不存在时创建最小设置并在结束后移除。现存设置须`[Intro] Play=no`。此核验不包含影片音轨或原版逐像素对照。

雷达影片的普通窗口画面和音轨可使用三组对照核验：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-radar-paused --radar-paused
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-radar-unpaused --radar-unpaused
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-radar-control --radar-control
$sdkFfmpeg = (Get-Command ffmpeg).Source
python -B tools/verify_mission_radar_media.py --client $sdkClient --paused D:/rc2/new-sdk-radar-paused --unpaused D:/rc2/new-sdk-radar-unpaused --control D:/rc2/new-sdk-radar-control --ffmpeg $sdkFfmpeg --python-deps target/sdk-audio-tools --output D:/rc2/new-sdk-radar-media
```

所有输出目录须不存在。三组均经TS构建、依赖审计、普通安装与唯一请求检查；控制组保持同样的前后消息/计时/输入规则，但不请求影片。背景音乐声明为静音的SdkQuiet，以隔离Bink音轨。准备器导出生产查询命中的原始BIK和GAME.FNT；MIX仅硬链接，松散配置复制。工具正常点击战役菜单后22秒不发送键鼠，保留完整PNG及原始雷达BGRA。固定1000×736客户区中，影片是雷达表面(832,48)内居中的140×110区域，即(846,48)；与FFmpeg独立解码45帧RGB逐帧求相关，要求多帧实际画面、约三秒可见区间和四段橙色字幕按序完整出现。已知文本用同安装GAME.FNT精确位图检测，渐显未完成的文字不当作完整短语。

两种播放模式均须匹配音轨的两段时频指纹，且匹配时间落在实际可见影片期间；不播放组不出现匹配画面、字幕或音轨。对照声明静音，允许实际零幅度录音，但仍要求录音样本存在、有限且无回调错误；有影片两组必须非零。原始WASAPI浮点PCM、WAV、逐包单调时钟和严格波形相关度均保留；严格PCM相关度失败仍原样记录，不以时频通过代替逐样本保真。第16游戏秒剧情消息在暂停组须晚于影片结束，在不暂停组可先于影片结束，两组影片后仍能看到后续消息。逻辑帧/倒计时暂停及键鼠恢复沿用上面的实时服务检查，当前普通窗口工具不读写模拟状态。不覆盖原版像素、扬声器听感、菜单跳过、嵌套或播放中存档恢复。

影片音轨输出可用独立解码和WASAPI回环比较。使用当前准备器重新生成上述两份夹具；完整安装中存在影片时，准备器额外导出生产资源查询命中的原始`A01_P01e.bik`，不更改安装资源。需要FFmpeg及前述本地音频验证依赖：

```powershell
$sdkFfmpeg = (Get-Command ffmpeg).Source
python -B tools/verify_mission_movie_audio.py --client $sdkClient --fixture D:/rc2/new-sdk-team-media --control-fixture D:/rc2/new-sdk-team-media-control --ffmpeg $sdkFfmpeg --python-deps target/sdk-audio-tools --output D:/rc2/new-sdk-movie-audio
```

此工具复用普通战役窗口核验，在整个启动/播放期间回录Windows默认输出，保存原始浮点PCM、可试听WAV和逐回调时间戳。FFmpeg独立解码第一音轨，取0.25～1.85秒和1.25～2.55秒两段，在整个回录中搜索时频指纹；两段相关度都须≥0.9、出现在可见影片期间，next对照的最佳相关度都须<0.6。指纹使用64毫秒Hann窗、16毫秒步长及约47～3422Hz幅度谱，允许输出相位变化。严格式波形相关度也原样记录为`strict_waveform_match`，不会因频谱匹配而改成通过；这证明音轨输出，不证明每个PCM样本、设备相位、扬声器听感或原版保真。每包音频时刻独立映射到与PNG相同的单调时钟，避免把WASAPI省略的设备空闲期当作连续墙钟时间。

音乐和 EVA 的实际输出可用独立 `--audio-streams` 变体验证。它经 TS 声明媒体依赖，生成静音开局曲目、660 Hz 音乐和880 Hz EVA；原有440 Hz全局/航点声音保留。静音开局曲目避免随机背景音乐掩盖短音，不替换原始安装的文件。以下命令需要 Windows 默认输出的 WASAPI 回环设备；Python 包只供本地验证使用，不加入编辑器运行依赖。

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_presentation_fixture -- D:/Games/YR target/new-sdk-audio-streams --audio-streams
python -m pip install --target target/sdk-audio-tools PyAudioWPatch numpy
python -B tools/verify_mission_audio.py --client $sdkClient --fixture target/new-sdk-audio-streams --output target/new-sdk-audio-loopback --python-deps target/sdk-audio-tools
```

`verify_mission_audio.py` 从现有隐藏窗口战役入口运行到720帧，同时录制默认输出设备的回环流。音乐在第3秒请求，EVA在第30秒请求，为音乐的墙钟淡入/播放留出间隔。`report.json` 必须整体通过：三种主频的持续非零输出、采样有限、无回调错误、音乐先于 EVA、客户端观察到 `SdkEva` 活动且到达目标帧。产物含 `loopback.wav`、客户端日志和 `capture/capture.json`。记录的秒数按返回采样序列计算，设备不活动期间 WASAPI 可能不提供采样，因此不能把它当作游戏时间或完整墙钟时间。这项检查证明自定义音乐/EVA动作实际输出，不代表原版曲目/语音保真、扬声器听感或影片音轨验收。

SDK 适配优先使用引擎已有公开接口。当前 vendor 差异限于只读资源查询：将动画名与粒子系统的真实注册序号查询从 `pub(crate)` 改为 `pub`，以及新增 `weapon_array_len()`。武器查询在歧义槽位也返回空，必须知道完整数组长度才能保留后续编号；不能遇到空值就停止，也不能用按名称排序的目录推断编号。这些接口不改游戏执行逻辑。审计加载器须与生产路径一致，在通用效果根之外绑定 `trigger_anim_class_roots` 和 `script_anim_class_roots`。旧审计漏掉此步骤，曾将投放区信标失败误报为引擎缺口；补齐编辑器侧审计加载器后，`target/mission-sdk-production-load-final/report.json` 整体通过，含信标实际创建。发现真正的引擎缺口仍须逐项记录在计划表；审计失败应保留报告并返回非零退出码。

多人 SDK 的起点归属、循环增援和共享倒计时可独立通过生产 headless 加载器验收：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_multiplayer_audit -- D:/rc2/yl D:/rc2/new-sdk-multiplayer
```

输出目录须不存在。工具使用同一份强类型 TS 任务，在 Skirmish、LAN、WOL 三种实际会话模式各交换两名人类玩家的起点，并分别连续运行或在第50帧通过公共快照路径恢复，共12组。检查原始对象从 Neutral 经零秒触发器转交实际玩家、每两秒增援三次、空起点不创建小队、后续脚本标记、倒计时设置/停止/继续/增减，以及恢复后的列明对象、完整Team、起点映射、全局变量和倒计时时序一致。本夹具的两个转交规则分别在第1/2帧执行，不能把加载完成视为转交完成。LAN/WOL 在推进前通过现有生产接口安装 MaxAhead=10，不注入成员、通知、计时器或起点映射。报告保留各阶段、输入指纹和快照；该证据不覆盖套接字通信、多客户端同步、GUI加载或原版SAV兼容性。

实际双客户端传输另用 Windows 回环 UDP 核验。先生成上面的多人夹具，再构建只读录像观察器；客户端使用前文独立构建的 `$sdkClient`，所有输出目录须不存在：

```powershell
cargo build --locked --offline -p ra2-map-io --example mission_sdk_lan_replay_audit
python -B tools/verify_mission_lan.py --client $sdkClient --replay-audit target/debug/examples/mission_sdk_lan_replay_audit.exe --game 'D:/rc2/Command & Conquer Red Alert II' --fixture D:/rc2/new-sdk-multiplayer --output D:/rc2/new-sdk-lan
```

工具为两端各建独立安装、设置、日志和录像目录，通过已有 `lan-live-host/guest` 客户端入口运行真实房间服务、加载、UDP 命令交换及两次开局。静态 MIX 仅硬链接，其他松散资源复制；输出与游戏须在同一卷。生成的 `multiplayer.map` 原字节以 `sdk-network.yrm` 安装，便于普通联机地图目录发现，准入 SHA256 必须与夹具相等。默认使用 33120/33122 与 33130/33132 四个 UDP 端口，可通过 `--host-port`、`--guest-port` 调整；超时仅终止本工具拥有的进程。

`report.json` 必须整体通过：两端正常结束两局并清理房间/传输状态；每局 300 项原生校验、20 项强状态哈希、结束时完整状态摘要和结果一致；两端的 Move/Stop 经 MaxAhead=10 延迟实际执行。随后四份录像各通过生产 `ReplayPlayback` 的地图/规则/开局及全部记录哈希校验，结束哈希与对应真实联机 receipt 相等；观察器只读取增援、原始对象、起点、Team、全局变量和倒计时，不修改回放命令或哈希。每份检查第 31/61/91 帧累计 2/4/6 名工程师、空起点无成员/标记、第 106 帧停用后至 300 帧无新增、原始坦克转交后保留，以及倒计时设置/停止/继续/增减。两端各阶段和第 300 帧列明状态再作比较；运行捕获的最后一帧可能不同，不能直接把两个运行快照当成同帧全量比较。

单份实际录像也可独立复查，报告文件须不存在：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_lan_replay_audit -- D:/rc2/new-sdk-lan/host/install D:/rc2/new-sdk-lan/host/replays/replay_tick430_TIMESTAMP.json D:/rc2/new-replay-sdk-report.json
```

此验收覆盖同一机器上两个独立客户端的真实回环通信与列明 SDK 状态，不覆盖跨机器网络、WOL 传输、丢包/重连、联机存档或原版 `.SAV` 兼容性；不需要改动引擎。

胜负动作与战役切关另用普通可见 Windows 客户端核验，需要桌面保持前台且窗口无遮挡，以及前述 numpy/PyAudioWPatch 验证依赖：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_result_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-result-fixtures
python -B tools/verify_mission_results.py --client $sdkClient --fixtures D:/rc2/new-sdk-result-fixtures --python-deps target/sdk-audio-tools --output D:/rc2/new-sdk-results
```

准备器生成四种 Country 胜负匹配/不匹配、`winPlayer`/`losePlayer`、`endScenario`、小队 `winPlayer`/`losePlayer` 共九种任务及无胜负动作对照。每组均以 TS `defineCampaign` 构建并发布两关，经过完整依赖审计和干净安装；原安装只读。工具普通点击单人、新战役、盟军，随后由任务自身的规则或招募后脚本触发结果，不调用隐藏捕获器或设置胜负字段。约27秒点击胜利页 Continue 或失败重试框 No，40秒结束观察；只关闭自己拥有的客户端。可加 `--case country_win_match` 等名称单独复查，输出目录须不存在。

`report.json` 必须整体通过：九条路径分别出现胜利结算标题/Continue 或失败重试问题/Yes/No，声音由真正的结果事件请求，胜利660Hz、失败880Hz各至少0.6秒，另一频率不通过；对照两者都不出现。胜利点击后必须在第二关看到开局消息与继承的全局标记7，失败点击No后提示框消失且没有进入第二关。保留PNG、输入时间戳、原始WASAPI浮点PCM/可试听WAV及逐包时钟；客户端、工具和实际字体指纹随报告记录。

文字检测独立读取相同安装的 `GAME.FNT` 位图，游戏消息按原尺寸精确匹配；1000×736客户区的shell文字按736/600缩放，允许每字形分数像素舍入导致的形状差异，匹配须同时满足范围和至少0.80交并比。结算标题与按钮、失败问题与两个按钮须成对/成组出现；仍应查看各组 `before-answer.png` 和切关消息帧确认实际画面。此为已知短语检测，不是通用OCR、任意语言/字体/分辨率验收。测试EVA使用短于8字符的样本名（如 `sdkwin`/`sdklose`），因为引擎按原生九字节槽只保留8字符；较长名称可能被截断为不存在的声音。这里验证结果EVA真实输出，不声称原版语音保真、Win/Lose影片、所有战役分支或存档组合已验。

结算评级另用两组定向夹具验证：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_result_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-ranking-fixtures --ranking-only
python -B tools/verify_mission_results.py --client $sdkClient --fixtures D:/rc2/new-sdk-ranking-fixtures --python-deps target/sdk-audio-tools --output D:/rc2/new-sdk-ranking-results
```

`ranking_under` 的三难度基准均为60秒，`ranking_over` 均为0秒，胜利由第4秒的TS规则触发。普通客户端必须同时显示对应评级标题、评语、基准时间、Mission Score和Continue，另一组评级文案不得出现；继续沿用结果EVA和第二关全局标记检查。准备器额外检查四个评级CSF依赖全部解析、显式缺失标签阻止发布、未配置Ranking时不引入缺省GUI标签依赖。三难度不同秒数、原生时间编码/30字节标签边界、中文文本、重建/删除原值恢复及撤销/重做在SDK回归中验证。本工具的已知短语检测使用ASCII夹具，任意中文字体呈现不在该检测范围内。

开局场景标志的定向运行验证使用：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_special_flags_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-special-flags
```

工具在新目录内构建七份强类型TS任务并验证无变化重建。五组单人任务各连续推进240帧及第10帧公共快照恢复，检查 Inert 对真实武器爆炸的伤害门控、HarvesterImmune 对自然自动选敌的排除、FogOfWar/桥梁标志加载、普通与加速增长的原生计时和邻格矿石扩散；两组多人任务分别以 Skirmish/LAN/WOL 实际模式加载，检查精英与新兵起始部队、MCV及三名地图预放对象等级。`report.json` 共16组、122项检查须全部通过。资源安装只读，失败及通过产物分开保存。

`TiberiumGrows` 是加速位：当前原版 Riparius 的 Growth=2200，关闭该位重装2200帧，开启重装659帧（原生0.3倍截断）。它不代替 Basic/TiberiumGrowthEnabled 总开关，首轮增长处理也可能在第1帧发生。扩散会创建同矿种的不同Overlay变体，不能只数最初的单一Overlay编号。公共快照恢复会重置既有Scenario RNG，因此只比较即时列明状态、最终场景标志和各自的效果条件，不宣称后续随机矿石位置或全状态一致。

审计共用的 `examples/support/campaign.rs` 现与普通客户端加载器一样，把地图的 Inert/HarvesterImmune/矿石特殊位投影到既有 ScenarioDescriptor；不向模拟注入损伤、目标或矿石结果。此工具核验模拟和开局状态，不核验迷雾GPU、真实网络传输或原版SAV兼容性。`MCVDeploy` 只解析未消费，SDK仍拒绝该字段。

开局光照的真实资源检查使用：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_lighting_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-lighting
```

工具构建七份强类型TS任务并检查无变化重建。六组单人任务各连续推进1000帧和效果期间公共快照恢复，检查三组光照的18个分量及两项间隔、普通初始亮度、风暴/心灵控制器/核爆选用的光照、真实亮度变化和返回普通亮度。心灵控制器通过已授予/充满的超武和正常同步命令发射；核爆使用TS的 `nukeAt()` 发射并等待弹体命中。两种淡变间隔另比较同帧亮度，必须观察到快组实际变化更快，不能只检查配置或计时器字段。多人任务在实际Skirmish/LAN/WOL模式检查全部20个开局字段；`report.json` 共15组、114项检查须全部通过。

共用campaign审计加载器按普通客户端现有路径构造 `ScenarioLightingState::from_map()`；没有修改引擎或注入选中的光照。快照对照只要求列明的完整光照状态及恢复后截至第1000帧的观察轨迹一致，不扩大为随机对象或全模拟状态相等。SDK回归另验原生单精度量化、负值、溢出/非有限值、未知字段、错误间隔、原值恢复、冲突、撤销/重做；GPU呈现、联机传输及原版SAV兼容不在此工具范围内。

开局光照的 GPU 检查复用固定客户端现有的 Battle 捕获入口：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_lighting_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-lighting-gpu-fixtures
python -B tools/verify_mission_lighting.py --client target/vera-client/debug/vera20k.exe --game 'D:/rc2/Command & Conquer Red Alert II' --fixture D:/rc2/new-sdk-lighting-gpu-fixtures --contract vendor/vera20k/src/app/diagnostics/tactical_capture/contract.v2.json --output D:/rc2/new-sdk-lighting-gpu
```

准备器生成十二份 TS 任务：普通白光、暗光、红/绿/蓝单通道、自定义普通光照、风暴、两种核爆淡变间隔、两种心灵控制器淡变间隔，以及心灵控制器残留 Ground 的独立对照。每份经真实构建和无变化重建、生产 headless 普通帧推进并保存 `runtime.json`；捕获器按固定种子、镜头、四个无单位遮挡的平地格保存 BGRA 原帧、PNG 和完整 receipt。检查像素明暗、关闭的颜色通道、侧栏原色、效果期间光照及结束后状态、快慢间隔的像素差异；同时核对地图指纹和实际绘制的 palette_light。心灵控制器通过 TS 授予超武及正常同步命令发射，符号命令使用已有 v3 profile；风暴须等待真实启动/退场，不能将请求帧当成生效帧。

心灵控制器恢复普通模拟亮度和 RGB 后，现有引擎的 FadingBack→Idle 只发布后续 relight profile，不重新刷新已绘制格子；这些格子仍可能保留 DominatorGround 对亮度的影响。本夹具的普通 Ground=0.05、DominatorGround=0.01，恢复后与普通初始帧像素不同，须与单独声明 Ground=0.01 的 TS 对照一致。不能把“模拟恢复普通亮度”扩大成“所有格子立即恢复开局像素”；本工具保留这种现有行为，不修改引擎。无战争迷雾/黑幕时，远离单位视野的格子 `visible=false` 仍正常绘制，验收依据为实际 `revealed=true`、A=127 和像素。

输出和原安装须同卷，静态 MIX 只硬链接，松散配置复制到独立安装与设置目录。工具不修改原资源或引擎；只验证固定客户端的列明 Skirmish 地形样本，不宣称原版像素一致、战役 GPU、所有高程/点光源、全状态或网络等价。可用相同参数加 `--analyze-only` 重新读取已保留的原帧与 receipt。

House 显示名、增援方向及有效 Country 归属的定向检查使用：

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_house_names_edges_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-house-names-edges
```

工具构建八份强类型 TS 任务，覆盖四个方向、Automatic、Air、显式运输航点和新建 House。各组连续运行及第35帧载客中公共快照恢复，合计16组208项检查须全部通过：实际 House/CSF 名称与边缘、运输机首次出现的独立地图边缘坐标、两名原始乘员实际着陆存活和脚本完成；中文与 Emoji 的CSF内容精确匹配、文本依赖解析及移除CSF阻止发布、无变化重建。每组即时保存结果，失败时保留已完成证据。此工具不验证 GPU 文字、其他运输路径或联机传输。

TeamType/House 的生产读取器接受 Country 或起点玩家标记，不能直接使用任意地图 House ID。SDK 根据包含本轮 Country 修改的投影 roster 编码，复用已有 Rules 查询按原生顺序投影 Country 的别名和 ID，保留首次命中。目标必须是该 Country 的第一个 House；后续同国 House 和无效 Country 报错。库回归覆盖别名/ID碰撞、大小写、默认首国、修改Country、歧义、原值恢复和事务历史；新建 House 的真实增援验证该编码路径，无需修改引擎。

## TS 战役迷雾 GPU 对照

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_fog_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/mission-sdk-fog-fixtures-new
python -B tools/verify_mission_fog.py --client target/vera-client/debug/vera20k.exe --fixture D:/rc2/mission-sdk-fog-fixtures-new --output D:/rc2/mission-sdk-fog-captures-new
```

输出目录必须为新目录。四份TS任务对照FogOfWar和局部探索，使用现有ALL1战役开场捕获，到第120帧检查三个20×8地形样本、侧栏及镜头；35项须全通过。`--analyze-only`可重新读取已保存捕获。原始帧、receipt、PNG预览和输入指纹均保留。逐格视野来自同图独立普通帧运行，shell receipt没有这些格子的GPU进程视野读回。各客户端开局种子不同，当前可见区域亮度允许10%的地形变体差异，不以整图哈希相同作为验收。现有图形选项保持默认、不启用translucent_fog。此为战役平地呈现检查，无引擎修改。

## TS House 名称来源审计

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_house_ui_sources_audit -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/mission-sdk-house-ui-sources-new
```

输出必须为新目录。两份TS任务均给四个House写入不同中文UIName；战役及实际Skirmish/LAN/WOL模式分别加载并推进60帧，28项检查必须全通过。报告记录每个House的实际名称来源：战役CSF、多人人类Handle、电脑Computer、Neutral/Special的TypeName；没有手工设置模拟名称。普通战役没有多人House列表入口，本工具只验证模式加载和元数据，不宣称GPU文字已验，不修改引擎创造UI入口。

## TS 运行时地图范围、雷达和快照对照

```powershell
cargo run --locked --offline -p ra2-map-io --example mission_sdk_map_area_fixture -- 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-map-area-fixture
python -B tools/verify_mission_map_area.py --client target/vera-client/debug/vera20k.exe --game 'D:/rc2/Command & Conquer Red Alert II' --fixture D:/rc2/new-sdk-map-area-fixture --contract vendor/vera20k/src/app/diagnostics/tactical_capture/contract.v2.json --output D:/rc2/new-sdk-map-area-gpu
```

两个输出目录必须不存在。准备器构建完整范围、缩小、缩小后恢复三份强类型TS多人任务；原始Neutral对象通过开局规则正常转交，电厂与空指部提供正常在线雷达。第151帧缩小LocalSize，第181帧以相同参数再次写入，第211帧恢复。三组各连续运行至240帧，另在165/195帧分别走公共快照恢复，231项检查须全部通过，包括六名原始对象身份/坐标/生命保持、范围标记切换、平地通行区、四个建筑占用格的实际PathGrid阻挡、修订号每次递增及列明完整观察轨迹与连续运行相同。缩小后建筑阻挡格仍可有正的简化zone标签，因此不以zone=0冒充建筑阻挡检查。

GPU工具复用现有Battle捕获与正常minimap鼠标输入，分别保存0/165/195/240帧，共12次捕获、53项检查。九次点击均在[940,100]，完整范围目标格[62,63]、缩小后[64,64]；重复写入保持后者，恢复后的目标和雷达区域像素须与完整范围对照相等。雷达有开启动画，夹具第45/75帧点击被正常输入门控拒绝，第120帧无输入探针已显示在线雷达，因此最终范围变化和点击均放在开启动画之后。不能通过直接调用坐标转换绕过点击准入。

保留全部BGRA、receipt、PNG、普通帧观察、两份快照和输入指纹；`--analyze-only`可复核已有捕获。原安装与输出同卷，MIX硬链接、松散配置复制。逐帧几何/快照来自独立模拟运行；GPU进程验证雷达重绘和实际点击，不声称整幅战术画面裁剪、高程/桥梁、所有寻路层级、GUI加载或全状态等价。此批无SDK产品API或引擎修改。

## TS 普通存档浏览器验收

```powershell
cargo build --locked --offline -p ra2-map-io --example mission_sdk_save_browser_fixture --example mission_sdk_save_audit
target/debug/examples/mission_sdk_save_browser_fixture.exe 'D:/rc2/Command & Conquer Red Alert II' D:/rc2/new-sdk-save-browser-fixture
python -B tools/verify_mission_save_browser.py --client target/vera-client/debug/vera20k.exe --auditor target/debug/examples/mission_sdk_save_audit.exe --fixture D:/rc2/new-sdk-save-browser-fixture --output D:/rc2/new-sdk-save-browser-gui --python-deps target/sdk-audio-tools
```

准备器输出必须为新目录；客户端验收要求新输出目录、夹具还没有saves目录、Windows前台桌面无遮挡，以及前述numpy验证依赖。只向工具自己启动的前台客户端发送正常鼠标/键盘输入。它从单人→新战役进入生成任务，保存a、运行一段后保存b、在加载浏览器选中较早的a，实际恢复后再保存c并继续至守卫完成/停止增援后保存d。每个页面用生产字体匹配标题，保存成功提示、四份实际存档和加载提交日志均独立检查；加载日志出现后还须等待真实游戏倒计时重绘，不能把“Mission is loading”画面当作已恢复画面。

最终报告37项必须全通过。`mission_sdk_save_audit`只读生产封套与保存的列明对象/Team/Trigger状态，单独运行并不构成GUI恢复证明；此处另以正常加载后的真实保存c/d、窗口画面和生产提交日志共同建立证据。保存时游戏暂停，输入耗时不计作逻辑帧；不要求c与a完全同帧，但必须比b早、接近a，且原始守卫与重复触发器的计时锚点/时长保持。该任务中增援在第451帧先于同帧的停止规则执行，最终15名ENGINEER；不把该规则定义次序推广到任意任务。保留全部PNG、四份存档、独立状态JSON、客户端日志与指纹。MIX只读共享，松散输入为独立副本，设置文件退出后恢复；没有引擎改动。不验证真实联机读档、原版SAV或全状态等价。

## TS SDK 整体回归

涉及当前SDK、资源目录、文档托管、场景或GUI的改动，按同一组包运行库测试；IO用例共享部分进程环境，使用串行测试：

```powershell
cargo test --locked --offline -p ra2-map-document -p ra2-map-assets -p ra2-map-scene -p ra2-map-io -p ra2-map-mission-script -p ra2-map-app --lib -- --test-threads=1
cargo clippy --locked --offline --workspace --no-deps --all-targets -- -D warnings
./tools/format_workspace.ps1 -Check
git diff --check
git -C vendor/vera20k diff --check
```

包级格式脚本只选择编辑器workspace成员，避免 `cargo fmt --all` 扫描引擎。能力矩阵回归会对照固定引擎的真实动作分发器及小队命令默认/Ares准入；升级引擎后必须审计增删与模式，不能仅修改预期数量来通过。库测试及静态检查不能代替客户端呈现、真实联网和存档浏览器验收，具体剩余入口与关闭条件见[SDK计划](mission-sdk-plan.zh-CN.md#s40-剩余项的当前入口与关闭条件)。

## 补给箱建筑 SDK 验证

实现只生成原生地图规则、触发器和奖励小队：`CrateBeneath=no`，`BuildLimit=1`，玩家的建筑存在→不存在触发一次待发奖标记；重置各奖励分支随机计时器，首个到期分支清标记并超时空增援到 `rewardAt`。普通 `Powerups`、`CrateRules` 和车辆 `CrateGoodie` 不变，奖励副本设为 `CrateGoodie=no`。无引擎扩展。仅单人，ticket 专用且禁止预放，玩家 Country 不能共用；不能获得动态放置坐标，随机同秒平局按 Tag 顺序，不保证等概率。

```powershell
cargo test --locked --offline -p ra2-map-mission-script --lib crate_lottery -- --test-threads=1
cargo run --locked --offline -p ra2-map-io --example mission_sdk_crate_lottery_audit -- GAME_DIRECTORY NEW_OUTPUT_DIRECTORY
```

审计关闭随机箱子，使用真实资源、TS 构建和同步生产／PLACE／ForceAttackCell 命令，验证付款、阻挡重试、未拆不发奖、拆毁直接到航点、连续购买、取消／未购买不发奖，以及就绪商品／完整建筑／已发奖 headless 快照恢复。场上特意没有矿车。对比构建前后普通箱子全局字段和原车型 CrateGoodie，确认保持原样。没有注入进度、伤害、生命、资金或奖励。无窗口验证不代表 GUI 或真实联网验收。详见[用法](../examples/missions/README.md#购买补给箱建筑攻击拆开领取车辆)。

## 引擎版本管理

编辑器使用 `vendor/vera20k` 子模块固定的引擎提交，切换编辑器版本后运行：

```powershell
git submodule update --init --recursive
```

引擎是独立 workspace；子模块提交与 `Cargo.lock` 共同构成可复现构建输入。初始化、恢复和升级的完整流程见 [引擎依赖说明](../vendor/README.md)。


奖励车辆隔离：`m.rewardUnit` 将宿主加载得到的基础车辆规则快照与当前地图覆盖合并，写入托管的 `VehicleTypes` / `TSRU…` section。规则快照不发送到 TS evaluator。`Image` 复用基础 ART；副本设置隐藏生产与禁止随机开局，`CrateGoodie=no` 禁止普通箱子抽到副本。已有武器引用按目录验证。托管清单负责重复构建、重新加载目录后的构建、删除恢复与手工冲突检测；不改 `vendor/vera20k`。

上面的 `mission_sdk_crate_lottery_audit` 同时构建 `examples/missions/reward-unit/mission.ts` 并加载其真实规则，验证奖励类型生命与武器继承、普通车辆属性保持、隐藏生产、保存重新加载后的字节稳定性，以及购买→放置→拆毁→任务发奖和检查点恢复流程。示例中的可选注释不代表所有特殊能力都完成运行验收；Mod 的路径相关默认值与扩展规则仍须按实际资源验证。
