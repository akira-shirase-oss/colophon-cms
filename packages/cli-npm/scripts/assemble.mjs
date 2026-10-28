#!/usr/bin/env node
// Assembles the npm packages from a release's archives (docs/modules/08-cli.md, "Install"):
// one package per platform holding that platform's `colophon` binary, and the launcher,
// @colophon-cms/cli, with every version set to the release's.
//
//   node packages/cli-npm/scripts/assemble.mjs --version 0.1.0-alpha.1 --archives dist --out npm
//
// <archives> holds colophon-cli-<version>-<target>.tar.gz (.zip for Windows) for every target and
// the release's SHA256SUMS; each archive is checked against it before anything is taken out of
// it. <out> gets cli/ (the launcher) and cli-<platform>/ for each platform, ready for `npm pack`.
// It never publishes anything.

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { chmod, copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const require = createRequire(import.meta.url);
const { PLATFORMS, binaryName } = require('../lib/platform.js');

const here = path.dirname(fileURLToPath(import.meta.url));
const source = path.resolve(here, '..');
const repository = path.resolve(source, '../..');

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/;

class AssembleError extends Error {}

/** `<hex>  <name>` lines, as `sha256sum` writes them, into a map from name to hex. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split('\n')) {
    const match = /^([0-9a-f]{64}) [ *](.+)$/.exec(line.trim());
    if (match) sums.set(match[2], match[1]);
  }
  return sums;
}

async function sha256(file) {
  return createHash('sha256').update(await readFile(file)).digest('hex');
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw new AssembleError(
      `${command} ${args.join(' ')} failed: ${result.error?.message ?? result.stderr.trim()}`,
    );
  }
}

/** The platform package's package.json: its `os` and `cpu` make npm install only the matching one. */
function platformManifest(launcher, platformKey, wanted, version) {
  const [os, cpu] = platformKey.split('-');
  const manifest = {
    name: wanted.name,
    version,
    description: `The colophon binary for ${wanted.label} (${wanted.target}). Install @colophon-cms/cli, which picks it for you.`,
    homepage: launcher.homepage,
    bugs: launcher.bugs,
    repository: launcher.repository,
    os: [os],
    cpu: [cpu],
    files: [`bin/${binaryName(os)}`],
    // Yarn's Plug'n'Play must keep the binary on disk to run it.
    preferUnplugged: true,
    publishConfig: { access: 'public' },
  };
  if (launcher.license) manifest.license = launcher.license;
  return manifest;
}

/**
 * Builds every package into `out`. Returns the directories it wrote.
 * @param {{version: string, archives: string, out: string}} options
 */
export async function assemble({ version, archives, out }) {
  if (!SEMVER.test(version)) throw new AssembleError(`${version} is not a semantic version`);
  const launcher = JSON.parse(await readFile(path.join(source, 'package.json'), 'utf8'));
  if (launcher.version !== version) {
    throw new AssembleError(
      `packages/cli-npm/package.json is ${launcher.version} but the release is ${version}; ` +
        'raise both together (docs/modules/19-oss.md, "Versions")',
    );
  }
  let sums;
  try {
    sums = parseSums(await readFile(path.join(archives, 'SHA256SUMS'), 'utf8'));
  } catch (error) {
    throw new AssembleError(`no SHA256SUMS in ${archives}: ${error.message}`);
  }
  let licence = null;
  try {
    await stat(path.join(repository, 'LICENSE'));
    licence = path.join(repository, 'LICENSE');
  } catch {
    // No licence confirmed yet (O-014): the release workflow refuses to publish without one.
  }

  await rm(out, { recursive: true, force: true });
  const written = [];
  for (const [platformKey, wanted] of Object.entries(PLATFORMS)) {
    const windows = platformKey.startsWith('win32');
    const archiveName = `colophon-cli-${version}-${wanted.target}.${windows ? 'zip' : 'tar.gz'}`;
    const archive = path.join(archives, archiveName);
    const expected = sums.get(archiveName);
    if (!expected) throw new AssembleError(`SHA256SUMS doesn't list ${archiveName}`);
    let actual;
    try {
      actual = await sha256(archive);
    } catch (error) {
      throw new AssembleError(`${archiveName} is missing: ${error.message}`);
    }
    if (actual !== expected) {
      throw new AssembleError(`${archiveName} doesn't match SHA256SUMS (${actual}, expected ${expected})`);
    }

    const binary = binaryName(platformKey.split('-')[0]);
    const scratch = await mkdtemp(path.join(tmpdir(), 'colophon-npm-'));
    try {
      if (windows) run('unzip', ['-q', '-o', archive, binary, '-d', scratch]);
      else run('tar', ['-xzf', archive, '-C', scratch, binary]);
      const dir = path.join(out, `cli-${platformKey}`);
      await mkdir(path.join(dir, 'bin'), { recursive: true });
      await copyFile(path.join(scratch, binary), path.join(dir, 'bin', binary));
      await chmod(path.join(dir, 'bin', binary), 0o755);
      const manifest = platformManifest(launcher, platformKey, wanted, version);
      await writeFile(path.join(dir, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
      await writeFile(
        path.join(dir, 'README.md'),
        `# ${wanted.name}\n\nThe \`colophon\` binary for ${wanted.label} (\`${wanted.target}\`), from ` +
          `Colophon CMS ${version}. Install [@colophon-cms/cli](https://www.npmjs.com/package/@colophon-cms/cli) ` +
          'instead: it depends on this package and runs the binary for you.\n',
      );
      if (licence) await copyFile(licence, path.join(dir, 'LICENSE'));
      written.push(dir);
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  }

  const dir = path.join(out, 'cli');
  await mkdir(path.join(dir, 'bin'), { recursive: true });
  await mkdir(path.join(dir, 'lib'), { recursive: true });
  for (const file of launcher.files) await copyFile(path.join(source, file), path.join(dir, file));
  await chmod(path.join(dir, 'bin', 'colophon.js'), 0o755);
  await copyFile(path.join(source, 'README.md'), path.join(dir, 'README.md'));
  if (licence) await copyFile(licence, path.join(dir, 'LICENSE'));
  const { scripts: _scripts, devDependencies: _dev, ...published } = launcher;
  published.optionalDependencies = Object.fromEntries(
    Object.values(PLATFORMS).map((p) => [p.name, version]),
  );
  await writeFile(path.join(dir, 'package.json'), `${JSON.stringify(published, null, 2)}\n`);
  written.push(dir);
  return written;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { values } = parseArgs({
    options: { version: { type: 'string' }, archives: { type: 'string' }, out: { type: 'string' } },
  });
  if (!values.version || !values.archives || !values.out) {
    process.stderr.write('usage: assemble.mjs --version <version> --archives <dir> --out <dir>\n');
    process.exit(2);
  }
  try {
    for (const dir of await assemble(values)) process.stdout.write(`assembled ${dir}\n`);
  } catch (error) {
    if (!(error instanceof AssembleError)) throw error;
    process.stderr.write(`error: ${error.message}\n`);
    process.exit(1);
  }
}
