import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const manifest = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));

function standalone(t) {
  const directory = mkdtempSync(join(tmpdir(), 'vera-sdk-facade-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const source = join(directory, 'sdk');
  mkdirSync(source);
  for (const name of ['package.json', ...manifest.files]) {
    cpSync(join(root, name), join(source, name), { recursive: true });
  }
  return { directory, source };
}

function linkPackage(source, destination) {
  mkdirSync(dirname(destination), { recursive: true });
  symlinkSync(source, destination, process.platform === 'win32' ? 'junction' : 'dir');
}

test('native npm pack publishes only the facade and pins the upstream SDK', t => {
  const { directory, source } = standalone(t);
  const npmCli = process.env.npm_execpath || join(dirname(require.resolve('npm/package.json', {
    paths: [dirname(process.execPath)],
  })), 'bin/npm-cli.js');
  const [packed] = JSON.parse(execFileSync(process.execPath,
    [npmCli, 'pack', '--json', '--ignore-scripts', '--pack-destination', directory], {
      cwd: source, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    }));
  assert.equal(packed.name, '@ra2-map-editor/mission-sdk');
  assert.equal(packed.version, '0.1.3');
  assert.equal(manifest.dependencies['@vera20k/sdk'], '0.1.0');
  assert.equal(manifest.dependencies.assemblyscript, '0.28.20');
  const files = new Set(packed.files.map(file => file.path));
  for (const name of ['index.ts', 'client.ts', 'assembly/mission.ts', 'tsconfig.base.json', 'LICENSE']) {
    assert.ok(files.has(name), `missing facade file: ${name}`);
  }
  for (const name of ['assembly/core.ts', 'assembly/triggers.ts', 'bin/vera20k.js', 'package-release.mjs']) {
    assert.equal(files.has(name), false, `unexpected upstream implementation or staging script: ${name}`);
  }
  assert.equal(manifest.scripts.prepack, undefined);
  assert.equal(manifest.private, undefined);
  assert.ok(existsSync(join(directory, packed.filename)));
  assert.equal(existsSync(join(directory, 'vendor')), false);
});

test('standalone source checks against installed npm dependencies without path aliases', t => {
  const { directory, source } = standalone(t);
  const nodeModules = join(directory, 'node_modules');
  linkPackage(dirname(require.resolve('@vera20k/sdk/package.json')), join(nodeModules, '@vera20k/sdk'));
  linkPackage(dirname(require.resolve('assemblyscript/package.json')), join(nodeModules, 'assemblyscript'));
  execFileSync(process.execPath, [require.resolve('typescript/bin/tsc'), '--noEmit', '-p', source], {
    cwd: directory, stdio: 'pipe',
  });
  const config = JSON.parse(readFileSync(join(source, 'tsconfig.json'), 'utf8'));
  assert.equal(config.compilerOptions?.rootDirs, undefined);
  assert.equal(config.compilerOptions?.paths, undefined);
  const api = readFileSync(join(source, 'assembly/mission.ts'), 'utf8');
  assert.match(api, /from "@vera20k\/sdk\/assembly\/core"/);
});

test('client facade reexports only the upstream client entry without simulation helpers', () => {
  const api = readFileSync(join(root, 'client.ts'), 'utf8');
  assert.match(api, /export \* from "@vera20k\/sdk\/client"/);
  assert.doesNotMatch(api, /from ["'](?:\.\/assembly\/mission|@vera20k\/sdk)["']/);
  assert.ok(JSON.parse(readFileSync(join(root, 'tsconfig.json'), 'utf8')).include.includes('client.ts'));
});
