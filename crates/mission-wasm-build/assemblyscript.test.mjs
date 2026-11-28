import assert from 'node:assert/strict';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url));
const fixture = join(root, 'engine-check', 'fixture');

function makeProject(t) {
  const scratch = mkdtempSync(join(tmpdir(), 'vera-engine-sdk-'));
  t.after(() => rmSync(scratch, { recursive: true, force: true }));
  const project = join(scratch, 'project');
  mkdirSync(join(project, 'src'), { recursive: true });
  cpSync(join(fixture, 'src', 'mission.ts'), join(project, 'src', 'mission.ts'));
  cpSync(join(fixture, 'vera.json'), join(project, 'vera.json'));
  return {
    builder: join(root, 'project-build.mjs'),
    output: join(scratch, 'dist', 'rules.wasm'),
    project,
  };
}

test('builds the published upstream SDK and mission facade fixture with the production Wasm ABI', (t) => {
  const { builder, output, project } = makeProject(t);
  execFileSync(process.execPath, [builder, project, output], { stdio: 'pipe' });

  assert.equal(existsSync(join(project, 'vendor')), false);
  const module = new WebAssembly.Module(readFileSync(output));
  const imports = WebAssembly.Module.imports(module);
  const exports = WebAssembly.Module.exports(module);
  assert.ok(imports.length > 0);
  assert.ok(imports.every(item => item.module === 'vera:sim@1.0'));
  for (const name of ['memory', 'on_events', 'on_frame', 'on_rules', 'on_scenario_start', 'vera_init']) {
    assert.ok(exports.some(item => item.name === name), `missing lifecycle export ${name}`);
  }
  const [delivery] = WebAssembly.Module.customSections(module, 'vera.delivery');
  assert.ok(delivery);
  const section = new DataView(delivery);
  assert.equal(section.getUint8(0), 1);
  assert.equal(section.getUint32(1, true), 8);
  assert.equal(section.getUint32(5, true), 1016);
});

test('rejects missing, malformed, or invalid-kind vera.json before producing output', async (t) => {
  const cases = [
    { name: 'missing vera.json', contents: null, message: /vera\.json is required/ },
    { name: 'null vera.json', contents: 'null', message: /vera\.json must be a JSON object/ },
    { name: 'array vera.json', contents: '[]', message: /vera\.json must be a JSON object/ },
    { name: 'extension is not a module kind', contents: JSON.stringify({ kind: 'extension' }), message: /vera\.json kind must be \"rules\"/ },
    { name: 'malformed vera.json', contents: '{', message: /vera\.json must contain valid JSON/ },
    { name: 'invalid module kind', contents: JSON.stringify({ kind: 'unknown' }), message: /vera\.json kind must be \"rules\"/ },
    { name: 'missing module kind', contents: JSON.stringify({}), message: /vera\.json kind must be \"rules\"/ },
  ];

  for (const scenario of cases) {
    await t.test(scenario.name, (t) => {
      const { builder, output, project } = makeProject(t);
      const manifest = join(project, 'vera.json');
      if (scenario.contents === null) rmSync(manifest);
      else writeFileSync(manifest, scenario.contents);

      let failure;
      try {
        execFileSync(process.execPath, [builder, project, output], { stdio: 'pipe' });
      } catch (error) {
        failure = error;
      }
      assert.ok(failure, 'expected the build to reject the vera.json configuration');
      assert.match(String(failure.stderr), scenario.message);
      assert.equal(existsSync(output), false);
    });
  }
});
