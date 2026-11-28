import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const facadeSource = resolve(root, '../vera20k-sdk');
const fixture = join(root, 'engine-check/fixture');
const require = createRequire(import.meta.url);

function linkDirectory(source, destination) {
  mkdirSync(dirname(destination), { recursive: true });
  symlinkSync(source, destination, process.platform === 'win32' ? 'junction' : 'dir');
}

function packFacade(t) {
  const parent = mkdtempSync(join(tmpdir(), 'vera-facade-package-'));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const source = join(parent, 'sdk-source');
  cpSync(facadeSource, source, {
    recursive: true,
    filter: path => !relative(facadeSource, path).split(/[\\/]/).includes('node_modules'),
  });
  const npmCli = process.env.npm_execpath || join(dirname(require.resolve('npm/package.json', { paths: [dirname(process.execPath)] })), 'bin/npm-cli.js');
  const [packed] = JSON.parse(execFileSync(process.execPath, [npmCli, 'pack', '--offline', '--json', '--pack-destination', parent, source], {
    cwd: parent,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }));
  const project = join(parent, 'project');
  const installed = join(project, 'node_modules/@ra2-map-editor/mission-sdk');
  mkdirSync(installed, { recursive: true });
  execFileSync('tar', ['-xzf', join(parent, packed.filename), '--strip-components=1', '-C', installed], { stdio: 'pipe' });
  return { source, project, installed, packed };
}

test('npm pack publishes only our mission facade and depends on the published upstream SDK', t => {
  const { source, installed, packed } = packFacade(t);
  const manifest = JSON.parse(readFileSync(join(installed, 'package.json'), 'utf8'));
  assert.equal(manifest.name, '@ra2-map-editor/mission-sdk');
  assert.equal(manifest.version, '0.1.3');
  assert.notEqual(manifest.private, true);
  assert.equal(manifest.dependencies['@vera20k/sdk'], '0.1.0');
  assert.equal(manifest.dependencies.assemblyscript, '0.28.20');
  assert.equal(manifest.bin, undefined, 'the upstream package owns the compiler CLI');
  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
    for (const version of Object.values(manifest[field] ?? {})) {
      assert.doesNotMatch(version, /^(?:file:|link:|workspace:)/, 'published dependencies must not require repository paths');
    }
  }
  const files = packed.files.map(file => file.path);
  assert.deepEqual(files.filter(path => path.startsWith('assembly/')), ['assembly/mission.ts']);
  assert.ok(files.every(path => !/(?:^|\/)(?:node_modules|vendor|bin|template|engine)(?:\/|$)/.test(path)));
  assert.ok(files.includes('client.ts'), 'publish the thin client facade entry');
  assert.match(readFileSync(join(installed, 'client.ts'), 'utf8'), /export \* from "@vera20k\/sdk\/client"/);
  for (const path of ['package.json', 'index.ts', 'assembly/mission.ts', 'tsconfig.base.json', 'README.md', 'LICENSE']) {
    assert.ok(files.includes(path), `tarball is missing ${path}`);
    assert.deepEqual(readFileSync(join(installed, path)), readFileSync(join(source, path)), `packed file differs from package source: ${path}`);
  }
  const entry = readFileSync(join(installed, 'index.ts'), 'utf8');
  assert.match(entry, /export \* from ["']@vera20k\/sdk["']/);
  assert.match(entry, /export \* from ["']\.\/assembly\/mission["']/);
  const mission = readFileSync(join(installed, 'assembly/mission.ts'), 'utf8');
  assert.match(mission, /namespace UnitType/);
  assert.match(mission, /namespace BuildingType/);
  assert.match(mission, /from ["']@vera20k\/sdk\/assembly\/core["']/);
  assert.match(mission, /from ["']@vera20k\/sdk\/assembly\/triggers["']/);
  const config = JSON.parse(readFileSync(join(installed, 'tsconfig.base.json'), 'utf8'));
  assert.equal(config.extends, '@vera20k/sdk/tsconfig.base.json');
  assert.equal(config.compilerOptions?.paths?.['@vera20k/sdk'], undefined, 'the facade must not redirect canonical engine imports to itself');
});

test('the facade tarball type-checks against the actual upstream SDK without a canonical alias override', t => {
  const { source, project, installed } = packFacade(t);
  rmSync(source, { recursive: true, force: true });
  cpSync(fixture, project, { recursive: true });
  const upstream = dirname(require.resolve('@vera20k/sdk/package.json'));
  const upstreamManifest = JSON.parse(readFileSync(join(upstream, 'package.json'), 'utf8'));
  assert.equal(upstreamManifest.name, '@vera20k/sdk');
  assert.equal(upstreamManifest.version, '0.1.0');
  linkDirectory(upstream, join(project, 'node_modules/@vera20k/sdk'));
  const compiler = dirname(require.resolve('assemblyscript/package.json'));
  linkDirectory(compiler, join(project, 'node_modules/assemblyscript'));
  cpSync(join(installed, 'tsconfig.base.json'), join(project, 'tsconfig.json'));
  const typescript = dirname(require.resolve('typescript/package.json', { paths: [facadeSource] }));
  const options = { cwd: project, stdio: 'pipe', env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '' } };
  execFileSync(process.execPath, [join(upstream, 'bin/vera20k.js'), 'generate', project], options);
  execFileSync(process.execPath, [join(typescript, 'bin/tsc'), '--noEmit', '-p', project], options);
});
