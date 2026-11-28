import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const upstream = dirname(require.resolve('@vera20k/sdk/package.json'));

function linkPackage(source, destination) {
  mkdirSync(dirname(destination), { recursive: true });
  symlinkSync(source, destination, process.platform === 'win32' ? 'junction' : 'dir');
}

function compile(t, source) {
  const directory = mkdtempSync(join(tmpdir(), 'vera-mission-api-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  mkdirSync(join(directory, 'src'));
  // Request on_frame explicitly: these tests exercise SDK behavior through the
  // upstream CLI; timer-only lifecycle discovery belongs to the builder tests.
  writeFileSync(join(directory, 'src/mission.ts'), source + `
import { Every, onFrame } from '@vera20k/sdk';
onFrame(new Every(1), (): void => {});
`);
  linkPackage(root, join(directory, 'node_modules/@ra2-map-editor/mission-sdk'));
  linkPackage(upstream, join(directory, 'node_modules/@vera20k/sdk'));
  writeFileSync(join(directory, 'vera.json'), JSON.stringify({ kind: 'rules', entry: 'src/mission.ts', vars: [], notify: [] }));
  const output = join(directory, 'rules.wasm');
  execFileSync(process.execPath, [join(upstream, 'bin/vera20k.js'), 'build', directory, '--out', output], { stdio: 'pipe' });
  return new WebAssembly.Module(readFileSync(output));
}

function run(module, frames) {
  let frame = 0;
  let instance;
  const lines = [];
  instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': {
    frame: () => frame,
    log: (ptr, length) => lines.push([frame, Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')]),
  } });
  instance.exports.vera_init();
  instance.exports.on_scenario_start();
  for (frame of frames) instance.exports.on_frame?.();
  return lines;
}

for (const timer of ['setTimeout', 'setInterval']) {
  test(`${timer} can cancel earlier timers without postponing another due callback`, t => {
    const module = compile(t, `
import { clearInterval, clearTimeout, console, onScenarioStart, setInterval, setTimeout, TimerId } from '@ra2-map-editor/mission-sdk';
let previous: TimerId = 0;
let current: TimerId = 0;
function unused(): void { console.log('unexpected'); }
function remove(): void {
  console.log('remove');
  clearTimeout(previous);
  clearInterval(current);
}
function remaining(): void { console.log('remaining'); }
onScenarioStart((): void => {
  previous = setInterval(unused, 100);
  current = ${timer}(remove, 1);
  setTimeout(remaining, 1);
});
`);
    assert.deepEqual(run(module, [1, 2]), [[1, 'remove'], [1, 'remaining']]);
  });
}

test('timer cancellation, minimum delays, aliases, and new registrations respect frame boundaries', t => {
  const module = compile(t, `
import { clearInterval, clearTimeout, console, onScenarioStart, setInterval as repeat, setTimeout as after, TimerId } from '@ra2-map-editor/mission-sdk';
let cancelled: TimerId = 0;
let repeating: TimerId = 0;
let count: i32 = 0;
function unexpected(): void { console.log('unexpected'); }
function created(): void { console.log('created'); }
function first(): void {
  console.log('first');
  clearInterval(cancelled);
  after(created, 0);
}
function second(): void { console.log('second'); }
function interval(): void {
  console.log('interval');
  count++;
  if (count == 2) clearTimeout(repeating);
}
onScenarioStart((): void => {
  after(first, -10);
  after(second, 0);
  cancelled = after(unexpected, 1);
  repeating = repeat(interval, 0);
});
`);
  const expected = [[1, 'first'], [1, 'second'], [1, 'interval'], [2, 'interval'], [2, 'created']];
  assert.deepEqual(run(module, [0, 1, 2, 3]), expected);
  assert.deepEqual(run(module, [0, 1, 2, 3]), expected, 'new module instances must start with independent timer state');
});

// Full catalog snapshot, audited against the local original rulesmd.ini type
// lists and each section's Name/UIName, with stock_names_zh.tsv cross-checks.
// Keep these expectations independent of the implementation: packaged SDK tests
// must not require an engine checkout or installed retail game assets.
const stockCatalog = {
  UnitType: [
    ['Rifleman', 'E1', 'GI (generic infantry alias)'],
    ['Conscript', 'E2', 'Conscript'],
    ['GI', 'E1', 'GI'],
    ['Engineer', 'ENGINEER', 'Engineer'],
    ['Spy', 'SPY', 'Spy'],
    ['Seal', 'GHOST', 'SEAL'],
    ['Tanya', 'TANY', 'Tanya'],
    ['YuriPrime', 'YURIPR', 'Yuri Prime'],
    ['Initiate', 'INIT', 'Yuri Initiate'],
    ['Brute', 'BRUTE', 'Yuri Brute'],
    ['GrizzlyTank', 'MTNK', 'Grizzly Battle Tank'],
    ['RhinoTank', 'HTNK', 'Rhino Heavy Tank'],
    ['ApocalypseTank', 'APOC', 'Apocalypse'],
    ['FlakTrack', 'HTK', 'Flak Track'],
    ['InfantryFightingVehicle', 'FV', 'IFV'],
    ['V3RocketLauncher', 'V3', 'V3 Launcher'],
    ['TerrorDrone', 'DRON', 'Terror Drone'],
    ['LasherTank', 'LTNK', 'Lasher Light Tank'],
    ['Magnetron', 'TELE', 'Magnetron'],
    ['Mastermind', 'MIND', 'Master Mind'],
    ['FloatingDisc', 'DISK', 'Floating Disk'],
    // ORCA's internal Name is Intruder; its stock UI name is Harrier.
    ['Harrier', 'ORCA', 'Intruder / Harrier'],
    ['BlackEagle', 'BEAG', 'Black Eagle'],
    ['Rocketeer', 'JUMPJET', 'Rocketeer'],
    ['NightHawk', 'SHAD', 'BlackHawk Transport / Nighthawk Transport'],
    ['Kirov', 'ZEP', 'Kirov Airship'],
    ['Dreadnought', 'DRED', 'Dreadnought'],
    ['Destroyer', 'DEST', 'Destroyer'],
    ['AegisCruiser', 'AEGIS', 'Aegis Cruiser'],
    ['Dolphin', 'DLPH', 'Dolphin'],
    ['GiantSquid', 'SQD', 'Giant Squid'],
    ['AttackSubmarine', 'SUB', 'Typhoon Attack Sub'],
  ],
  BuildingType: [
    ['AlliedConstructionYard', 'GACNST', 'Allied Construction Yard'],
    ['AlliedPowerPlant', 'GAPOWR', 'Allied Power Plant'],
    ['AlliedBarracks', 'GAPILE', 'Allied Barracks'],
    ['AlliedWarFactory', 'GAWEAP', 'Allied War Factory'],
    ['AlliedOreRefinery', 'GAREFN', 'Allied Ore Refinery'],
    ['AlliedServiceDepot', 'GADEPT', 'Allied Service Depot'],
    ['AlliedBattleLab', 'GATECH', 'Allied Battle Lab'],
    ['AlliedAirforceCommand', 'GAAIRC', 'Allied Airforce Command Headquarters'],
    ['AlliedSpySat', 'GASPYSAT', 'Allied SpySat Uplink'],
    ['SovietConstructionYard', 'NACNST', 'Soviet Construction Yard'],
    ['SovietPowerPlant', 'NAPOWR', 'Soviet Tesla Reactor'],
    ['SovietBarracks', 'NAHAND', 'Soviet Barracks'],
    ['SovietWarFactory', 'NAWEAP', 'Soviet War Factory'],
    ['SovietOreRefinery', 'NAREFN', 'Soviet Ore Refinery'],
    ['SovietServiceDepot', 'NADEPT', 'Soviet Service Depot'],
    ['SovietBattleLab', 'NATECH', 'Soviet Battle Lab'],
    ['SovietRadar', 'NARADR', 'Soviet Radar Tower'],
    ['YuriConstructionYard', 'YACNST', 'Yuri Construction Yard'],
    ['YuriBioReactor', 'YAPOWR', 'Yuri Bio Reactor'],
    ['YuriBarracks', 'YABRCK', 'Yuri Barracks'],
    ['YuriWarFactory', 'YAWEAP', 'Yuri War Factory'],
    // A real stock building: the deployed SMIN, not the unused YADEPT stub.
    ['YuriOreRefinery', 'YAREFN', 'Yuri Ore Refinery / deployed Slave Miner'],
    ['YuriBattleLab', 'YATECH', 'Yuri Battle Lab'],
    ['YuriGattlingCannon', 'YAGGUN', 'Yuri Gattling Cannon'],
  ],
};

test('every stock constant exposes Chinese JSDoc through TypeScript symbol documentation', () => {
  const ts = require('typescript');
  const filename = join(root, 'assembly/mission.ts');
  const program = ts.createProgram([filename], { noEmit: true, skipLibCheck: true });
  const source = program.getSourceFile(filename);
  const checker = program.getTypeChecker();
  assert.ok(source);
  for (const [namespace, rows] of Object.entries(stockCatalog)) {
    const declaration = source.statements.find(node => ts.isModuleDeclaration(node) && node.name.text === namespace);
    assert.ok(declaration && ts.isModuleBlock(declaration.body), `missing ${namespace}`);
    const constants = declaration.body.statements.filter(ts.isVariableStatement)
      .flatMap(statement => [...statement.declarationList.declarations]);
    for (const [member] of rows) {
      const constant = constants.find(node => ts.isIdentifier(node.name) && node.name.text === member);
      assert.ok(constant, `missing ${namespace}.${member}`);
      const symbol = checker.getSymbolAtLocation(constant.name);
      assert.ok(symbol, `missing symbol for ${namespace}.${member}`);
      const documentation = ts.displayPartsToString(symbol.getDocumentationComment(checker));
      assert.match(documentation, /\p{Script=Han}/u, `${namespace}.${member} needs Chinese hover documentation`);
    }
  }
});

for (const [namespace, rows] of Object.entries(stockCatalog)) {
  test(`${namespace} exactly matches the audited complete stock catalog`, t => {
    const source = readFileSync(join(root, 'assembly/mission.ts'), 'utf8');
    const body = source.match(new RegExp(`export namespace ${namespace} \\{([\\s\\S]*?)\\n\\}`))?.[1];
    assert.ok(body, `missing ${namespace} namespace`);
    const actual = Object.fromEntries([...body.matchAll(/export const (\w+): string = "([^"]+)";/g)]
      .map(([, member, id]) => [member, id]));
    assert.deepEqual(Object.keys(actual).sort(), rows.map(([member]) => member).sort(),
      'the table must cover every supported stock constant and valid alias');
    for (const [member, id, name] of rows) {
      assert.equal(actual[member], id, `${namespace}.${member} must resolve ${name} [${id}]`);
    }
    const statements = rows.map(([member]) => `console.log('${member}=' + ${namespace}.${member});`).join('\n');
    const module = compile(t, `
import { console, onScenarioStart, ${namespace} } from '@ra2-map-editor/mission-sdk';
onScenarioStart((): void => {
${statements}
});
`);
    assert.deepEqual(run(module, []), rows.map(([member, id]) => [0, `${member}=${id}`]));
  });
}

test('top-level timers and noncapturing callbacks run once per due frame without catch-up bursts', t => {
  const module = compile(t, `
import { console, onScenarioStart, setInterval, setTimeout } from '@ra2-map-editor/mission-sdk';
setTimeout((): void => { console.log('top-level'); }, 2);
function interval(): void { console.log('interval'); }
function once(): void { console.log('once'); }
onScenarioStart((): void => {
  setInterval(interval, 3);
  setTimeout(once, 5);
});
`);
  assert.deepEqual(run(module, [1, 2, 10, 11, 13, 20]), [[2, 'top-level'], [10, 'interval'], [10, 'once'], [13, 'interval'], [20, 'interval']]);
});

test('console methods emit deterministic UTF-8 log lines and assertion prefixes', t => {
  const module = compile(t, `
import { console, onScenarioStart } from '@ra2-map-editor/mission-sdk';
onScenarioStart((): void => {
  console.log('中文日志');
  console.info('info');
  console.debug('debug');
  console.warn('warn');
  console.error('error');
  console.assert(true, 'must stay silent');
  console.assert(false, 'failed');
  console.assert(false);
});
`);
  assert.deepEqual(run(module, []), [[0, '中文日志'], [0, 'info'], [0, '[debug] debug'], [0, '[warn] warn'], [0, '[error] error'], [0, '[error] failed'], [0, '[error] Assertion failed']]);
});


test('128 simultaneous mixed timers retain order through compaction and later registration', t => {
  const module = compile(t, `
import { clearInterval, console, onScenarioStart, setInterval, setTimeout, TimerId } from '@ra2-map-editor/mission-sdk';
let repeating = new Array<TimerId>();
function once(): void { console.log('once'); }
function repeat(): void { console.log('repeat'); }
function cancel(): void {
  for (let i = 0; i < repeating.length; i++) clearInterval(repeating[i]);
  setTimeout(once, 0);
}
onScenarioStart((): void => {
  for (let i = 0; i < 64; i++) {
    setTimeout(once, 1);
    repeating.push(setInterval(repeat, 1));
  }
  setTimeout(cancel, 2);
});
`);
  const first = Array.from({ length: 64 }, () => [[1, 'once'], [1, 'repeat']]).flat();
  const second = Array.from({ length: 64 }, () => [2, 'repeat']);
  assert.deepEqual(run(module, [1, 2, 3, 4]), [...first, ...second, [3, 'once']]);
});
