import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const cli = join(root, 'cli.mjs');
const sdk = '@ra2-map-editor/mission-sdk';
const builder = '@ra2-map-editor/mission-wasm-build';

function create(parent, name, ...args) {
  const destination = join(parent, name);
  const result = spawnSync(process.execPath, [cli, destination, ...args], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return destination;
}

function readJson(project, file) {
  return JSON.parse(readFileSync(join(project, file), 'utf8'));
}

function filesUnder(directory, prefix = '') {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const relative = prefix + entry.name;
    return entry.isDirectory()
      ? filesUnder(join(directory, entry.name), relative + '/')
      : [relative];
  }).sort();
}

function assertDependencies(pkg) {
  assert.deepEqual(pkg.dependencies, {
    [sdk]: '0.1.3',
    '@vera20k/sdk': '0.1.0',
  });
  assert.deepEqual(pkg.devDependencies, {
    [builder]: '0.1.3',
    assemblyscript: '0.28.20',
    typescript: '5.9.3',
  });
}

test('scaffold always creates one shared extension project', t => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-extension-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const project = create(parent, 'test-extension');
  assert.equal(existsSync(join(project, 'vera.json')), false);
  const pkg = readJson(project, 'package.json');
  assert.equal(pkg.name, 'test-extension');
  assert.deepEqual(pkg.vera, { kind: 'extension' });
  assertDependencies(pkg);
  assert.deepEqual(pkg.scripts, {
    build: 'ra2-mission-project .',
    'build:rules': 'ra2-mission-project rules dist/rules.wasm',
    'build:ui': 'ra2-mission-project ui dist/ui.wasm',
    check: 'vera20k generate rules && vera20k generate ui && tsc --noEmit -p .',
  });
  const tsconfig = readJson(project, 'tsconfig.json');
  assert.equal(tsconfig.extends, '@ra2-map-editor/mission-sdk/tsconfig.base.json');
  assert.deepEqual(tsconfig.include, ['rules/src/**/*.ts', 'ui/src/**/*.ts', 'shared/**/*.ts']);
  for (const kind of ['rules', 'ui']) {
    assert.deepEqual(readJson(project, `${kind}/vera.json`), {
      kind, entry: 'src/main.ts', out: `../dist/${kind}.wasm`, vars: [], notify: [],
    });
    assert.equal(existsSync(join(project, kind, 'package.json')), false);
    assert.equal(existsSync(join(project, kind, 'vendor')), false);
    const source = readFileSync(join(project, kind, 'src/main.ts'), 'utf8');
    assert.match(source, /import \{ MessageKind \} from '\.\.\/\.\.\/shared\/protocol'/);
    assert.match(source, /MessageKind\.Ready\.toString\(\)/);
    assert.match(source, kind === 'rules'
      ? /from '@ra2-map-editor\/mission-sdk'/
      : /from '@ra2-map-editor\/mission-sdk\/client'/);
    if (kind === 'ui') assert.doesNotMatch(source, /onScenarioStart|setTimeout|setInterval/);
  }
  const shared = readFileSync(join(project, 'shared/protocol.ts'), 'utf8');
  assert.match(shared, /export enum MessageKind/);
  assert.match(shared, /Ready = 1/);
  assert.match(shared, /[\u4e00-\u9fff]/);
  assert.doesNotMatch(shared, /\bimport\b|@vera20k|@ra2-map-editor|\bClient\b|\bconsole\b/);
  assert.equal(existsSync(join(project, 'vendor')), false);
  assert.deepEqual(filesUnder(project), [
    '.gitignore', 'README.md', 'package.json',
    'rules/src/main.ts', 'rules/vera.json',
    'shared/protocol.ts', 'tsconfig.json',
    'ui/src/main.ts', 'ui/vera.json',
  ].sort());
  for (const [source, output] of [['README.md', 'README.md'], ['_gitignore', '.gitignore']]) {
    assert.equal(readFileSync(join(project, output), 'utf8'), readFileSync(join(root, 'template', source), 'utf8'));
  }
});

test('packages-dir rewrites SDK and builder dependencies to local tarballs', () => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-local-'));
  try {
    const packages = join(parent, 'packages');
    const projectParent = join(parent, 'projects');
    mkdirSync(packages);
    mkdirSync(projectParent);
    const sdkTarball = join(packages, 'ra2-map-editor-mission-sdk-0.1.3.tgz');
    const builderTarball = join(packages, 'ra2-map-editor-mission-wasm-build-0.1.3.tgz');
    writeFileSync(sdkTarball, 'test archive placeholder');
    writeFileSync(builderTarball, 'test archive placeholder');

    const project = create(projectParent, 'local-project', '--packages-dir', packages);
    const pkg = JSON.parse(readFileSync(join(project, 'package.json'), 'utf8'));
    assert.equal(pkg.dependencies[sdk], `file:${sdkTarball.replaceAll('\\', '/')}`);
    assert.equal(pkg.devDependencies[builder], `file:${builderTarball.replaceAll('\\', '/')}`);
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});

test('unsupported mode flag is rejected without creating a project', () => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-option-'));
  try {
    const destination = join(parent, 'unsupported');
    const result = spawnSync(process.execPath, [cli, destination, '--mode', 'anything'], { encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Unknown option.*mode/);
    assert.equal(existsSync(destination), false);
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});


test('packages-dir recognizes unscoped TypeScript tarballs as well as scoped SDK packages', t => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-typescript-local-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const packages = join(parent, 'packages');
  mkdirSync(packages);
  for (const file of ['ra2-map-editor-mission-sdk-0.1.3.tgz', 'ra2-map-editor-mission-wasm-build-0.1.3.tgz', 'typescript-5.9.3.tgz']) {
    writeFileSync(join(packages, file), 'test archive placeholder');
  }
  const project = create(parent, 'project', '--packages-dir', packages);
  const pkg = JSON.parse(readFileSync(join(project, 'package.json'), 'utf8'));
  assert.equal(pkg.devDependencies.typescript, `file:${join(packages, 'typescript-5.9.3.tgz').replaceAll('\\', '/')}`);
});

for (const missing of ['README.md', 'extension/ui/src/main.ts']) {
  test(`failed template copying (${missing}) removes only the newly created destination`, t => {
    const parent = mkdtempSync(join(tmpdir(), 'vera-partial-project-'));
    t.after(() => rmSync(parent, { recursive: true, force: true }));
    const isolated = join(parent, 'scaffolder');
    mkdirSync(isolated);
    cpSync(cli, join(isolated, 'cli.mjs'));
    cpSync(join(root, 'template'), join(isolated, 'template'), { recursive: true });
    rmSync(join(isolated, 'template', missing));
    const projects = join(parent, 'projects');
    mkdirSync(projects);
    const sentinel = join(projects, 'keep.txt');
    writeFileSync(sentinel, 'existing sibling');
    const destination = join(projects, 'partial');
    const result = spawnSync(process.execPath, [join(isolated, 'cli.mjs'), destination], { encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /ENOENT/);
    assert.equal(existsSync(destination), false, 'failed scaffolding must not leave a half-created project');
    assert.equal(readFileSync(sentinel, 'utf8'), 'existing sibling');
  });
}

test('an existing destination is rejected without removing user files', t => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-existing-project-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const destination = join(parent, 'existing');
  mkdirSync(destination);
  writeFileSync(join(destination, 'keep.txt'), 'user content');
  const result = spawnSync(process.execPath, [cli, destination], { encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Destination already exists/);
  assert.equal(readFileSync(join(destination, 'keep.txt'), 'utf8'), 'user content');
});

test('packages-dir localizes all dependencies, including paths with spaces', t => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-all-local-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const packages = join(parent, 'packages with spaces');
  mkdirSync(packages);
  const archives = {
    [sdk]: 'ra2-map-editor-mission-sdk-0.1.3.tgz',
    '@vera20k/sdk': 'vera20k-sdk-0.1.0.tgz',
    [builder]: 'ra2-map-editor-mission-wasm-build-0.1.3.tgz',
    assemblyscript: 'assemblyscript-0.28.20.tgz',
    typescript: 'typescript-5.9.3.tgz',
  };
  for (const archive of Object.values(archives)) writeFileSync(join(packages, archive), 'test archive placeholder');
  const project = create(parent, 'project', '--packages-dir', packages);
  const pkg = readJson(project, 'package.json');
  for (const [name, archive] of Object.entries(archives)) {
    assert.equal((pkg.dependencies[name] ?? pkg.devDependencies[name]), `file:${join(packages, archive).replaceAll('\\', '/')}`);
  }
});

test('missing required local tarballs fail before creating the destination', t => {
  const parent = mkdtempSync(join(tmpdir(), 'vera-missing-package-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const packages = join(parent, 'packages');
  mkdirSync(packages);
  writeFileSync(join(packages, 'ra2-map-editor-mission-sdk-0.1.3.tgz'), 'test archive placeholder');
  const destination = join(parent, 'project');
  const result = spawnSync(process.execPath, [cli, destination, '--packages-dir', packages], { encoding: 'utf8' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Missing package .*ra2-map-editor-mission-wasm-build-0\.1\.3\.tgz/);
  assert.equal(existsSync(destination), false);
});

test('template resources contain only the extension tree and common files', () => {
  assert.deepEqual(filesUnder(join(root, 'template')), [
    'README.md', '_gitignore',
    'extension/_package.json', 'extension/tsconfig.json',
    'extension/rules/vera.json', 'extension/rules/src/main.ts',
    'extension/ui/vera.json', 'extension/ui/src/main.ts',
    'extension/shared/protocol.ts',
  ].sort());
  const pkg = readJson(root, 'template/extension/_package.json');
  assertDependencies(pkg);
  assert.deepEqual(pkg.vera, { kind: 'extension' });
  assert.equal(readJson(root, 'package.json').version, '0.1.3');
});

test('help documents fixed extension creation and only supported options', () => {
  const result = spawnSync(process.execPath, [cli, '--help'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Usage: create-vera-app <directory> \[--packages-dir <directory>\]/);
  assert.match(result.stdout, /extension with rules, ui and shared sources/);
  assert.doesNotMatch(result.stdout, /--kind|--mode/);
});

for (const args of [
  ['--kind', 'extension'], ['--kind', 'rules'], ['--kind', 'ui'],
  ['--kind=extension'], ['--kind'], ['--unknown'], ['-k'],
  ['--help', '--kind', 'extension'],
]) {
  test(`unknown arguments ${args.join(' ')} are rejected before creating a project`, t => {
    const parent = mkdtempSync(join(tmpdir(), 'vera-unknown-option-'));
    t.after(() => rmSync(parent, { recursive: true, force: true }));
    const destination = join(parent, 'unsupported');
    const result = spawnSync(process.execPath, [cli, destination, ...args], { encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /Unknown option/);
    assert.equal(existsSync(destination), false);
  });
}
