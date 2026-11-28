#!/usr/bin/env node
import { copyFileSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, parse, resolve } from 'node:path';
import { parseArgs } from 'node:util';

const template = new URL('./template/', import.meta.url);
const projectTemplate = new URL('extension/', template);
const usage = `Usage: create-vera-app <directory> [--packages-dir <directory>]

Creates an AssemblyScript VERA20k extension with rules, ui and shared sources.
--packages-dir   Use the SDK and builder npm tarballs in this directory`;

function localizePackages(manifest, packagesDir) {
  for (const dependencies of [manifest.dependencies, manifest.devDependencies]) {
    for (const [name, version] of Object.entries(dependencies)) {
      const filename = `${name.replace(/^@/, '').replace('/', '-')}-${version}.tgz`;
      const archive = resolve(packagesDir, filename);
      if (!statSync(archive, { throwIfNoEntry: false })?.isFile()) {
        if (name.startsWith('@ra2-map-editor/')) throw new Error(`Missing package ${archive}`);
        continue;
      }
      dependencies[name] = `file:${archive.replaceAll('\\', '/')}`;
    }
  }
}

function createProject(directory, packagesDir) {
  const target = resolve(directory);
  const manifest = JSON.parse(readFileSync(new URL('_package.json', projectTemplate), 'utf8'));
  const slug = basename(target).toLowerCase().replace(/[^a-z0-9_-]+/g, '-');
  manifest.name = slug.replace(/^[-_]+|[-_]+$/g, '').slice(0, 214) || 'vera-extension';
  if (packagesDir) localizePackages(manifest, packagesDir);

  mkdirSync(dirname(target), { recursive: true });
  try {
    mkdirSync(target);
  } catch (error) {
    if (error.code === 'EEXIST' || target === parse(target).root) {
      throw new Error(`Destination already exists: ${target}`);
    }
    throw error;
  }
  try {
    copyFileSync(new URL('README.md', template), join(target, 'README.md'));
    copyFileSync(new URL('_gitignore', template), join(target, '.gitignore'));
    for (const file of [
      'tsconfig.json', 'shared/protocol.ts',
      'rules/vera.json', 'rules/src/main.ts',
      'ui/vera.json', 'ui/src/main.ts',
    ]) {
      mkdirSync(dirname(join(target, file)), { recursive: true });
      copyFileSync(new URL(file, projectTemplate), join(target, file));
    }
    writeFileSync(join(target, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
  } catch (error) {
    // target was created exclusively above; existing destinations never reach here.
    rmSync(target, { recursive: true, force: true });
    throw error;
  }
  console.log(`Created AssemblyScript VERA20k extension project in ${target}\nRun npm install and npm run build in the project directory.`);
}

try {
  const { values, positionals } = parseArgs({
    strict: true,
    allowPositionals: true,
    options: {
      help: { type: 'boolean', short: 'h' },
      'packages-dir': { type: 'string' },
    },
  });
  if (values.help || positionals.length === 0) {
    console.log(usage);
  } else if (positionals.length === 1 && positionals[0]) {
    createProject(positionals[0], values['packages-dir']);
  } else {
    throw new Error(usage);
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
