//! Independent, production-budget acceptance runner for a VERA Wasm module.
//!
//! This crate intentionally depends only on the real `vera20k-wasm-ext` crate.
//! It does not alter the engine, the production linker, or any SDK package.
//! The host below is a deterministic fixture for the AssemblyScript lifecycle example:
//! it is not a replacement for the game's world host.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::process::ExitCode;

use vera20k_wasm_ext::{
    ActionParams, EventRecord, Export, Fault, FindPage, Invalid, Limits, Query, SimHost, Status,
    TriggerDef,
};

const ONCE_BUDGET: u64 = 5_000_000;
const FRAME_BUDGET: u64 = 100_000;
const MAP_MEMORY_BYTES: usize = 16 << 20;
const FRAME_COUNT: i32 = 600;
const TIMER_EVENT: i32 = 1023;

#[derive(Debug, Default)]
struct FixtureHost {
    frame: i32,
    vars: BTreeMap<i32, i64>,
    logs: Vec<String>,
    rules: Vec<(String, String, String)>,
    triggers: BTreeMap<i64, FixtureTrigger>,
    removed_triggers: BTreeSet<i64>,
    delivered_by_key: BTreeMap<i32, u32>,
    next_trigger: i64,
    next_modifier: i64,
}

#[derive(Debug, Clone)]
struct FixtureTrigger {
    repeat: vera20k_wasm_ext::triggers::Repeat,
    event_kind: i32,
    key: i32,
    interval: i32,
    created_at: i32,
}

impl FixtureHost {
    fn timer_events(&self, frame: i32) -> Vec<EventRecord> {
        let mut records = Vec::new();
        for (handle, trigger) in &self.triggers {
            if self.removed_triggers.contains(handle) || trigger.event_kind != TIMER_EVENT {
                continue;
            }
            let due = frame >= trigger.created_at
                && trigger.interval > 0
                && (frame - trigger.created_at) % trigger.interval == 0;
            if due {
                let mut record = EventRecord::empty(TIMER_EVENT);
                record.key = trigger.key;
                record.trigger = *handle;
                record.frame = frame;
                records.push(record);
            }
        }
        records
    }

    fn complete_timer_events(&mut self, records: &[EventRecord]) {
        for record in records {
            *self.delivered_by_key.entry(record.key).or_default() += 1;
            if self.triggers.get(&record.trigger).is_some_and(|trigger| {
                trigger.repeat != vera20k_wasm_ext::triggers::Repeat::Repeating
            }) {
                self.removed_triggers.insert(record.trigger);
            }
        }
    }

    fn check_runtime_state(&self) -> Result<(), String> {
        if !self
            .rules
            .iter()
            .any(|(section, key, value)| section == "E1" && key == "Strength" && value == "237")
        {
            return Err(format!(
                "AssemblyScript fixture rule write was not observed: {:?}",
                self.rules
            ));
        }
        if !self.logs.iter().any(|line| line == "mission ready")
            || !self
                .logs
                .iter()
                .any(|line| line.starts_with("mission timer "))
            || !self.logs.iter().any(|line| line == "console.info ready")
            || !self
                .logs
                .iter()
                .any(|line| line == "[debug] console.debug ready")
            || !self
                .logs
                .iter()
                .any(|line| line == "[warn] console.warn ready")
            || !self
                .logs
                .iter()
                .any(|line| line == "[error] console.error ready")
            || !self
                .logs
                .iter()
                .any(|line| line == "console.log timer fired")
        {
            return Err(format!(
                "AssemblyScript fixture expected log messages were not observed: {:?}",
                self.logs
            ));
        }
        if self.delivered_by_key.get(&2) != Some(&1) || self.delivered_by_key.get(&1) != Some(&40) {
            return Err(format!(
                "unexpected TSSDK timer delivery counts: {:?}",
                self.delivered_by_key
            ));
        }
        for id in [5, 7, 8, 9, 10, 11] {
            if self.vars.get(&id) != Some(&1) {
                return Err(format!(
                    "AssemblyScript fixture Local({id}) was not set: {:?}",
                    self.vars.get(&id)
                ));
            }
        }
        if self.vars.contains_key(&12) {
            return Err(format!(
                "canceled AssemblyScript timer unexpectedly fired: {:?}",
                self.vars.get(&12)
            ));
        }
        Ok(())
    }
}

impl SimHost for FixtureHost {
    fn frame(&mut self) -> i32 {
        self.frame
    }

    fn random(&mut self, lo: i32, _hi: i32) -> i32 {
        lo
    }

    fn log(&mut self, line: &str) {
        self.logs.push(line.to_owned());
    }

    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        true
    }

    fn lookup(&mut self, _kind: i32, name: &str) -> Result<(i32, Status), Invalid> {
        let id = match name {
            "E1" => 9,
            _ => return Ok((-1, Status::NotFound)),
        };
        Ok((id, Status::Ok))
    }

    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok(("E1".to_owned(), Status::Ok))
    }

    fn get(&mut self, _reference: i64, property: i32) -> Result<(i64, Status), Invalid> {
        // These are the property IDs used by runtime.ts:
        // owner=1 and health=3.  The values are deliberately positive so the
        // example follows its object-detected branch.
        let value = match property {
            1 => 140_737_488_355_328, // house(0)
            3 => 237,                 // current health
            _ => 0,
        };
        Ok((value, Status::Ok))
    }

    fn set(&mut self, _reference: i64, _property: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Ok)
    }

    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((
            FindPage {
                found: vec![(10, 10)],
                scanned: 1,
                ..FindPage::default()
            },
            Status::Ok,
        ))
    }

    fn var_get(&mut self, _scope: i64, id: i32) -> Result<(i64, Status), Invalid> {
        Ok((*self.vars.get(&id).unwrap_or(&0), Status::Ok))
    }

    fn var_set(&mut self, _scope: i64, id: i32, value: i64) -> Result<Status, Invalid> {
        self.vars.insert(id, value);
        Ok(Status::Ok)
    }

    fn action(
        &mut self,
        _kind: i32,
        _target: i64,
        _params: &ActionParams,
        _text: &str,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Ok))
    }

    fn modify(
        &mut self,
        _reference: i64,
        _property: i32,
        _op: i32,
        _value: i64,
        _frames: i32,
        _source: i64,
        _group: i32,
    ) -> Result<(i64, Status), Invalid> {
        self.next_modifier += 1;
        Ok((self.next_modifier, Status::Ok))
    }

    fn unmodify(&mut self, _modifier: i64) -> Result<Status, Invalid> {
        Ok(Status::Ok)
    }

    fn trigger_create(&mut self, definition: &TriggerDef) -> Result<(i64, Status), Invalid> {
        let event = definition
            .events
            .first()
            .ok_or_else(|| Invalid("fixture requires one event per trigger".to_owned()))?;
        let action = definition
            .actions
            .first()
            .ok_or_else(|| Invalid("fixture requires one action per trigger".to_owned()))?;
        self.next_trigger += 1;
        let handle = self.next_trigger;
        let interval = event.interval;
        let created_at = self.frame + interval;
        self.triggers.insert(
            handle,
            FixtureTrigger {
                repeat: definition.repeat,
                event_kind: event.kind,
                key: action.params.id,
                interval,
                created_at,
            },
        );
        Ok((handle, Status::Ok))
    }

    fn trigger_remove(&mut self, trigger: i64) -> Result<Status, Invalid> {
        if self.triggers.contains_key(&trigger) {
            self.removed_triggers.insert(trigger);
            Ok(Status::Ok)
        } else {
            Ok(Status::Gone)
        }
    }

    fn rules_set(&mut self, section: &str, key: &str, value: &str) -> Result<Status, Invalid> {
        self.rules
            .push((section.to_owned(), key.to_owned(), value.to_owned()));
        Ok(Status::Ok)
    }
}

fn usage() -> &'static str {
    "usage: vera-mission-engine-check <module.wasm> [--frames]\n\n"
}

fn print_step(name: &str, used: u64, memory: usize) {
    println!("{name}: {used} operations, memory={memory} bytes");
}

fn call_once(
    module: &mut vera20k_wasm_ext::SimModule,
    host: &mut FixtureHost,
    export: Export,
) -> Result<u64, String> {
    match module.call(export, ONCE_BUDGET, host) {
        Ok(used) => {
            print_step(export.name(), used, module.memory_len());
            Ok(used)
        }
        Err(Fault::Budget) => {
            eprintln!(
                "{}: BUDGET EXCEEDED (limit={} operations; exact attempted total is not exposed by SimModule::call)",
                export.name(),
                ONCE_BUDGET
            );
            Err(format!(
                "{} failed under the real map one-shot budget {}: exceeded its operation budget",
                export.name(),
                ONCE_BUDGET
            ))
        }
        Err(error) => Err(format!(
            "{} failed under the real map one-shot budget {}: {error}",
            export.name(),
            ONCE_BUDGET
        )),
    }
}

fn run(path: &str, full: bool) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    let mut module = vera20k_wasm_ext::SimModule::load(&bytes, Limits::MAP)
        .map_err(|error| format!("load failed: {error}"))?;
    let mut host = FixtureHost::default();

    println!("module: {path}");
    println!(
        "limits: memory <= {MAP_MEMORY_BYTES} bytes; one-shot budget <= {ONCE_BUDGET}; frame/event budget <= {FRAME_BUDGET}"
    );
    println!("loaded memory: {} bytes", module.memory_len());
    if module.memory_len() > MAP_MEMORY_BYTES {
        return Err(format!(
            "module memory exceeds Limits::MAP: {}",
            module.memory_len()
        ));
    }

    // Keep the order used by the real rules layer. A failure is reported as
    // a production-budget failure and returns non-zero; no larger budget is
    // attempted as a fallback.
    call_once(&mut module, &mut host, Export::Init)?;
    call_once(&mut module, &mut host, Export::Rules)?;
    call_once(&mut module, &mut host, Export::ScenarioStart)?;

    if !full {
        println!("init/rules/start acceptance passed");
        return Ok(());
    }

    let mut max_frame = 0;
    let mut event_calls = 0u32;
    for frame in 1..=FRAME_COUNT {
        host.frame = frame;
        let used = module
            .call(Export::Frame, FRAME_BUDGET, &mut host)
            .map_err(|error| {
                format!("on_frame at frame {frame} failed under budget {FRAME_BUDGET}: {error}")
            })?;
        max_frame = max_frame.max(used);
        println!(
            "on_frame[{frame}]: {used} operations, memory={} bytes",
            module.memory_len()
        );

        let events = host.timer_events(frame);
        if !events.is_empty() {
            let used = module
                .deliver(&events, FRAME_BUDGET, &mut host)
                .map_err(|error| {
                    if error == Fault::Budget {
                        eprintln!(
                            "on_events[{frame}]: BUDGET EXCEEDED (limit={FRAME_BUDGET} operations; exact attempted total unavailable)"
                        );
                    }
                    format!("on_events at frame {frame} failed under budget {FRAME_BUDGET}: {error}")
                })?;
            event_calls += 1;
            println!(
                "on_events[{frame}]: {} records, {used} operations",
                events.len()
            );
            host.complete_timer_events(&events);
        }

        if frame == 300 {
            let state = module.state();
            module = vera20k_wasm_ext::SimModule::restore(&bytes, Limits::MAP, &state)
                .map_err(|error| format!("restore at frame 300 failed: {error}"))?;
            println!(
                "snapshot: restored at frame 300, memory={} bytes",
                module.memory_len()
            );
        }
    }

    host.check_runtime_state()?;
    if event_calls != 41 {
        return Err(format!(
            "expected 41 timer event calls, observed {event_calls}"
        ));
    }
    println!(
        "full acceptance passed: frames={FRAME_COUNT}, event_calls={event_calls}, max_frame_operations={max_frame}, logs={}, rules={}",
        host.logs.len(),
        host.rules.len()
    );
    Ok(())
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(path) = args.next() else {
        eprint!("{}missing module path", usage());
        return ExitCode::from(2);
    };
    let mut full = false;
    for arg in args {
        match arg.as_str() {
            "--frames" => full = true,
            "-h" | "--help" => {
                print!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}\n{}", usage());
                return ExitCode::from(2);
            }
        }
    }
    match run(&path, full) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ERROR: {error}");
            ExitCode::from(1)
        }
    }
}
