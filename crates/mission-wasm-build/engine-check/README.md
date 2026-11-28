# vera-mission-engine-check

Standalone acceptance runner for real VERA map Wasm modules. This crate is
separate from the editor workspace and links only to the engine's actual
`vera20k-wasm-ext` implementation through a relative path dependency.

Both `fixture/` and `ui-fixture/` are standalone modules identified only by their own `vera.json` (`kind: "rules"` and `kind: "ui"` respectively). They need no `package.json` or separate project marker. Joint projects instead set `"vera": { "kind": "extension" }` in their root `package.json`; see the [builder README](../README.md).

The JavaScript fixtures and builder regressions can be verified separately with `npm test` from `crates/mission-wasm-build/`, without running Cargo. The commands below are optional engine acceptance checks.

## Run

From the repository root:

```powershell
npm ci
node crates/mission-wasm-build/project-build.mjs crates/mission-wasm-build/engine-check/fixture target/assemblyscript-engine-check.wasm
cargo run --locked --manifest-path crates/mission-wasm-build/engine-check/Cargo.toml -- target/assemblyscript-engine-check.wasm --frames
```

Without `--frames`, the runner measures `vera_init`, `on_rules`, and `on_scenario_start`.
With `--frames`, it additionally runs 600 frame callbacks, fixture timer deliveries,
and a save/restore roundtrip at frame 300. Both use `Limits::MAP`; no
production budget is raised. Any load, lifecycle, frame, event, restore, or
fixture assertion failure exits non-zero.

The engine API reports successful operation counts. If a call trips the
budget, it returns `Fault::Budget` without exposing the exact attempted total;
the runner reports the applicable production limit and exits non-zero rather
than retrying with a larger budget.

The host is a deterministic fixture for `fixture/src/mission.ts`, which uses the engine TSSDK directly; it is not a simulated game world. It supplies one E1-like query result, fixed owner and
health properties, local/global variable storage, rule recording, log
collection, and timer triggers solely to exercise the SDK paths.

## Current limits reflected by this runner

- `Limits::MAP`: 16 MiB maximum linear memory.
- Init/rules/scenario-start: 5,000,000 operations per call, matching
  `vendor/vera20k/src/sim/ext_modules.rs::ONCE_BUDGET`.
- Frames and event batches: 100,000 operations per call, matching
  `MAP_FRAME_BUDGET`.
- Wasm input file: 8 MiB, enforced by the real engine loader.
- Individual function body: 1 MiB; at most 20,000 defined functions, enforced
  by the real engine loader.
- The engine meter charges one operation per original Wasm instruction in a
  basic block before execution. Bulk copy/fill/init/table instructions are
  additionally charged one operation per 8 bytes of requested length.

The operation budgets are current engine configuration, not WebAssembly
limits. This tool deliberately neither changes them nor treats its fixture
host as production behavior.


## Client UI lifecycle fixture

The independent `ui-fixture/` uses `@ra2-map-editor/mission-sdk/client` and only
imports `client_log` and `client_frame` from `vera:client@1.0`. It registers
`onStart`, `onRender`, and `ClientEvent.SelectionChanged` without touching UI
nodes or game APIs. Compile it and run the editor workspace's actual client
runtime probe:

```powershell
node crates/mission-wasm-build/project-build.mjs crates/mission-wasm-build/engine-check/ui-fixture target/assemblyscript-ui-engine-check.wasm
cargo run --locked -p ra2-map-io --example wasm_ui_smoke -- target/assemblyscript-ui-engine-check.wasm
```

The probe delivers start once, render 600 times, and two selection changes
(at zero-based frames 0 and 300). Only the 600th render logs the accumulated
counts:

```text
ui lifecycle start=1 render=600 events=2 frame=599
```

Node tests also instantiate the compiled fixture and assert this exact log and
its client imports, lifecycle exports, and delivery metadata. The Rust probe
uses the real `ClientModule` with production budgets; neither test renders a
visible game UI or simulates a live match.
