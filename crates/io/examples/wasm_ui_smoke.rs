//! 已编译 ui.wasm 的真实引擎运行时冒烟验证。
//!
//! 用法：cargo run -p ra2-map-io --example wasm_ui_smoke -- <ui.wasm> [--expect-log <text>]
//!
//! 此工具验证 client ABI、生命周期和生产执行预算，不启动图形界面或真实对局。
//! 默认 UI 模板只需日志和帧读取；其他 host 能力明确拒绝，不能据此验证节点绘制、
//! 玩家视野、网络消息或本地存储。已有自定义 UI 若依赖这些能力，应在游戏中验证。

use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
use vera20k_wasm_ext::{
    ActionParams, ClientEvent, ClientHost, ClientModule, FindPage, Invalid, Limits, NodeInit,
    Order, Query, Source, Status,
    client::{CLIENT_FRAME_BUDGET, CLIENT_ONE_TIME_BUDGET, client_event},
};

const RENDER_FRAMES: i32 = 600;
const RENDER_HZ: f32 = 60.0;
const LOG_PREVIEW_LINES: usize = 24;
const CLIENT_EXPORTS: [&str; 4] = [
    "client_init",
    "client_on_start",
    "client_on_events",
    "client_on_render",
];

#[derive(Default)]
struct ProbeHost {
    frame: i32,
    phase: &'static str,
    log_lines: usize,
    nonempty_logs: usize,
    expected_log: Option<String>,
    matched_expected_log: bool,
    unsupported: Option<&'static str>,
}

impl ProbeHost {
    fn check(&self) -> Result<()> {
        ensure!(
            self.unsupported.is_none(),
            "{} at frame {} requires unsupported probe host call: {}",
            self.phase,
            self.frame,
            self.unsupported.unwrap_or_default(),
        );
        Ok(())
    }
}

// 将不支持的能力集中列出，避免维护一套与游戏实现逐渐偏离的假 UI。
// 返回 Invalid 会终止真实运行时；即使 guest 忽略状态，也不会被计作成功。
macro_rules! unsupported_calls {
    ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $result:ty;)+) => {
        $(
            fn $name(&mut self, $($arg: $ty),*) -> $result {
                self.unsupported = Some(concat!("client_", stringify!($name)));
                Err(Invalid(format!(
                    "wasm_ui_smoke does not implement client_{}; validate this UI in-game",
                    stringify!($name),
                )))
            }
        )+
    };
}

impl ClientHost for ProbeHost {
    fn frame(&mut self) -> i32 {
        self.frame
    }

    fn log(&mut self, line: &str) {
        self.log_lines += 1;
        if self.expected_log.as_deref() == Some(line) {
            self.matched_expected_log = true;
        }
        if !line.trim().is_empty() {
            self.nonempty_logs += 1;
        }
        if self.log_lines <= LOG_PREVIEW_LINES {
            println!("[{} frame={}] {line}", self.phase, self.frame);
        }
    }

    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        // 此接口没有错误返回值，必须另外记录并在本次生命周期调用后检查。
        self.unsupported = Some("client_has_kind");
        false
    }

    unsupported_calls! {
        fn lookup(_kind: i32, _name: &str) -> Result<(i32, Status), Invalid>;
        fn name_of(_kind: i32, _id: i32) -> Result<(String, Status), Invalid>;
        fn get(_reference: i64, _prop: i32) -> Result<(i64, Status), Invalid>;
        fn find(_query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid>;
        fn var_get(_scope: i64, _var: i32) -> Result<(i64, Status), Invalid>;
        fn set(_reference: i64, _prop: i32, _value: i64) -> Result<Status, Invalid>;
        fn set_bytes(_reference: i64, _prop: i32, _bytes: &[u8]) -> Result<Status, Invalid>;
        fn node_create(_kind: i32, _parent: i64, _init: &[NodeInit]) -> Result<(i64, Status), Invalid>;
        fn node_remove(_node: i64) -> Result<Status, Invalid>;
        fn node_find(_name: &str) -> Result<(i64, Status), Invalid>;
        fn action(
            _kind: i32,
            _target: i64,
            _params: &ActionParams,
            _text: &str,
            _extra: &[u8],
        ) -> Result<Status, Invalid>;
        fn send_order(_order: &Order) -> Result<Status, Invalid>;
        fn send_message(_data: &[u8]) -> Result<Status, Invalid>;
        fn storage_read(_key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid>;
        fn storage_write(_key: &str, _data: &[u8]) -> Result<Status, Invalid>;
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (path, expected_log) = match args.as_slice() {
        [path] => (path, None),
        [path, flag, text] if flag == "--expect-log" => (
            path,
            Some(
                text.to_str()
                    .context("expected log must be UTF-8")?
                    .to_owned(),
            ),
        ),
        _ => anyhow::bail!("usage: wasm_ui_smoke <ui.wasm> [--expect-log <text>]"),
    };
    let path = Path::new(path);
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    // 直接交给引擎的 client 加载器检查导入、签名、delivery 和地图内存上限。
    let mut module = ClientModule::load(&bytes, Limits::MAP)
        .map_err(|error| anyhow::anyhow!("load {} as client ABI: {error}", path.display()))?;
    let exports: Vec<_> = CLIENT_EXPORTS
        .into_iter()
        .filter(|name| module.has_export(name))
        .collect();
    ensure!(
        !exports.is_empty(),
        "expected client lifecycle exports; this is not a runnable ui.wasm",
    );
    let has_events = module.has_export("client_on_events");
    let has_render = module.has_export("client_on_render");
    println!("client exports: {}", exports.join(", "));
    let mut host = ProbeHost {
        phase: "init",
        expected_log,
        ..Default::default()
    };
    module
        .init(&mut host)
        .map_err(|error| anyhow::anyhow!("client_init: {error}"))?;
    host.check()?;
    host.phase = "start";
    module
        .start(&mut host)
        .map_err(|error| anyhow::anyhow!("client_on_start: {error}"))?;
    host.check()?;

    // 仅模拟一次空选择变化及中途再变化，不冒充存在任何地图对象或通知注册。
    let selection_changed = [ClientEvent {
        kind: client_event::SELECTION_CHANGED,
        source: Source::Map,
        reference: 0,
        id: -1,
        value: 0,
        data: Vec::new(),
    }];
    let mut event_batches = 0;
    for frame in 0..RENDER_FRAMES {
        // 这是 probe 的合成帧号，不代表运行了真实游戏模拟。
        host.frame = frame;
        // 每渲染帧只重置一次；events 与 render 共同消耗引擎默认预算。
        module.begin_frame();
        host.phase = "events";
        let events = if has_events && (frame == 0 || frame == RENDER_FRAMES / 2) {
            event_batches += 1;
            selection_changed.as_slice()
        } else {
            &[]
        };
        module
            .events(&mut host, events)
            .map_err(|error| anyhow::anyhow!("client_on_events at frame {frame}: {error}"))?;
        host.check()?;
        host.phase = "render";
        module
            .render(&mut host, 1.0 / RENDER_HZ, 0.5)
            .map_err(|error| anyhow::anyhow!("client_on_render at frame {frame}: {error}"))?;
        host.check()?;
    }
    ensure!(
        host.nonempty_logs > 0,
        "client produced no nonempty log; the default UI template must report readiness",
    );
    if let Some(expected) = &host.expected_log {
        ensure!(
            host.matched_expected_log,
            "missing expected client log: {expected}"
        );
    }
    if host.log_lines > LOG_PREVIEW_LINES {
        println!(
            "... {} additional log lines omitted",
            host.log_lines - LOG_PREVIEW_LINES,
        );
    }
    // 输出预算上限而非虚构实际消耗；ClientModule 的公开接口不暴露剩余指令数。
    println!(
        "PASS: {RENDER_FRAMES} render frames, render_export={has_render}, \
         {event_batches} event batches, {} log lines; \
         engine limits: {CLIENT_FRAME_BUDGET} operations/frame (events + render), \
         {CLIENT_ONE_TIME_BUDGET} operations/one-time call, {} bytes memory",
        host.log_lines,
        Limits::MAP.memory_bytes,
    );
    Ok(())
}
