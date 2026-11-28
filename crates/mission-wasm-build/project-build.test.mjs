import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { execFile, execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, toNamespacedPath } from 'node:path';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const repository = resolve(root, '../..');
const facadeSource = join(repository, 'crates/vera20k-sdk');
const fixture = join(root, 'engine-check/fixture');
const require = createRequire(import.meta.url);
const compiler = dirname(require.resolve('assemblyscript/package.json'));
const enginePackage = '@vera20k/sdk';
const facadePackage = '@ra2-map-editor/mission-sdk';

function scratch(t) {
  const directory = mkdtempSync(join(tmpdir(), 'vera-builder-regression-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function linkDirectory(source, destination) {
  mkdirSync(dirname(destination), { recursive: true });
  symlinkSync(source, destination, process.platform === 'win32' ? 'junction' : 'dir');
}

function makeProject(directory, name = 'project') {
  const project = join(directory, name);
  cpSync(fixture, project, { recursive: true });
  return project;
}

function sdkCopy(destination, name = enginePackage) {
  const source = name === enginePackage ? dirname(require.resolve(`${enginePackage}/package.json`)) : facadeSource;
  cpSync(source, destination, {
    recursive: true,
    filter: path => !relative(source, path).split(/[\\/]/).includes('node_modules'),
  });
}

function installSdks(directory) {
  const engine = join(directory, 'node_modules', enginePackage);
  const facade = join(directory, 'node_modules', facadePackage);
  sdkCopy(engine);
  sdkCopy(facade, facadePackage);
  return { engine, facade };
}

function builderCopy(directory) {
  const builder = join(directory, 'builder');
  mkdirSync(builder);
  for (const file of ['project-build.mjs', 'package.json']) cpSync(join(root, file), join(builder, file));
  linkDirectory(compiler, join(directory, 'node_modules/assemblyscript'));
  return join(builder, 'project-build.mjs');
}

function build(builder, project, output, extra = [], env = process.env) {
  return execFileSync(process.execPath, [builder, project, output, ...extra], { stdio: 'pipe', env: { ...env, NODE_PATH: '' } });
}

function assertWasm(output) {
  const module = new WebAssembly.Module(readFileSync(output));
  const imports = WebAssembly.Module.imports(module);
  assert.ok(imports.length > 0);
  assert.ok(imports.every(item => item.module === 'vera:sim@1.0'));
  assert.ok(WebAssembly.Module.exports(module).some(item => item.name === 'vera_init'));
  return module;
}

test('builds using the installed upstream SDK and workspace facade without modifying either package', t => {
  const directory = scratch(t);
  const project = makeProject(directory);
  const engine = dirname(require.resolve(`${enginePackage}/package.json`));
  const paths = [join(engine, 'assembly/index.ts'), join(engine, 'bin/vera20k.js'), join(engine, 'bin/transform.js'),
    join(facadeSource, 'index.ts'), join(facadeSource, 'assembly/mission.ts')];
  const before = paths.map(path => readFileSync(path));
  const output = join(directory, 'rules.wasm');
  build(join(root, 'project-build.mjs'), project, output);
  assertWasm(output);
  paths.forEach((path, i) => assert.deepEqual(readFileSync(path), before[i], 'installed SDK packages must remain unchanged'));
});

test('builds canonicalized Windows project and output paths directly', { skip: process.platform !== 'win32' }, t => {
  const directory = scratch(t);
  const project = makeProject(directory);
  const output = join(directory, 'canonicalized output', 'rules.wasm');
  const projectArgument = toNamespacedPath(project);
  assert.ok(projectArgument.startsWith('\\\\?\\'));
  build(join(root, 'project-build.mjs'), projectArgument, toNamespacedPath(output));
  assertWasm(output);
});


// The same strict package shape is used at the selected project and at an
// independent rules/ui child's parent. A bad vera field cannot hide a joint
// project by making it look like a standalone module.
const invalidPackages = [
  { name: 'malformed JSON', contents: '{', message: /must contain valid JSON/ },
  ...[null, [], 'package', false, 0].map(value => ({
    name: `non-object package ${JSON.stringify(value)}`, contents: JSON.stringify(value), message: /must be a JSON object/,
  })),
  ...[null, [], 'extension', false, true, 0].map(vera => ({
    name: `non-object vera ${JSON.stringify(vera)}`, contents: JSON.stringify({ vera }), message: /vera must be a JSON object/,
  })),
  ...[{}, { kind: 'rules' }, { kind: 'ui' }, { kind: 'unknown' }, { kind: null }, { kind: [] }, { kind: {} }].map(vera => ({
    name: `invalid vera kind ${JSON.stringify(vera)}`, contents: JSON.stringify({ vera }), message: /vera.kind must be "extension"/,
  })),
];

for (const location of ['project', 'parent of rules', 'parent of ui']) {
  test(`rejects invalid package.json at ${location} instead of falling back to a single module`, async t => {
    for (const scenario of invalidPackages) {
      await t.test(scenario.name, t => {
        const directory = scratch(t);
        const child = location === 'project' ? 'project' : location.split(' ').at(-1);
        const project = makeProject(directory, child);
        const manifest = join(location === 'project' ? project : directory, 'package.json');
        writeFileSync(manifest, scenario.contents);
        const output = join(directory, 'selected.wasm');
        const previous = Buffer.from('previous standalone output');
        writeFileSync(output, previous);
        assert.throws(() => build(join(root, 'project-build.mjs'), project, output), error => {
          assert.match(String(error.stderr), scenario.message);
          assert.ok(String(error.stderr).includes(location === 'project' ? 'package.json' : '../package.json'));
          return true;
        });
        assert.deepEqual(readFileSync(output), previous);
        assert.equal(existsSync(join(project, 'src/generated')), false, 'reject before SDK generation');
        assert.ok(!readdirSync(directory).some(name => name.startsWith('.ra2-mission-build-')));
      });
    }
  });
}

for (const kind of ['rules', 'ui']) {
  for (const withPackage of [false, true]) {
    test(`ordinary ${kind} child with parent package lacking vera builds standalone ${withPackage ? 'with' : 'without'} its own package`, t => {
      const directory = scratch(t);
      const parent = join(directory, '普通父目录 with spaces');
      mkdirSync(parent);
      writeFileSync(join(parent, 'package.json'), JSON.stringify({ name: 'ordinary-parent', private: true }));
      const project = join(parent, kind);
      cpSync(join(root, 'engine-check', kind === 'ui' ? 'ui-fixture' : 'fixture'), project, { recursive: true });
      if (withPackage) writeFileSync(join(project, 'package.json'), JSON.stringify({ name: 'standalone-child', private: true }));
      // An ordinary parent must not cause sibling validation or generation.
      const sibling = join(parent, kind === 'ui' ? 'rules' : 'ui');
      mkdirSync(sibling);
      writeFileSync(join(sibling, 'vera.json'), '{}');
      const output = join(directory, `${kind}.wasm`);
      build(join(root, 'project-build.mjs'), project, output);
      (kind === 'ui' ? assertUiWasm : assertWasm)(output);
      assert.equal(existsSync(join(sibling, 'src/generated')), false);
    });
  }
}

test('project SDKs win over builder and ancestor shadows without changing installed sources', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  const selected = installSdks(project);
  const ancestor = installSdks(directory);
  const bundled = installSdks(dirname(builder));
  writeFileSync(join(ancestor.engine, 'assembly/index.ts'), 'export * from "./must-not-use-this-ancestor";\n');
  writeFileSync(join(ancestor.facade, 'index.ts'), 'export * from "./must-not-use-this-ancestor";\n');
  writeFileSync(join(bundled.engine, 'assembly/index.ts'), 'export * from "./must-not-use-this-builder";\n');
  writeFileSync(join(bundled.facade, 'index.ts'), 'export * from "./must-not-use-this-builder";\n');
  const paths = [join(selected.engine, 'assembly/index.ts'), join(selected.facade, 'index.ts'),
    join(ancestor.engine, 'assembly/index.ts'), join(ancestor.facade, 'index.ts'),
    join(bundled.engine, 'assembly/index.ts'), join(bundled.facade, 'index.ts')];
  const before = paths.map(path => readFileSync(path));
  const output = join(directory, 'rules.wasm');
  build(builder, project, output);
  assertWasm(output);
  paths.forEach((path, i) => assert.deepEqual(readFileSync(path), before[i]));
});

test('npm tarballs scaffold, type-check and build a joint extension and its independent children', t => {
  const directory = scratch(t);
  const npmCli = process.env.npm_execpath || join(dirname(require.resolve('npm/package.json', { paths: [dirname(process.execPath)] })), 'bin/npm-cli.js');
  const project = join(directory, 'project');
  function npm(args, cwd = directory) {
    return execFileSync(process.execPath, [npmCli, ...args], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '' },
    });
  }
  function pack(source) {
    const [packed] = JSON.parse(npm(['pack', '--offline', '--json', '--pack-destination', directory, source]));
    return { ...packed, tarball: join(directory, packed.filename) };
  }
  const facade = pack(facadeSource);
  const builder = pack(root);
  const scaffolder = pack(join(repository, 'crates/create-vera-app'));
  assert.ok(scaffolder.files.some(file => file.path === 'cli.mjs'));
  const unpacked = join(directory, 'scaffolder');
  mkdirSync(unpacked);
  execFileSync('tar', ['-xf', scaffolder.tarball, '-C', unpacked], { stdio: 'pipe' });
  execFileSync(process.execPath, [join(unpacked, 'package/cli.mjs'), project, '--packages-dir', directory], { stdio: 'pipe' });
  assert.deepEqual(JSON.parse(readFileSync(join(project, 'package.json'), 'utf8')).vera, { kind: 'extension' });
  const config = JSON.parse(readFileSync(join(project, 'tsconfig.json'), 'utf8'));
  assert.equal(config.compilerOptions?.paths?.[enginePackage], undefined, 'the canonical engine alias must not point back to the facade');
  assert.ok(facade.files.some(file => file.path === 'assembly/mission.ts'));
  assert.ok(builder.files.some(file => file.path === 'project-build.mjs'));
  npm(['install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--package-lock=false', facade.tarball, builder.tarball], project);
  const projectRequire = createRequire(join(project, 'package.json'));
  for (const name of [enginePackage, facadePackage, '@ra2-map-editor/mission-wasm-build']) {
    const manifestPath = projectRequire.resolve(`${name}/package.json`);
    assert.ok(!relative(project, manifestPath).startsWith('..'), `${name} must resolve inside the independent project`);
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    assert.equal(manifest.name, name);
    assert.equal(manifest.version, name === enginePackage ? '0.1.0' : '0.1.3');
  }
  npm(['run', 'check'], project);
  for (const extra of [['--', '--debug'], []]) {
    npm(['run', 'build', ...extra], project);
    for (const built of ['rules', 'ui']) (built === 'ui' ? assertUiWasm : assertWasm)(join(project, `dist/${built}.wasm`));
  }
  const installedBuilder = projectRequire.resolve('@ra2-map-editor/mission-wasm-build/project-build.mjs');
  for (const kind of ['rules', 'ui']) {
    const output = join(project, `dist/${kind}.wasm`);
    build(installedBuilder, join(project, kind), output, ['--debug']);
    (kind === 'ui' ? assertUiWasm : assertWasm)(output);
    assert.equal(existsSync(join(project, kind, 'package.json')), false);
    assert.equal(existsSync(join(project, kind, 'tsconfig.json')), false);
  }
  assert.equal(existsSync(join(project, 'vera.json')), false);
  assert.equal(existsSync(join(project, 'vendor')), false);
});

for (const name of [enginePackage, facadePackage]) {
  test(`an isolated builder diagnoses missing ${name} without a repository fallback`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeProject(directory);
    const other = name === enginePackage ? facadePackage : enginePackage;
    sdkCopy(join(project, 'node_modules', other), other);
    const output = join(directory, 'rules.wasm');
    assert.throws(() => build(builder, project, output), error => {
      assert.ok(String(error.stderr).includes(name), 'diagnostic must identify the missing dependency');
      assert.doesNotMatch(String(error.stderr), /ENOENT|copyfile/);
      return true;
    });
    assert.equal(existsSync(output), false);
  });

  for (const [field, value] of [['name', '@unexpected/sdk'], ['version', '0.0.0']]) {
    test(`an isolated builder rejects ${name} with the wrong ${field}`, t => {
      const directory = scratch(t);
      const builder = builderCopy(directory);
      const project = makeProject(directory);
      installSdks(project);
      const manifestPath = join(project, 'node_modules', name, 'package.json');
      const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
      manifest[field] = value;
      writeFileSync(manifestPath, JSON.stringify(manifest));
      const output = join(directory, 'rules.wasm');
      assert.throws(() => build(builder, project, output), error => {
        assert.ok(String(error.stderr).includes(name), 'diagnostic must identify the invalid dependency');
        return true;
      });
      assert.equal(existsSync(output), false);
    });
  }
}

for (const location of ['project', 'builder', 'hoisted']) {
  test(`resolves the engine and facade from a ${location} installation`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeProject(directory);
    const parent = location === 'project' ? project : location === 'builder' ? dirname(builder) : directory;
    installSdks(parent);
    const output = join(directory, 'rules.wasm');
    build(builder, project, output);
    assertWasm(output);
  });
}

for (const timer of ['setTimeout', 'setInterval']) {
  test(`bundled ${timer}-only builders preserve aliased facade imports, dependencies and asconfig`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const { engine, facade } = installSdks(dirname(builder));
    const paths = [join(engine, 'bin/vera20k.js'), join(engine, 'bin/transform.js'), join(facade, 'assembly/mission.ts')];
    const before = paths.map(path => readFileSync(path));
    const project = makeProject(directory, '中文项目 with spaces');
    writeFileSync(join(project, 'asconfig.json'), JSON.stringify({ options: { initialMemory: 4 } }));
    writeFileSync(join(project, 'src/helper.ts'), 'export function answer(): i32 { return 42; }\n');
    const dependency = join(project, 'node_modules/mission-helper');
    mkdirSync(dependency, { recursive: true });
    writeFileSync(join(dependency, 'index.ts'), 'export function label(): string { return "external helper"; }\n');
    writeFileSync(join(project, 'src/mission.ts'), `
import { onScenarioStart } from '@vera20k/sdk';
import { console as missionConsole, ${timer} as afterFrames } from '@ra2-map-editor/mission-sdk';
import { answer } from './helper';
import { label } from 'mission-helper';
function later(): void { missionConsole.log(answer().toString()); }
function start(): void { missionConsole.info(label()); afterFrames(later, 2); }
onScenarioStart(start);
`);
    const output = join(directory, 'rules.wasm');
    build(builder, project, output);
    const module = assertWasm(output);
    assert.ok(WebAssembly.Module.exports(module).some(item => item.name === 'on_frame'), 'timer-only missions need on_frame even without onFrame calls');
    let frame = 0;
    let instance;
    const lines = [];
    const host = {
      frame: () => frame,
      log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
    };
    instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': host });
    assert.ok(instance.exports.memory.buffer.byteLength >= 4 * 65536, 'must honor project asconfig');
    instance.exports.vera_init();
    instance.exports.on_scenario_start();
    for (frame = 1; frame <= 5; frame++) instance.exports.on_frame();
    assert.deepEqual(lines, timer === 'setInterval' ? ['external helper', '42', '42'] : ['external helper', '42']);
    paths.forEach((path, i) => assert.deepEqual(readFileSync(path), before[i], 'installed packages must not be patched'));
  });
}

for (const { file, marker, message } of [
  { file: 'bin/vera20k.js', marker: 'const result = await asc.main(args, {', message: /Unsupported engine SDK compiler entry/ },
  { file: 'bin/vera20k.js', marker: '"--path", sdkPath(),', message: /Unsupported engine SDK module search path/ },
  { file: 'bin/transform.js', marker: 'on_frame: [/\\bonFrame\\s*\\(/],', message: /Unsupported engine SDK lifecycle transform/ },
  { file: 'bin/transform.js', marker: '    for (const [name, patterns] of Object.entries(NEEDS)) {', message: /Unsupported engine SDK lifecycle transform/ },
]) {
  for (const copies of [0, 2]) {
    test(`rejects ${copies} pinned hook matches in ${file} before compiling`, t => {
      const directory = scratch(t);
      const builder = builderCopy(directory);
      const project = makeProject(directory);
      const { engine } = installSdks(project);
      const path = join(engine, file);
      const original = readFileSync(path, 'utf8');
      assert.equal(original.split(marker).length, 2, 'the published pinned SDK must have exactly one supported hook');
      const unsupported = copies === 0 ? original.replace(marker, '// unsupported hook') : `${original}\n// ${marker}\n`;
      writeFileSync(path, unsupported);
      const output = join(directory, 'rules.wasm');
      const previous = Buffer.from('previous successfully validated artifact');
      writeFileSync(output, previous);
      assert.throws(() => build(builder, project, output), error => {
        assert.match(String(error.stderr), message);
        return true;
      });
      assert.deepEqual(readFileSync(output), previous);
      assert.equal(readFileSync(path, 'utf8'), unsupported, 'only the disposable SDK copy may be patched');
    });
  }
}

test('a compile failure preserves previous output and removes private staging', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  installSdks(project);
  writeFileSync(join(project, 'src/mission.ts'), 'export function broken(): i32 { return noSuchValue; }\n');
  const output = join(directory, 'rules.wasm');
  const previous = Buffer.from('previous successfully validated artifact');
  writeFileSync(output, previous);
  const temporary = join(directory, 'compiler-temporary');
  mkdirSync(temporary);
  assert.throws(() => build(builder, project, output, [], { ...process.env, TEMP: temporary, TMP: temporary, TMPDIR: temporary }), error => {
    assert.match(String(error.stderr), /noSuchValue/);
    return true;
  });
  assert.deepEqual(readFileSync(output), previous);
  assert.deepEqual(readdirSync(temporary), []);
});


test('private staging dereferences linked engine directories without changing their targets', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  const { engine } = installSdks(project);
  const originalBin = join(directory, 'immutable-engine-bin');
  const bin = join(engine, 'bin');
  cpSync(bin, originalBin, { recursive: true });
  assert.ok(relative(directory, bin).startsWith('project'));
  rmSync(bin, { recursive: true, force: true });
  linkDirectory(originalBin, bin);
  const paths = ['vera20k.js', 'transform.js'].map(file => join(originalBin, file));
  const before = paths.map(path => readFileSync(path));
  const output = join(directory, 'rules.wasm');
  build(builder, project, output);
  assertWasm(output);
  paths.forEach((path, index) => assert.deepEqual(readFileSync(path), before[index], 'a linked source must remain immutable'));
});

test('rejects an unpinned compiler before compiling or replacing an existing output', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  installSdks(project);
  const installed = join(directory, 'node_modules/assemblyscript');
  assert.equal(dirname(dirname(installed)), directory);
  rmSync(installed, { recursive: true, force: true });
  mkdirSync(installed);
  const manifest = JSON.parse(readFileSync(join(compiler, 'package.json'), 'utf8'));
  manifest.version = '0.28.19';
  writeFileSync(join(installed, 'package.json'), JSON.stringify(manifest));
  for (const entry of ['dist', 'lib', 'std', 'util', 'bin']) {
    if (existsSync(join(compiler, entry))) linkDirectory(join(compiler, entry), join(installed, entry));
  }
  const output = join(directory, 'rules.wasm');
  const before = Buffer.from('previous validated artifact');
  writeFileSync(output, before);
  assert.throws(() => build(builder, project, output), error => {
    assert.match(String(error.stderr), /assemblyscript@0\.28\.20.*assemblyscript@0\.28\.19/);
    return true;
  });
  assert.deepEqual(readFileSync(output), before);
});

for (const timer of ['setTimeout', 'setInterval']) {
  test('dependency-hidden ' + timer + ' retains and executes the frame hook', t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    installSdks(dirname(builder));
    const project = makeProject(directory);
    const scheduler = join(project, 'node_modules/mission-scheduler');
    mkdirSync(scheduler, { recursive: true });
    writeFileSync(join(scheduler, 'index.ts'), [
      "import { " + timer + " as later } from '@ra2-map-editor/mission-sdk';",
      'export function schedule(callback: () => void): void { later(callback, 2); }',
    ].join('\n'));
    writeFileSync(join(project, 'src/mission.ts'), [
      "import { onScenarioStart } from '@vera20k/sdk';",
      "import { console } from '@ra2-map-editor/mission-sdk';",
      "import { schedule } from 'mission-scheduler';",
      "function callback(): void { console.log('scheduled'); }",
      'onScenarioStart((): void => { schedule(callback); });',
    ].join('\n'));
    const output = join(directory, 'rules.wasm');
    build(builder, project, output);
    const module = assertWasm(output);
    assert.ok(WebAssembly.Module.exports(module).some(item => item.name === 'on_frame'), 'timers in npm dependencies still need on_frame');
    let frame = 0;
    let instance;
    const lines = [];
    const host = {
      frame: () => frame,
      log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
    };
    instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': host });
    instance.exports.vera_init();
    instance.exports.on_scenario_start();
    for (frame = 1; frame <= 5; frame++) instance.exports.on_frame();
    assert.deepEqual(lines, timer === 'setInterval' ? ['scheduled', 'scheduled'] : ['scheduled']);
  });
}

const rulesLifecycles = ['on_scenario_start', 'on_rules', 'on_frame', 'on_events', 'on_decide', 'on_message'];
for (const { hook, imports, registration, handler } of [
  { hook: 'on_scenario_start', imports: 'onScenarioStart as begin', registration: 'begin(handle);', handler: "function handle(): void { console.log('scenario'); }" },
  { hook: 'on_rules', imports: 'onRules as rules', registration: 'rules(handle);', handler: "function handle(): void { console.log('rules'); }" },
  { hook: 'on_frame', imports: 'onFrame as tick, Every', registration: 'tick(new Every(1), handle);', handler: "function handle(): void { console.log('frame'); }" },
  { hook: 'on_events', imports: 'Trigger, EventRecord', registration: 'Trigger.every(2).enqueue<EventRecord>(handle).done();', handler: "function handle(e: EventRecord): void { console.log('event:' + e.value.toString()); }" },
  { hook: 'on_decide', imports: 'Trigger, Event, DecideRecord', registration: 'Trigger.on(Event.FireAllowed).decideSync<DecideRecord>(handle);', handler: "function handle(d: DecideRecord): void { d.setResult(d.previous + 7); console.log('decision'); }" },
  { hook: 'on_message', imports: 'onMessage as receive, House', registration: 'receive(handle);', handler: "function handle(sender: House, data: Uint8Array): void { console.log('message:' + data.length.toString()); }" },
]) {
  test('npm helper retains and executes only the registered rules lifecycle ' + hook, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    installSdks(dirname(builder));
    const project = makeProject(directory);
    const helper = join(project, 'node_modules/mission-lifecycle-helper');
    mkdirSync(helper, { recursive: true });
    writeFileSync(join(helper, 'package.json'), JSON.stringify({ name: 'mission-lifecycle-helper', version: '1.0.0', ascMain: 'index.ts' }));
    writeFileSync(join(helper, 'index.ts'), [
      "import { " + imports + ", console } from '@ra2-map-editor/mission-sdk';",
      handler,
      'export function install(): void { ' + registration + ' }',
    ].join('\n'));
    writeFileSync(join(project, 'src/mission.ts'), "import { install } from 'mission-lifecycle-helper';\ninstall();\n");
    const output = join(directory, 'rules.wasm');
    build(builder, project, output);
    const module = assertWasm(output);
    const exported = WebAssembly.Module.exports(module).map(item => item.name);
    assert.deepEqual(exported.filter(name => rulesLifecycles.includes(name)), [hook]);
    let instance;
    let dispatchKey;
    let triggerCount = 0;
    const lines = [];
    const host = {
      frame: () => 1,
      log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
      trigger_bind: () => assert.fail('these helper triggers have no explicit bindings'),
      trigger_create: (ptr, length, out) => {
        const view = new DataView(instance.exports.memory.buffer);
        // TriggerDef (32), one EventDef (32), then ActionDef.params.id (8).
        if (hook === 'on_events') {
          assert.equal(length, 32 + 32 + 80);
          dispatchKey = view.getInt32(ptr + 32 + 32 + 8, true);
        } else {
          assert.equal(length, 32 + 32);
          dispatchKey = view.getInt32(ptr + 20, true);
        }
        triggerCount++;
        view.setBigInt64(out, 1n, true);
        return 0;
      },
    };
    instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': host });
    instance.exports.vera_init();
    // The SDK starts at memoryBase=1024; this is the reserved host delivery area.
    const ptr = 256;
    const view = new DataView(instance.exports.memory.buffer);
    if (hook === 'on_events' || hook === 'on_decide') {
      assert.equal(triggerCount, 1);
      assert.ok(dispatchKey > 0, 'read the real registered key from trigger_create');
      view.setInt32(ptr, dispatchKey, true);
      if (hook === 'on_events') {
        view.setBigInt64(ptr + 72, 37n, true);
        instance.exports.on_events(ptr, 1);
        assert.deepEqual(lines, ['event:37']);
      } else {
        view.setBigInt64(ptr + 128, 35n, true);
        instance.exports.on_decide(ptr, 1);
        const result = new DataView(instance.exports.memory.buffer);
        assert.equal(result.getBigInt64(ptr + 136, true), 42n);
        assert.equal(result.getInt32(ptr + 144, true), 1, 'decision marked handled');
        assert.deepEqual(lines, ['decision']);
      }
    } else if (hook === 'on_message') {
      instance.exports.on_message(0n, ptr, 3);
      assert.deepEqual(lines, ['message:3']);
    } else {
      instance.exports[hook]();
      assert.deepEqual(lines, [{ on_scenario_start: 'scenario', on_rules: 'rules', on_frame: 'frame' }[hook]]);
    }
  });
}

test('aliased lifecycle imports retain scenario, rules, frame and message exports', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  installSdks(dirname(builder));
  const project = makeProject(directory);
  writeFileSync(join(project, 'src/mission.ts'), [
    "import { onScenarioStart as begin, onRules as rules, onFrame as tick, onMessage as receive, House, Every, console } from '@ra2-map-editor/mission-sdk';",
    "begin((): void => { console.log('begin'); });",
    "rules((): void => { console.log('rules'); });",
    "tick(new Every(1), (): void => { console.log('tick'); });",
    "receive((sender: House, data: Uint8Array): void => { console.log('message'); });",
  ].join('\n'));
  const output = join(directory, 'rules.wasm');
  build(builder, project, output);
  const module = assertWasm(output);
  const exported = WebAssembly.Module.exports(module).map(item => item.name);
  for (const name of ['on_scenario_start', 'on_rules', 'on_frame', 'on_message']) assert.ok(exported.includes(name), name + ' must survive import aliases');
  let frame = 0;
  let instance;
  const lines = [];
  const host = {
    frame: () => frame,
    log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
  };
  instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': host });
  instance.exports.vera_init();
  instance.exports.on_scenario_start();
  instance.exports.on_rules();
  for (frame = 1; frame <= 2; frame++) instance.exports.on_frame();
  instance.exports.on_message(0n, 0, 0);
  assert.deepEqual(lines, ['begin', 'rules', 'tick', 'tick', 'message']);
});


test('a facade import alone does not enable unused frame or rules hooks', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  installSdks(dirname(builder));
  const project = makeProject(directory);
  writeFileSync(join(project, 'src/mission.ts'), [
    "import { console, onScenarioStart } from '@ra2-map-editor/mission-sdk';",
    "onScenarioStart((): void => { console.log('ready'); });",
  ].join('\n'));
  const output = join(directory, 'rules.wasm');
  build(builder, project, output);
  const exported = WebAssembly.Module.exports(assertWasm(output)).map(item => item.name);
  assert.deepEqual(exported.filter(name => rulesLifecycles.includes(name)), ['on_scenario_start'], 'SDK definitions must not force unused lifecycle hooks');
});

test('late action metadata validation preserves output and cleans temporary directories', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  installSdks(project);
  writeFileSync(join(project, 'src/mission.ts'), [
    "import { namedAction } from '@vera20k/sdk';",
    'function action(value: i32): void {}',
    "namedAction<i32>('CaseSensitive', action);",
    "namedAction<i32>('casesensitive', action);",
  ].join('\n'));
  const output = join(directory, 'rules.wasm');
  const previous = Buffer.from('previous validated artifact');
  writeFileSync(output, previous);
  const temporary = join(directory, 'compiler-temporary');
  mkdirSync(temporary);
  assert.throws(() => build(builder, project, output, [], { ...process.env, TEMP: temporary, TMP: temporary, TMPDIR: temporary }), error => {
    assert.match(String(error.stderr), /named action "casesensitive" repeats "CaseSensitive" \(names ignore case\)/);
    return true;
  });
  assert.deepEqual(readFileSync(output), previous);
  assert.deepEqual(readdirSync(temporary), []);
  assert.ok(!readdirSync(directory).some(name => name.startsWith('.vera20k-build-')));
});

test('invalid CLI arguments do not touch existing output or create an alternate output', t => {
  const directory = scratch(t);
  const project = makeProject(directory);
  const output = join(directory, 'rules.wasm');
  const alternate = join(directory, 'alternate.wasm');
  const previous = Buffer.from('previous validated artifact');
  writeFileSync(output, previous);
  for (const args of [[], [project], [project, output, '--release'], [project, output, '--out', alternate], [project, output, '--debug', '--debug']]) {
    assert.throws(() => execFileSync(process.execPath, [join(root, 'project-build.mjs'), ...args], { stdio: 'pipe' }), error => {
      assert.match(String(error.stderr), /usage: node project-build\.mjs/);
      return true;
    });
    assert.deepEqual(readFileSync(output), previous);
    assert.equal(existsSync(alternate), false);
  }
});


const clientLifecycles = ['client_on_start', 'client_on_render', 'client_on_events'];

function makeUiProject(directory) {
  const project = makeProject(directory);
  rmSync(join(project, 'src/mission.ts'));
  writeFileSync(join(project, 'src/ui.ts'), "import { Client, onStart } from '@ra2-map-editor/mission-sdk/client';\nonStart((): void => { Client.log('ui ready'); });\n");
  writeFileSync(join(project, 'vera.json'), JSON.stringify({
    kind: 'ui', entry: 'src/ui.ts', out: 'dist/ui.wasm',
    vars: [{ name: 'Score', scope: 'global' }], notify: ['Notice'],
  }));
  return project;
}

function assertUiWasm(output) {
  const module = new WebAssembly.Module(readFileSync(output));
  const imports = WebAssembly.Module.imports(module);
  assert.ok(imports.length > 0);
  assert.ok(imports.every(item => item.module === 'vera:client@1.0'), JSON.stringify(imports));
  const exported = WebAssembly.Module.exports(module).map(item => item.name);
  assert.ok(exported.includes('client_init'));
  assert.ok(exported.includes('memory'));
  assert.ok(!exported.some(name => name === 'vera_init' || /^on_/.test(name)), 'UI must not export simulation or timer hooks');
  assert.deepEqual(WebAssembly.Module.customSections(module, 'vera.delivery').map(bytes => [...new Uint8Array(bytes)]), [
    [1, 8, 0, 0, 0, 248, 3, 0, 0],
  ]);
  for (const name of ['vera.actions', 'vera.vars', 'vera.notify']) {
    assert.equal(WebAssembly.Module.customSections(module, name).length, 0, 'UI must not declare ' + name);
  }
  return module;
}

function clientInstance(module, host = {}) {
  const lines = [];
  const instance = new WebAssembly.Instance(module, { 'vera:client@1.0': {
    ...host,
    client_log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
  } });
  instance.exports.client_init();
  return { instance, lines };
}

test('UI module builds without npm install using the client facade and generates client declarations', t => {
  const directory = scratch(t);
  const project = makeUiProject(directory);
  const output = join(project, 'dist/ui.wasm');
  const paths = [join(facadeSource, 'client.ts'), join(dirname(require.resolve(`${enginePackage}/package.json`)), 'bin/transform.js')];
  const before = paths.map(path => readFileSync(path));
  build(join(root, 'project-build.mjs'), project, output);
  const module = assertUiWasm(output);
  assert.deepEqual(WebAssembly.Module.exports(module).map(item => item.name).filter(name => clientLifecycles.includes(name)), ['client_on_start']);
  const { instance, lines } = clientInstance(module);
  instance.exports.client_on_start();
  assert.deepEqual(lines, ['ui ready']);
  assert.match(readFileSync(join(project, 'src/generated/vars.ts'), 'utf8'), /ClientVar.*from "@vera20k\/sdk\/client"/);
  assert.match(readFileSync(join(project, 'src/generated/notify.ts'), 'utf8'), /NotifyName.*from "@vera20k\/sdk\/client"/);
  paths.forEach((path, i) => assert.deepEqual(readFileSync(path), before[i]));
});

for (const { hook, imports, registration, handler, dispatch } of [
  { hook: 'client_on_start', imports: 'onStart as ready', registration: 'ready(handle);', handler: "function handle(): void { Client.log('start'); }", dispatch: exports => exports.client_on_start() },
  { hook: 'client_on_render', imports: 'onRender as render', registration: 'render(handle);', handler: "function handle(dt: f32, alpha: f32): void { Client.log('render'); }", dispatch: exports => exports.client_on_render(0.1, 0.5) },
  { hook: 'client_on_events', imports: 'onClientEvent as listen, ClientEvent, ClickEvent', registration: 'listen<ClickEvent>(ClientEvent.Click, handle);', handler: "function handle(e: ClickEvent): void { Client.log('click'); }", dispatch: exports => {
    // ClientEvent.Click = 0, one delivery record in the host-reserved area.
    new DataView(exports.memory.buffer).setInt32(256, 0, true);
    exports.client_on_events(256, 1);
  } },
]) {
  for (const dependency of [false, true]) {
    test(`UI ${hook} survives aliased imports${dependency ? ' in an npm helper' : ''} and executes only its lifecycle`, t => {
      const directory = scratch(t);
      const project = makeUiProject(directory);
      let source = `import { Client, ${imports} } from '@ra2-map-editor/mission-sdk/client';\n${handler}\n${registration}\n`;
      if (dependency) {
        const helper = join(project, 'node_modules/client-helper');
        mkdirSync(helper, { recursive: true });
        writeFileSync(join(helper, 'package.json'), JSON.stringify({ name: 'client-helper', version: '1.0.0', ascMain: 'index.ts' }));
        source = source.replace(registration, `export function install(): void { ${registration} }`);
        writeFileSync(join(helper, 'index.ts'), source);
        source = "import { install } from 'client-helper';\ninstall();\n";
      }
      writeFileSync(join(project, 'src/ui.ts'), source);
      const output = join(project, 'dist/ui.wasm');
      build(join(root, 'project-build.mjs'), project, output);
      const module = assertUiWasm(output);
      assert.deepEqual(WebAssembly.Module.exports(module).map(item => item.name).filter(name => clientLifecycles.includes(name)), [hook]);
      const { instance, lines } = clientInstance(module);
      dispatch(instance.exports);
      assert.deepEqual(lines, [hook === 'client_on_start' ? 'start' : hook === 'client_on_render' ? 'render' : 'click']);
    });
  }
}

test('UI compilation rejects simulation helpers and preserves a previous artifact', t => {
  const directory = scratch(t);
  const project = makeUiProject(directory);
  const output = join(project, 'dist/ui.wasm');
  mkdirSync(dirname(output), { recursive: true });
  const previous = Buffer.from('previous client module');
  writeFileSync(output, previous);
  writeFileSync(join(project, 'src/ui.ts'), "import { setTimeout, console } from '@ra2-map-editor/mission-sdk';\nsetTimeout((): void => { console.log('bad'); }, 1);\n");
  assert.throws(() => build(join(root, 'project-build.mjs'), project, output), error => {
    assert.match(String(error.stderr), /client module may import only vera:client@1\.0/);
    return true;
  });
  assert.deepEqual(readFileSync(output), previous);
});

test('invalid module kind fails before creating an output', t => {
  const directory = scratch(t);
  const project = makeUiProject(directory);
  writeFileSync(join(project, 'vera.json'), JSON.stringify({ kind: 'client', entry: 'src/ui.ts' }));
  const output = join(project, 'dist/ui.wasm');
  assert.throws(() => build(join(root, 'project-build.mjs'), project, output), error => {
    assert.match(String(error.stderr), /kind must be "rules" or "ui"/);
    return true;
  });
  assert.equal(existsSync(output), false);
});


test('checked-in UI fixture reports exact lifecycle counts after 600 render frames', t => {
  const directory = scratch(t);
  const project = join(directory, 'ui-fixture');
  cpSync(join(root, 'engine-check/ui-fixture'), project, { recursive: true });
  const output = join(project, 'build/ui.wasm');
  build(join(root, 'project-build.mjs'), project, output);
  const module = assertUiWasm(output);
  const imports = WebAssembly.Module.imports(module);
  assert.deepEqual(imports.map(item => item.name).sort(), ['client_frame', 'client_log']);
  assert.deepEqual(WebAssembly.Module.exports(module).map(item => item.name).filter(name => clientLifecycles.includes(name)).sort(), [...clientLifecycles].sort());
  let frame = 0;
  const { instance, lines } = clientInstance(module, { client_frame: () => frame });
  instance.exports.client_on_start();
  for (; frame < 600; frame++) {
    if (frame === 0 || frame === 300) {
      // ClientEvent.SelectionChanged = 5; delivery uses one 32-byte record.
      new DataView(instance.exports.memory.buffer).setInt32(256, 5, true);
      instance.exports.client_on_events(256, 1);
    }
    assert.deepEqual(lines, [], 'the fixture must not log before the 600th render');
    instance.exports.client_on_render(1 / 60, 0.5);
  }
  assert.deepEqual(lines, ['ui lifecycle start=1 render=600 events=2 frame=599']);
});

function makeExtension(directory, name = 'extension') {
  const project = join(directory, name);
  mkdirSync(join(project, 'shared'), { recursive: true });
  writeFileSync(join(project, 'package.json'), JSON.stringify({
    name: 'joint-extension', private: true, type: 'module', vera: { kind: 'extension' },
    dependencies: { [facadePackage]: '0.1.3', [enginePackage]: '0.1.0' },
  }));
  writeFileSync(join(project, 'tsconfig.json'), JSON.stringify({
    extends: '@vera20k/sdk/tsconfig.base.json',
    include: ['rules/src/**/*.ts', 'ui/src/**/*.ts', 'shared/**/*.ts'],
  }));
  writeFileSync(join(project, 'shared/protocol.ts'), 'export function protocolVersion(): i32 { return 42; }\n');
  for (const kind of ['rules', 'ui']) {
    const child = join(project, kind);
    mkdirSync(join(child, 'src'), { recursive: true });
    writeFileSync(join(child, 'vera.json'), JSON.stringify({ kind, entry: 'src/main.ts', out: 'must-not-be-written.wasm' }));
    const imports = kind === 'rules' ? 'console as output, onScenarioStart as ready' : 'Client as output, onStart as ready';
    const sdk = kind === 'rules' ? facadePackage : `${facadePackage}/client`;
    // Registrations and relative imports outside the child directory must be scanned.
    writeFileSync(join(project, `shared/${kind}-hooks.ts`), `
import { ${imports} } from '${sdk}';
import { protocolVersion } from './protocol';
function start(): void { output.log(protocolVersion().toString()); }
export function install(): void { ready(start); }
`);
    writeFileSync(join(child, 'src/main.ts'), `import { install } from '../../shared/${kind}-hooks';\ninstall();\n`);
  }
  return project;
}

function buildExtension(builder, project, extra = [], env = process.env) {
  return execFileSync(process.execPath, [builder, project, ...extra], {
    stdio: 'pipe', env: { ...env, NODE_PATH: '' },
  });
}

function extensionOutputs(project) {
  return ['rules', 'ui'].map(kind => join(project, 'dist', `${kind}.wasm`));
}

function previousExtension(project) {
  mkdirSync(join(project, 'dist'), { recursive: true });
  const previous = [Buffer.from('previous rules module'), Buffer.from('previous UI module')];
  extensionOutputs(project).forEach((output, index) => writeFileSync(output, previous[index]));
  return previous;
}

function assertExtensionUnchanged(project, previous) {
  extensionOutputs(project).forEach((output, index) => assert.deepEqual(readFileSync(output), previous[index]));
  assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
}

function isolatedTemporary(directory) {
  const temporary = join(directory, 'compiler temporary');
  mkdirSync(temporary);
  return { temporary, env: { ...process.env, TEMP: temporary, TMP: temporary, TMPDIR: temporary } };
}

function patchBuildCli(engine, statement, marker = '  console.log(`${relative(process.cwd(), out) || out}: ${complete.length} bytes` +') {
  const cli = join(engine, 'bin/vera20k.js');
  const source = readFileSync(cli, 'utf8');
  assert.equal(source.split(marker).length, 2, 'test instrumentation needs one pinned SDK hook');
  writeFileSync(cli, source.replace(marker, statement + '\n' + marker));
}

for (const debug of [false, true]) {
  test(`joint ${debug ? 'debug' : 'release'} build publishes both Wasm worlds with shared imports and root-only dependencies`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeExtension(directory, '联合项目 with spaces');
    installSdks(project);
    const dependency = join(project, 'node_modules/protocol-helper');
    mkdirSync(dependency, { recursive: true });
    writeFileSync(join(dependency, 'package.json'), JSON.stringify({ name: 'protocol-helper', version: '1.0.0', ascMain: 'index.ts' }));
    writeFileSync(join(dependency, 'index.ts'), 'export function answer(): i32 { return 42; }\n');
    writeFileSync(join(project, 'shared/protocol.ts'), "export { answer as protocolVersion } from 'protocol-helper';\n");
    const { temporary, env } = isolatedTemporary(directory);
    previousExtension(project);
    buildExtension(builder, project, debug ? ['--debug'] : [], env);
    const [rules, ui] = extensionOutputs(project);
    const module = assertWasm(rules);
    const lines = [];
    let instance;
    instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': {
      log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
    } });
    instance.exports.vera_init();
    instance.exports.on_scenario_start();
    assert.deepEqual(lines, ['42']);
    const client = clientInstance(assertUiWasm(ui));
    client.instance.exports.client_on_start();
    assert.deepEqual(client.lines, ['42']);
    for (const output of [rules, ui]) {
      const wasm = new WebAssembly.Module(readFileSync(output));
      assert.equal(WebAssembly.Module.customSections(wasm, 'name').length > 0, debug, '--debug must reach both compilers');
    }
    assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
    assert.deepEqual(readdirSync(temporary), []);
    assert.equal(existsSync(join(project, 'vera.json')), false);
    for (const kind of ['rules', 'ui']) {
      for (const name of ['package.json', 'tsconfig.json', 'node_modules', 'must-not-be-written.wasm']) {
        assert.equal(existsSync(join(project, kind, name)), false, `must not generate ${kind}/${name}`);
      }
    }
  });
}

test('joint build works without npm install using only the bundled SDKs', t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  buildExtension(join(root, 'project-build.mjs'), project);
  const [rules, ui] = extensionOutputs(project);
  assertWasm(rules);
  assertUiWasm(ui);
  assert.equal(existsSync(join(project, 'node_modules')), false);
});

for (const kind of ['rules', 'ui']) {
  test(`joint ${kind} child builds independently without a child package.json or tsconfig.json`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeExtension(directory);
    installSdks(project);
    const outputs = extensionOutputs(project);
    const previous = previousExtension(project);
    build(builder, join(project, kind), outputs[kind === 'rules' ? 0 : 1], ['--debug']);
    (kind === 'rules' ? assertWasm : assertUiWasm)(outputs[kind === 'rules' ? 0 : 1]);
    assert.deepEqual(readFileSync(outputs[kind === 'rules' ? 1 : 0]), previous[kind === 'rules' ? 1 : 0]);
  });
}

test('both generated declaration sets exist before shared sources are compiled', t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  configureSharedDeclarations(project);
  buildExtension(join(root, 'project-build.mjs'), project);
  const [rules, ui] = extensionOutputs(project);
  assertWasm(rules);
  assertUiWasm(ui);
  assert.match(readFileSync(join(project, 'ui/src/generated/vars.ts'), 'utf8'), /ClientVar/);
  assert.match(readFileSync(join(project, 'rules/src/generated/vars.ts'), 'utf8'), /GlobalVar/);
});

for (const kind of ['rules', 'ui']) {
  for (const existing of [false, true]) {
    test(`joint ${kind} compilation failure ${existing ? 'preserves both previous artifacts' : 'publishes neither new artifact'}`, t => {
      const directory = scratch(t);
      const project = makeExtension(directory);
      const previous = existing ? previousExtension(project) : null;
      writeFileSync(join(project, kind, 'src/main.ts'), 'export function broken(): i32 { return noSuchJointValue; }\n');
      const { temporary, env } = isolatedTemporary(directory);
      assert.throws(() => buildExtension(join(root, 'project-build.mjs'), project, [], env), error => {
        assert.match(String(error.stderr), /noSuchJointValue/);
        return true;
      });
      if (existing) assertExtensionUnchanged(project, previous);
      else assert.deepEqual(readdirSync(join(project, 'dist')), []);
      assert.deepEqual(readdirSync(temporary), []);
    });
  }
}

for (const { path, value, phase } of [
  { path: 'package.json', value: { vera: { kind: 'rules' } }, phase: 'rules' },
  { path: 'rules/vera.json', value: { kind: 'rules', entry: 'src/different.ts' }, phase: 'ui' },
  { path: 'ui/vera.json', value: { kind: 'rules' }, phase: 'ui' },
  { path: 'package.json', value: { name: 'changed-root-dependencies' }, phase: 'ui' },
  { path: 'tsconfig.json', value: { include: [] }, phase: 'ui' },
  { path: 'rules/asconfig.json', value: { options: { initialMemory: 8 } }, phase: 'ui' },
  { path: 'vera.json', value: { kind: 'rules' }, phase: 'ui' },
]) {
  test(`joint build rejects ${path} drift after ${phase} compiles without publishing either artifact`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeExtension(directory);
    const { engine } = installSdks(project);
    const previous = previousExtension(project);
    const changed = join(project, path);
    patchBuildCli(engine, `if (config.kind === ${JSON.stringify(phase)}) writeFileSync(${JSON.stringify(changed)}, ${JSON.stringify(JSON.stringify(value))});`);
    const { temporary, env } = isolatedTemporary(directory);
    assert.throws(() => buildExtension(builder, project, [], env), error => {
      assert.match(String(error.stderr), /Configuration changed during build/);
      assert.ok(String(error.stderr).includes(changed));
      return true;
    });
    assertExtensionUnchanged(project, previous);
    assert.deepEqual(readdirSync(temporary), []);
  });
}

test('single-module compilation also rejects configuration drift without replacing output', t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeProject(directory);
  const { engine } = installSdks(project);
  const output = join(directory, 'rules.wasm');
  const previous = Buffer.from('previous single module');
  writeFileSync(output, previous);
  patchBuildCli(engine, `writeFileSync(${JSON.stringify(join(project, 'vera.json'))}, '{}');`);
  assert.throws(() => build(builder, project, output), error => {
    assert.match(String(error.stderr), /Configuration changed during build/);
    return true;
  });
  assert.deepEqual(readFileSync(output), previous);
  assert.ok(!readdirSync(directory).some(name => name.startsWith('.ra2-mission-build-')));
});


test('joint root rejects malformed or invalid package identity before touching either artifact', async t => {
  for (const scenario of invalidPackages) {
    await t.test(scenario.name, t => {
      const directory = scratch(t);
      const project = makeExtension(directory);
      const previous = previousExtension(project);
      writeFileSync(join(project, 'package.json'), scenario.contents);
      assert.throws(() => buildExtension(join(root, 'project-build.mjs'), project), error => {
        assert.match(String(error.stderr), scenario.message);
        return true;
      });
      assertExtensionUnchanged(project, previous);
      for (const kind of ['rules', 'ui']) assert.equal(existsSync(join(project, kind, 'src/generated')), false);
    });
  }
});

for (const { path, value, message } of [
  { path: 'vera.json', value: { kind: 'rules' }, message: /extension root must not contain vera.json/ },
  { path: 'rules/package.json', value: {}, message: /rules\/package.json.*root package.json/ },
  { path: 'rules/vera.json', value: { kind: 'ui', entry: 'src/main.ts' }, message: /rules\/vera.json kind must be "rules"/ },
  { path: 'ui/vera.json', value: { kind: 'rules' }, message: /ui\/vera.json kind must be "ui"/ },
  { path: 'ui/vera.json', value: null, message: /ui\/vera.json is required/ },
]) {
  test(`joint layout rejects invalid ${path} before publishing`, t => {
    const directory = scratch(t);
    const project = makeExtension(directory);
    const previous = previousExtension(project);
    if (value === null) rmSync(join(project, path));
    else writeFileSync(join(project, path), JSON.stringify(value));
    assert.throws(() => buildExtension(join(root, 'project-build.mjs'), project), error => {
      assert.match(String(error.stderr), message);
      return true;
    });
    assertExtensionUnchanged(project, previous);
  });
}

test('joint CLI rejects ambiguous output and invalid flags without touching artifacts', t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  const previous = previousExtension(project);
  for (const extra of [['alternate.wasm'], ['--release'], ['--debug', '--debug'], ['--debug', 'alternate.wasm']]) {
    assert.throws(() => buildExtension(join(root, 'project-build.mjs'), project, extra), error => {
      assert.match(String(error.stderr), /usage: node project-build.mjs/);
      return true;
    });
    assertExtensionUnchanged(project, previous);
  }
});



for (const code of ['EPERM', 'EACCES', 'EBUSY']) {
  test(`Windows publication retries transient ${code} without unlinking the previous output`, { skip: process.platform !== 'win32' }, t => {
    const directory = scratch(t);
    const project = makeProject(directory);
    const output = join(directory, 'rules.wasm');
    const previous = 'previous protected artifact';
    writeFileSync(output, previous);
    const audit = join(directory, 'rename-attempts.json');
    const hook = join(directory, 'transient-rename.mjs');
    writeFileSync(hook, `
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
const rename = fs.renameSync;
const remove = fs.rmSync;
const unlink = fs.unlinkSync;
let attempts = 0;
fs.rmSync = function(path, ...args) {
  if (path === ${JSON.stringify(output)}) throw new Error('must not remove the previous output');
  return remove(path, ...args);
};
fs.unlinkSync = function(path, ...args) {
  if (path === ${JSON.stringify(output)}) throw new Error('must not unlink the previous output');
  return unlink(path, ...args);
};
fs.renameSync = function(source, destination) {
  if (destination === ${JSON.stringify(output)}) {
    attempts++;
    fs.writeFileSync(${JSON.stringify(audit)}, JSON.stringify(attempts));
    if (attempts <= 2) {
      if (fs.readFileSync(destination, 'utf8') !== ${JSON.stringify(previous)}) throw new Error('old output changed before successful rename');
      throw Object.assign(new Error('simulated transient sharing conflict'), { code: ${JSON.stringify(code)} });
    }
  }
  return rename(source, destination);
};
syncBuiltinESMExports();
`);
    const { temporary, env } = isolatedTemporary(directory);
    execFileSync(process.execPath, ['--import', pathToFileURL(hook).href, join(root, 'project-build.mjs'), project, output], { stdio: 'pipe', env });
    assertWasm(output);
    assert.equal(JSON.parse(readFileSync(audit, 'utf8')), 3);
    assert.deepEqual(readdirSync(temporary), []);
    assert.ok(!readdirSync(directory).some(name => name.startsWith('.ra2-mission-build-')));
  });
}

test('Windows persistent publication EPERM stops after the bounded retries and preserves both outputs', { skip: process.platform !== 'win32' }, t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  const previous = previousExtension(project);
  const [rules] = extensionOutputs(project);
  const audit = join(directory, 'persistent-attempts.json');
  const hook = join(directory, 'persistent-rename.mjs');
  writeFileSync(hook, `
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
const rename = fs.renameSync;
let attempts = 0;
fs.renameSync = function(source, destination) {
  if (destination === ${JSON.stringify(rules)}) {
    fs.writeFileSync(${JSON.stringify(audit)}, JSON.stringify(++attempts));
    throw Object.assign(new Error('simulated persistent sharing conflict'), { code: 'EPERM' });
  }
  return rename(source, destination);
};
syncBuiltinESMExports();
`);
  const { temporary, env } = isolatedTemporary(directory);
  assert.throws(() => execFileSync(process.execPath, ['--import', pathToFileURL(hook).href, join(root, 'project-build.mjs'), project], { stdio: 'pipe', env }), error => {
    assert.match(String(error.stderr), /simulated persistent sharing conflict/);
    return true;
  });
  assert.equal(JSON.parse(readFileSync(audit, 'utf8')), 41, 'one initial attempt plus 40 bounded retries');
  assertExtensionUnchanged(project, previous);
  assert.deepEqual(readdirSync(temporary), []);
});

test('Windows rollback retries a transient rename conflict and restores the first output', { skip: process.platform !== 'win32' }, t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  const previous = previousExtension(project);
  const [rules, ui] = extensionOutputs(project);
  const audit = join(directory, 'rollback-attempts.json');
  const hook = join(directory, 'rollback-rename.mjs');
  writeFileSync(hook, `
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
const rename = fs.renameSync;
let attempts = 0;
fs.renameSync = function(source, destination) {
  if (destination === ${JSON.stringify(ui)}) throw new Error('simulated UI publication failure');
  if (destination === ${JSON.stringify(rules)} && source.endsWith('.previous.wasm')) {
    fs.writeFileSync(${JSON.stringify(audit)}, JSON.stringify(++attempts));
    if (attempts <= 2) throw Object.assign(new Error('simulated transient rollback conflict'), { code: 'EPERM' });
  }
  return rename(source, destination);
};
syncBuiltinESMExports();
`);
  const { temporary, env } = isolatedTemporary(directory);
  assert.throws(() => execFileSync(process.execPath, ['--import', pathToFileURL(hook).href, join(root, 'project-build.mjs'), project], { stdio: 'pipe', env }), error => {
    assert.match(String(error.stderr), /simulated UI publication failure/);
    assert.doesNotMatch(String(error.stderr), /rollback failed/);
    return true;
  });
  assert.equal(JSON.parse(readFileSync(audit, 'utf8')), 3);
  assertExtensionUnchanged(project, previous);
  assert.deepEqual(readdirSync(temporary), []);
});

for (const existing of [false, true]) {
  test(`joint publication failure rolls back the first ${existing ? 'replacement' : 'new artifact'}`, t => {
    const directory = scratch(t);
    const project = makeExtension(directory);
    const previous = existing ? previousExtension(project) : null;
    const [, ui] = extensionOutputs(project);
    const hook = join(directory, 'publish-failure.mjs');
    writeFileSync(hook, `
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
const rename = fs.renameSync;
fs.renameSync = (source, destination) => {
  if (destination === ${JSON.stringify(ui)}) throw new Error('simulated UI publication failure');
  return rename(source, destination);
};
syncBuiltinESMExports();
`);
    const { temporary, env } = isolatedTemporary(directory);
    assert.throws(() => execFileSync(process.execPath, [
      '--import', pathToFileURL(hook).href, join(root, 'project-build.mjs'), project,
    ], { stdio: 'pipe', env }), error => {
      assert.match(String(error.stderr), /simulated UI publication failure/);
      return true;
    });
    if (existing) assertExtensionUnchanged(project, previous);
    else assert.deepEqual(readdirSync(join(project, 'dist')), []);
    assert.deepEqual(readdirSync(temporary), []);
  });
}

test('concurrent joint builds use independent candidates and preserve old outputs until both modules compile', async t => {
  const directory = scratch(t);
  const builder = builderCopy(directory);
  const project = makeExtension(directory);
  const { engine } = installSdks(project);
  const previous = previousExtension(project);
  const audit = join(project, 'candidates.jsonl');
  // This is a disposable installed SDK copy, never a modified real package.
  // Each child reports its actual candidate after the upstream atomic write.
  patchBuildCli(engine, `
writeFileSync(${JSON.stringify(audit)}, JSON.stringify({ kind: config.kind, out }) + '\\n', { flag: 'a' });
`);
  const { temporary, env } = isolatedTemporary(directory);
  const run = () => new Promise((resolve, reject) => execFile(process.execPath, [builder, project, '--debug'], {
    env: { ...env, NODE_PATH: '' },
  }, (error, stdout, stderr) => error ? reject(new Error(stderr || error.message)) : resolve(stdout)));
  // Verify no early publication inside each invocation with a separate build;
  // concurrent invocations may legitimately finish at different times.
  patchBuildCli(engine, `
if (config.kind === 'ui') {
  const outputs = ${JSON.stringify(extensionOutputs(project))};
  const expected = ${JSON.stringify(previous.map(bytes => bytes.toString()))};
  for (let i = 0; i < outputs.length; i++) {
    if (readFileSync(outputs[i], 'utf8') !== expected[i]) throw new Error('published before both modules compiled');
  }
}
`);
  await run();
  assertWasm(extensionOutputs(project)[0]);
  assertUiWasm(extensionOutputs(project)[1]);
  // Restore the uninstrumented check; the audit remains enabled for concurrency.
  const cli = join(engine, 'bin/vera20k.js');
  const source = readFileSync(cli, 'utf8');
  const start = source.indexOf("if (config.kind === 'ui') {\n  const outputs =");
  const end = source.indexOf('  console.log(`${relative(process.cwd(), out)', start);
  assert.ok(start >= 0 && end > start);
  writeFileSync(cli, source.slice(0, start) + source.slice(end));
  rmSync(audit);
  const results = await Promise.allSettled([run(), run()]);
  for (const result of results) assert.equal(result.status, 'fulfilled', result.reason?.message);
  const records = readFileSync(audit, 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.equal(records.length, 4);
  const candidates = [...new Set(records.map(record => dirname(record.out)))];
  assert.equal(candidates.length, 2, 'each concurrent invocation must use its own staging directory');
  for (const candidate of candidates) {
    assert.deepEqual(records.filter(record => dirname(record.out) === candidate).map(record => record.kind).sort(), ['rules', 'ui']);
    assert.equal(existsSync(candidate), false);
  }
  assertWasm(extensionOutputs(project)[0]);
  assertUiWasm(extensionOutputs(project)[1]);
  assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
  assert.deepEqual(readdirSync(temporary), []);
});


function configureSharedDeclarations(project, variable = 'Score', notification = 'Notice') {
  for (const kind of ['rules', 'ui']) {
    writeFileSync(join(project, kind, 'vera.json'), JSON.stringify({
      kind, entry: 'src/main.ts', vars: [{ name: variable, scope: 'global' }], notify: [notification],
    }));
  }
  // Reading wrapper names needs no host calls from the other Wasm world, but
  // does require both generated sources and their current exported bindings.
  writeFileSync(join(project, 'shared/protocol.ts'), `
import { ${variable} as SimulationScore } from '../rules/src/generated/vars';
import { ${variable} as ClientScore } from '../ui/src/generated/vars';
import { ${notification} as SimulationNotice } from '../rules/src/generated/notify';
import { ${notification} as ClientNotice } from '../ui/src/generated/notify';
export function protocolVersion(): i32 {
  return SimulationScore.name.length + ClientScore.name.length
    + SimulationNotice.name.length + ClientNotice.name.length;
}
`);
  return 2 * (variable.length + notification.length);
}

function assertGeneratedProtocol(kind, output, expected) {
  if (kind === 'ui') {
    const client = clientInstance(assertUiWasm(output));
    client.instance.exports.client_on_start();
    assert.deepEqual(client.lines, [String(expected)]);
  } else {
    const module = assertWasm(output);
    const lines = [];
    let instance;
    instance = new WebAssembly.Instance(module, { 'vera:sim@1.0': {
      log: (ptr, length) => lines.push(Buffer.from(instance.exports.memory.buffer, ptr, length).toString('utf8')),
    } });
    instance.exports.vera_init();
    instance.exports.on_scenario_start();
    assert.deepEqual(lines, [String(expected)]);
  }
}

for (const kind of ['rules', 'ui']) {
  const index = kind === 'rules' ? 0 : 1;
  const other = kind === 'rules' ? 'ui' : 'rules';
  test(`cold child ${kind} build generates both sides for shared imports and publishes only its own Wasm`, t => {
    const directory = scratch(t);
    const project = makeExtension(directory, '独立共享项目 with spaces');
    const expected = configureSharedDeclarations(project);
    for (const child of ['rules', 'ui']) assert.equal(existsSync(join(project, child, 'src/generated')), false);
    // Generation must not compile the sibling's entry point.
    writeFileSync(join(project, other, 'src/main.ts'), 'export function broken(): i32 { return mustNotCompileSibling; }\n');
    const previous = previousExtension(project);
    const outputs = extensionOutputs(project);
    const { temporary, env } = isolatedTemporary(directory);
    build(join(root, 'project-build.mjs'), join(project, kind), outputs[index], [], env);
    assertGeneratedProtocol(kind, outputs[index], expected);
    assert.notDeepEqual(readFileSync(outputs[index]), previous[index]);
    assert.deepEqual(readFileSync(outputs[1 - index]), previous[1 - index]);
    assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
    assert.deepEqual(readdirSync(temporary), []);
    assert.equal(existsSync(join(project, 'node_modules')), false);
  });

  test(`child ${kind} build refreshes stale declarations on both sides before compiling shared imports`, t => {
    const directory = scratch(t);
    const project = makeExtension(directory);
    configureSharedDeclarations(project, 'OldScore', 'OldNotice');
    buildExtension(join(root, 'project-build.mjs'), project);
    const outputs = extensionOutputs(project);
    const previous = outputs.map(output => readFileSync(output));
    for (const child of ['rules', 'ui']) assert.match(readFileSync(join(project, child, 'src/generated/vars.ts'), 'utf8'), /OldScore/);
    const expected = configureSharedDeclarations(project, 'CurrentScore', 'CurrentNotice');
    writeFileSync(join(project, other, 'src/main.ts'), 'export function broken(): i32 { return mustNotCompileSibling; }\n');
    build(join(root, 'project-build.mjs'), join(project, kind), outputs[index], ['--debug']);
    assertGeneratedProtocol(kind, outputs[index], expected);
    assert.notDeepEqual(readFileSync(outputs[index]), previous[index]);
    assert.deepEqual(readFileSync(outputs[1 - index]), previous[1 - index]);
    assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
    for (const child of ['rules', 'ui']) {
      assert.match(readFileSync(join(project, child, 'src/generated/vars.ts'), 'utf8'), /CurrentScore/);
      assert.doesNotMatch(readFileSync(join(project, child, 'src/generated/vars.ts'), 'utf8'), /OldScore/);
      assert.match(readFileSync(join(project, child, 'src/generated/notify.ts'), 'utf8'), /CurrentNotice/);
    }
  });

  test(`child ${kind} build rejects sibling configuration drift without publishing`, t => {
    const directory = scratch(t);
    const builder = builderCopy(directory);
    const project = makeExtension(directory);
    const { engine } = installSdks(project);
    const previous = previousExtension(project);
    const changed = join(project, other, 'asconfig.json');
    patchBuildCli(engine, `writeFileSync(${JSON.stringify(changed)}, '{}');`);
    const { temporary, env } = isolatedTemporary(directory);
    assert.throws(() => build(builder, join(project, kind), extensionOutputs(project)[index], [], env), error => {
      assert.match(String(error.stderr), /Configuration changed during build/);
      assert.ok(String(error.stderr).includes(changed));
      return true;
    });
    assertExtensionUnchanged(project, previous);
    assert.deepEqual(readdirSync(temporary), []);
  });

  test(`child ${kind} build validates the sibling before generating or compiling`, t => {
    const directory = scratch(t);
    const project = makeExtension(directory);
    const previous = previousExtension(project);
    writeFileSync(join(project, other, 'vera.json'), JSON.stringify({ kind: 'invalid' }));
    assert.throws(() => build(join(root, 'project-build.mjs'), join(project, kind), extensionOutputs(project)[index]), error => {
      assert.match(String(error.stderr), /vera.json kind must be "rules" or "ui"/);
      return true;
    });
    assertExtensionUnchanged(project, previous);
  });
}


for (const kind of ['rules', 'ui']) {
  test(`canonical Windows ${kind} child paths retain parent detection and two-sided generation`, { skip: process.platform !== 'win32' }, t => {
    const directory = scratch(t);
    const project = makeExtension(directory, '联合规范路径 with spaces');
    const expected = configureSharedDeclarations(project);
    const previous = previousExtension(project);
    const index = kind === 'rules' ? 0 : 1;
    const outputs = extensionOutputs(project);
    build(join(root, 'project-build.mjs'), toNamespacedPath(join(project, kind)), toNamespacedPath(outputs[index]), ['--debug']);
    assertGeneratedProtocol(kind, outputs[index], expected);
    assert.deepEqual(readFileSync(outputs[1 - index]), previous[1 - index]);
    for (const child of ['rules', 'ui']) assert.ok(existsSync(join(project, child, 'src/generated/vars.ts')));
    assert.deepEqual(readdirSync(join(project, 'dist')).sort(), ['rules.wasm', 'ui.wasm']);
  });
}

test('child build prepares shared declarations but honors the Rust caller staging output', t => {
  const directory = scratch(t);
  const project = makeExtension(directory);
  const expected = configureSharedDeclarations(project);
  const previous = previousExtension(project);
  const output = join(project, 'dist/outer workspace/ui.wasm');
  build(join(root, 'project-build.mjs'), join(project, 'ui'), output);
  assertGeneratedProtocol('ui', output, expected);
  extensionOutputs(project).forEach((path, index) => assert.deepEqual(readFileSync(path), previous[index]));
  assert.deepEqual(readdirSync(dirname(output)), ['ui.wasm']);
});
