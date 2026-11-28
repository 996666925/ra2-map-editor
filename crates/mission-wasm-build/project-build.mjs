#!/usr/bin/env node
import { copyFileSync, cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

const engineVersion = '0.1.0';
const facadeVersion = '0.1.3';
const compilerVersion = '0.28.20';
const facadePackage = '@ra2-map-editor/mission-sdk';
const enginePackage = '@vera20k/sdk';
const engineEntries = ['assembly', 'bin', 'client.ts', 'index.ts', 'tsconfig.base.json', 'README.md', 'template', 'package.json'];
const facadeEntries = ['assembly', 'index.ts', 'client.ts', 'package.json'];

function fail(message) {
  throw new Error(message);
}

function readJsonObject(path, name) {
  let value;
  try {
    value = JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    if (error.code === 'ENOENT') fail(`${name} is required`);
    if (error instanceof SyntaxError) fail(`${name} must contain valid JSON`);
    fail(`Could not read ${name}: ${error.message}`);
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    fail(`${name} must be a JSON object`);
  }
  return value;
}

function resolvePackage(require, name) {
  try {
    return require.resolve(name + '/package.json');
  } catch (error) {
    if (error.code !== 'MODULE_NOT_FOUND') throw error;
    return null;
  }
}

function installedPackage(manifestPath, name, entries, version) {
  const manifest = readJsonObject(manifestPath, name + '/package.json');
  if (manifest.name !== name || manifest.version !== version) {
    fail('Expected ' + name + '@' + version + ', found ' + manifest.name + '@' + manifest.version + '.');
  }
  const root = dirname(manifestPath);
  for (const entry of entries) {
    if (!existsSync(join(root, entry))) fail(name + '@' + version + ' is incomplete: missing ' + entry + '.');
  }
  return { root, manifest };
}

function findSdk(project) {
  // Resolve the facade normally so project installs, hoisted dependencies, and
  // npm workspaces work without any dependency on repository source paths.
  for (const require of [createRequire(join(project, 'package.json')), createRequire(import.meta.url)]) {
    const manifestPath = resolvePackage(require, facadePackage);
    if (!manifestPath) continue;
    const facade = installedPackage(manifestPath, facadePackage, [...facadeEntries, 'assembly/mission.ts'], facadeVersion);
    if (facade.manifest.dependencies?.[enginePackage] !== engineVersion) {
      fail(facadePackage + ' must depend on ' + enginePackage + '@' + engineVersion + ' exactly.');
    }

    // Pair the facade with its own dependency, even if another engine SDK is
    // installed in the project or a different ancestor node_modules directory.
    const engineManifestPath = resolvePackage(createRequire(manifestPath), enginePackage);
    if (!engineManifestPath) fail('Install the ' + enginePackage + '@' + engineVersion + ' dependency of ' + facadePackage + '.');
    const engine = installedPackage(engineManifestPath, enginePackage,
      [...engineEntries, 'bin/vera20k.js', 'bin/transform.js'], engineVersion);
    return { engine: engine.root, facade: facade.root };
  }
  fail('Install ' + facadePackage + '@' + facadeVersion + ' in the mission project or alongside the builder.');
}

function stageSdk(source, directory) {
  const sdk = join(directory, 'node_modules', '@vera20k', 'sdk');
  const facade = join(directory, 'node_modules', '@ra2-map-editor', 'mission-sdk');
  mkdirSync(sdk, { recursive: true });
  mkdirSync(facade, { recursive: true });
  for (const name of engineEntries) {
    cpSync(join(source.engine, name), join(sdk, name), { recursive: true, force: true, dereference: true });
  }
  for (const name of facadeEntries) {
    cpSync(join(source.facade, name), join(facade, name), { recursive: true, force: true, dereference: true });
  }

  // The staged CLI must use the builder's pinned compiler, including its real
  // transitive dependencies. A directory junction also works on Windows without
  // enabling developer-mode file symlinks.
  const require = createRequire(import.meta.url);
  const compiler = installedPackage(require.resolve('assemblyscript/package.json'), 'assemblyscript',
    ['dist/asc.js', 'dist/transform.js', 'std/assembly.json'], compilerVersion).root;
  symlinkSync(compiler, join(directory, 'node_modules', 'assemblyscript'), process.platform === 'win32' ? 'junction' : 'dir');

  // asc 0.28.20 searches the project's node_modules and its ancestors BEFORE
  // --path. Redirect engine and facade reads to their private staged copies;
  // keep the project cwd, asconfig, local imports and other packages unchanged.
  // This is an adapter on a disposable copy, never a patch to the engine source
  // or an installed npm package. Fail closed if the pinned CLI changes shape.
  const cli = join(sdk, 'bin', 'vera20k.js');
  const sourceText = readFileSync(cli, 'utf8');
  const marker = 'const result = await asc.main(args, {';
  if (sourceText.split(marker).length !== 2) {
    fail('Unsupported engine SDK compiler entry; expected the pinned asc.main build hook.');
  }
  const modulePathMarker = '"--path", sdkPath(),';
  if (sourceText.split(modulePathMarker).length !== 2) {
    fail('Unsupported engine SDK module search path; expected the pinned sdkPath hook.');
  }
  // Both installed packages must be visible even before a mission project has
  // run npm install. Keep all package lookup inside this private staging area.
  const isolatedCli = sourceText.replace(modulePathMarker, '"--path", resolve(SDK, "../.."),');
  const readFileOption = `
    readFile: (filename, baseDir) => {
      const absolute = resolve(baseDir, filename).replaceAll("\\\\", "/");
      let path = absolute;
      for (const [prefix, root] of [
        ["/node_modules/@vera20k/sdk/", SDK],
        ["/node_modules/@ra2-map-editor/mission-sdk/", resolve(SDK, "../../@ra2-map-editor/mission-sdk")],
      ]) {
        const offset = absolute.lastIndexOf(prefix);
        if (offset >= 0) {
          path = join(root, absolute.slice(offset + prefix.length));
          break;
        }
      }
      try { return readFileSync(path, "utf8"); } catch { return null; }
    },`;
  writeFileSync(cli, isolatedCli.replace(marker, marker + readFileOption));

  // The engine scans only direct project sources. Lifecycle registrations can
  // also be aliased or hidden in npm helpers; inspect those sources separately
  // without scanning SDK definitions or changing named-action collection.
  const transform = join(sdk, 'bin', 'transform.js');
  const transformText = readFileSync(transform, 'utf8');
  const frameMarker = 'on_frame: [/\\bonFrame\\s*\\(/],';
  if (transformText.split(frameMarker).length !== 2) {
    fail('Unsupported engine SDK lifecycle transform; expected the pinned on_frame registration rule.');
  }
  const lifecycleMarker = '    for (const [name, patterns] of Object.entries(NEEDS)) {';
  if (transformText.split(lifecycleMarker).length !== 2) {
    fail('Unsupported engine SDK lifecycle transform; expected the pinned lifecycle scan hook.');
  }
  const registrations = `    const sdkPrefixes = ["~lib/@vera20k/sdk", "~lib/@ra2-map-editor/mission-sdk"];
    const registrationText = parser.sources.filter((source) => !isExports(source)
      && !sdkPrefixes.some((prefix) => source.normalizedPath === prefix || source.normalizedPath.startsWith(prefix + "/")))
      .map((source) => stripComments(source.text)).join("\\n");
    for (const [name, patterns] of Object.entries(NEEDS)) {
      if (patterns.some((pattern) => pattern.test(registrationText))) state.found.uses.add(name);
    }
    if (namedDecider(registrationText)) state.found.uses.add("on_decide");
    // Import aliases and timer helpers also require their lifecycle exports.
    for (const [name, pattern] of [
      ["on_scenario_start", /\\bonScenarioStart\\b/],
      ["on_rules", /\\bonRules\\b/],
      ["on_message", /\\bonMessage\\b/],
      ["on_frame", /\\b(?:onFrame|setTimeout|setInterval)\\b/],
      ["client_on_start", /\\bonStart\\b/],
      ["client_on_render", /\\bonRender\\b/],
      ["client_on_events", /\\bonClientEvent\\b/],
    ]) {
      if (pattern.test(registrationText)) state.found.uses.add(name);
    }
`;
  writeFileSync(transform, transformText.replace(lifecycleMarker, registrations + lifecycleMarker));
  return cli;
}

// Snapshot absent files too: creating a config during compilation also changes
// the inputs. Generated TypeScript files are deliberately not configuration.
const configNames = ['vera.json', 'package.json', 'tsconfig.json', 'asconfig.json'];

function configBytes(path) {
  try {
    return readFileSync(path);
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

function watchConfig(snapshots, project) {
  for (const name of configNames) {
    const path = join(project, name);
    if (!snapshots.has(path)) snapshots.set(path, configBytes(path));
  }
}

function assertConfigUnchanged(snapshots) {
  for (const [path, before] of snapshots) {
    const after = configBytes(path);
    if (before === null ? after !== null : after === null || !before.equals(after)) {
      fail(`Configuration changed during build: ${path}. Outputs were not published; retry the build.`);
    }
  }
}

// An absent vera field is a normal npm package, not an extension marker.
// Once present, it must be valid; never silently downgrade an invalid marker.
function isExtensionProject(project, name = 'package.json') {
  const path = join(project, 'package.json');
  if (configBytes(path) === null) return false;
  const manifest = readJsonObject(path, name);
  if (!Object.hasOwn(manifest, 'vera')) return false;
  if (!manifest.vera || typeof manifest.vera !== 'object' || Array.isArray(manifest.vera)) {
    fail(`${name} vera must be a JSON object`);
  }
  if (manifest.vera.kind !== 'extension') fail(`${name} vera.kind must be "extension"`);
  return true;
}

function moduleProject(project, kind) {
  const config = readJsonObject(join(project, 'vera.json'), `${kind ? kind + '/' : ''}vera.json`);
  if (!['rules', 'ui'].includes(config.kind)) fail('vera.json kind must be "rules" or "ui"');
  if (kind && config.kind !== kind) fail(`${kind}/vera.json kind must be "${kind}"`);
  return { project, kind: config.kind };
}

// Joint root and independent child commands share validation, snapshots and
// declaration generation. Only the requested outputs are compiled/published.
function extensionModules(project, snapshots) {
  watchConfig(snapshots, project);
  if (!isExtensionProject(project)) fail('Expected package.json vera.kind to be "extension"');
  if (snapshots.get(join(project, 'vera.json')) !== null) fail('An extension root must not contain vera.json');
  return ['rules', 'ui'].map(kind => {
    const child = join(project, kind);
    watchConfig(snapshots, child);
    if (snapshots.get(join(child, 'package.json')) !== null) fail(`${kind}/package.json is not supported; use the root package.json`);
    return moduleProject(child, kind);
  });
}

function sdkCommand(cli, command, project, args = []) {
  const result = spawnSync(process.execPath, [cli, command, project, ...args], {
    cwd: project,
    stdio: 'inherit',
    env: { ...process.env, NODE_OPTIONS: '' },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    fail(`${command} failed for ${project}${result.signal ? ` (${result.signal})` : ''}`);
  }
}

// Windows can briefly deny replacement while another publisher (or reader)
// holds the destination. Retry only sharing/access errors, for at most 1 second
// of waiting. Never unlink the destination first: old artifacts stay protected.
function renameOutput(source, destination) {
  for (let attempt = 0; ; attempt++) {
    try {
      renameSync(source, destination);
      return;
    } catch (error) {
      if (process.platform !== 'win32' || !['EPERM', 'EACCES', 'EBUSY'].includes(error.code) || attempt >= 40) throw error;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 25);
    }
  }
}

// All candidates and backups live next to the final files, so replacement and
// rollback use same-filesystem renames, including on Windows. Back up every old
// output before the first replacement, and roll back a partially published pair.
function publishOutputs(modules, pending) {
  const backups = modules.map(({ output }, index) => {
    try {
      if (!lstatSync(output).isFile()) fail(`Output must be a regular file: ${output}`);
    } catch (error) {
      if (error.code === 'ENOENT') return null;
      throw error;
    }
    const backup = join(pending, `${index}.previous.wasm`);
    copyFileSync(output, backup);
    return backup;
  });
  let published = 0;
  try {
    for (const module of modules) {
      renameOutput(module.candidate, module.output);
      published++;
    }
  } catch (error) {
    const failures = [];
    for (let index = published - 1; index >= 0; index--) {
      try {
        if (backups[index]) renameOutput(backups[index], modules[index].output);
        else rmSync(modules[index].output);
      } catch (rollbackError) {
        failures.push(rollbackError.message);
      }
    }
    if (failures.length) {
      // Keep the remaining backups if another process prevents rollback.
      error.keepPending = true;
      error.message += `; rollback failed: ${failures.join('; ')}. Backups retained in ${pending}`;
    }
    throw error;
  }
}

function run() {
  const args = process.argv.slice(2);
  const debug = args.at(-1) === '--debug';
  if (debug) args.pop();
  const usage = 'usage: node project-build.mjs PROJECT [OUTPUT] [--debug] (OUTPUT is required for a single module)';
  if (args.length < 1 || args.length > 2 || args.some(arg => !arg || arg.startsWith('--'))) fail(usage);
  const [projectArg, outputArg] = args;
  const project = resolve(projectArg);
  const snapshots = new Map();
  watchConfig(snapshots, project);
  const extension = isExtensionProject(project);
  if (extension ? outputArg !== undefined : outputArg === undefined) fail(usage);

  let extensionRoot = extension ? project : null;
  if (!extension) {
    const parent = dirname(project);
    if (['rules', 'ui'].includes(relative(parent, project)) && isExtensionProject(parent, '../package.json')) {
      extensionRoot = parent;
    }
  }
  const preparedModules = extensionRoot ? extensionModules(extensionRoot, snapshots) : [];
  let modules;
  if (extension) {
    modules = preparedModules.map(module => ({ ...module, output: join(project, 'dist', `${module.kind}.wasm`) }));
  } else {
    const selected = extensionRoot ? preparedModules.find(module => module.project === project) : moduleProject(project);
    if (!selected) fail('Requested module is not a child of the extension root');
    modules = [{ ...selected, output: resolve(outputArg) }];
  }

  assertConfigUnchanged(snapshots);
  const source = findSdk(extensionRoot ?? project);
  const staging = mkdtempSync(join(tmpdir(), 'ra2-mission-sdk-'));
  let pending;
  let keepPending = false;
  try {
    const cli = stageSdk(source, staging);
    assertConfigUnchanged(snapshots);
    const outputDirectory = dirname(modules[0].output);
    mkdirSync(outputDirectory, { recursive: true });
    pending = mkdtempSync(join(outputDirectory, '.ra2-mission-build-'));
    // A single joint child may also import the sibling's generated sources.
    for (const module of preparedModules) {
      sdkCommand(cli, 'generate', module.project);
      assertConfigUnchanged(snapshots);
    }
    for (const module of modules) {
      assertConfigUnchanged(snapshots);
      module.candidate = join(pending, `${module.kind}.wasm`);
      sdkCommand(cli, 'build', module.project, ['--out', module.candidate, ...(debug ? ['--debug'] : [])]);
      assertConfigUnchanged(snapshots);
    }
    assertConfigUnchanged(snapshots);
    publishOutputs(modules, pending);
    for (const module of modules) console.log(`Published ${module.output}`);
  } catch (error) {
    keepPending = error.keepPending === true;
    throw error;
  } finally {
    if (pending && !keepPending) rmSync(pending, { recursive: true, force: true });
    rmSync(staging, { recursive: true, force: true });
  }
}

try {
  run();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
