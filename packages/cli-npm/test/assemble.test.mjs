// The npm packages are assembled from a release's archives, checked against its SHA256SUMS, with
// one version everywhere; `npm pack --dry-run` lists exactly the files each package should ship
// (docs/modules/08-cli.md, "Install"). The archives here are made by the release workflow's own
// packaging script (scripts/release-package.sh) from stand-in binaries.

import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { appendFileSync, chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { assemble, parseSums } from '../scripts/assemble.mjs';

const require = createRequire(import.meta.url);
const { PLATFORMS } = require('../lib/platform.js');
const here = path.dirname(fileURLToPath(import.meta.url));
const source = path.resolve(here, '..');
const repository = path.resolve(source, '../..');
const launcher = JSON.parse(readFileSync(path.join(source, 'package.json'), 'utf8'));
const version = launcher.version;
const posix = process.platform !== 'win32';

const made = [];
after(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});
function scratch() {
  const dir = mkdtempSync(path.join(tmpdir(), 'colophon-assemble-'));
  made.push(dir);
  return dir;
}

/** A fake release: every target's archives, made by scripts/release-package.sh, and SHA256SUMS. */
function fakeRelease(releaseVersion = version) {
  const root = scratch();
  const dist = path.join(root, 'dist');
  for (const { target } of Object.values(PLATFORMS)) {
    const bin = path.join(root, 'bin', target);
    mkdirSync(bin, { recursive: true });
    const exe = target.includes('windows') ? '.exe' : '';
    for (const name of ['colophon', 'colophon-server']) {
      writeFileSync(path.join(bin, name + exe), `#!/bin/sh\necho "${name} ${releaseVersion} for ${target}"\n`);
      chmodSync(path.join(bin, name + exe), 0o755);
    }
    execFileSync(path.join(repository, 'scripts/release-package.sh'), [target, releaseVersion, dist, bin], { stdio: 'ignore' });
  }
  const sums = execFileSync('sh', ['-c', 'cat ./*.sha256'], { cwd: dist, encoding: 'utf8' });
  writeFileSync(path.join(dist, 'SHA256SUMS'), sums);
  return { root, dist };
}

function packed(dir) {
  const out = execFileSync('npm', ['pack', '--dry-run', '--json'], { cwd: dir, encoding: 'utf8' });
  const [result] = JSON.parse(out);
  return { name: result.name, version: result.version, files: result.files.map((f) => f.path).sort() };
}

test('the launcher carries the workspace version and depends on every platform package at it', () => {
  const cargo = readFileSync(path.join(repository, 'Cargo.toml'), 'utf8');
  const workspace = /\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/.exec(cargo);
  assert.ok(workspace, 'Cargo.toml has [workspace.package] version');
  assert.equal(version, workspace[1], 'raise packages/cli-npm/package.json with Cargo.toml (module 19, "Versions")');
  assert.deepEqual(
    launcher.optionalDependencies,
    Object.fromEntries(Object.values(PLATFORMS).map((p) => [p.name, version])),
  );
  assert.equal(launcher.name, '@colophon-cms/cli', 'the @colophon scope belongs to someone else (D-062)');
});

test('npm pack --dry-run lists the launcher, its lookup and the readme, nothing else', { skip: !posix }, () => {
  assert.deepEqual(packed(source), {
    name: '@colophon-cms/cli',
    version,
    files: ['README.md', 'bin/colophon.js', 'lib/platform.js', 'package.json'],
  });
});

test('SHA256SUMS lines are read as sha256sum writes them', () => {
  const sums = parseSums(`${'a'.repeat(64)}  one.tar.gz\n${'b'.repeat(64)} *two.zip\nnot a line\n`);
  assert.deepEqual([...sums], [['one.tar.gz', 'a'.repeat(64)], ['two.zip', 'b'.repeat(64)]]);
});

test('every platform package is assembled from its archive, with os, cpu and one version', { skip: !posix }, async () => {
  const { root, dist } = fakeRelease();
  const out = path.join(root, 'npm');
  const written = await assemble({ version, archives: dist, out });
  assert.equal(written.length, 6);

  for (const [key, { name, target }] of Object.entries(PLATFORMS)) {
    const [os, cpu] = key.split('-');
    const binary = os === 'win32' ? 'colophon.exe' : 'colophon';
    const dir = path.join(out, `cli-${key}`);
    const manifest = JSON.parse(readFileSync(path.join(dir, 'package.json'), 'utf8'));
    assert.equal(manifest.name, name);
    assert.equal(manifest.version, version);
    assert.deepEqual(manifest.os, [os]);
    assert.deepEqual(manifest.cpu, [cpu]);
    assert.deepEqual(manifest.repository, launcher.repository, 'npm provenance checks the repository');
    assert.equal(manifest.bin, undefined, 'only the launcher puts colophon on PATH');
    const file = path.join(dir, 'bin', binary);
    assert.equal(readFileSync(file, 'utf8'), `#!/bin/sh\necho "colophon ${version} for ${target}"\n`, 'the CLI, not the server');
    assert.equal(statSync(file).mode & 0o111, 0o111, 'executable');
    assert.deepEqual(packed(dir), { name, version, files: ['README.md', `bin/${binary}`, 'package.json'] });
  }

  const cli = path.join(out, 'cli');
  const manifest = JSON.parse(readFileSync(path.join(cli, 'package.json'), 'utf8'));
  assert.equal(manifest.scripts, undefined, 'no development scripts in the published launcher');
  assert.deepEqual(manifest.optionalDependencies, launcher.optionalDependencies);
  assert.deepEqual(packed(cli).files, ['README.md', 'bin/colophon.js', 'lib/platform.js', 'package.json']);

  // The assembled launcher runs the assembled binary for this machine, when it is one we build.
  const own = PLATFORMS[`${process.platform}-${process.arch}`];
  if (own) {
    const run = spawnSync(process.execPath, [path.join(cli, 'bin/colophon.js'), '--version'], {
      encoding: 'utf8',
      env: { ...process.env, NODE_PATH: '' },
      cwd: root,
    });
    // The launcher resolves platform packages from node_modules, which `npm/` isn't; installing
    // the packed tarballs is the release workflow's job. Here it must say so plainly.
    assert.equal(run.status, 6, run.stderr);
    assert.match(run.stderr, new RegExp(`${own.name.replace('/', '\\/')}, which isn't installed`));
  }
});

test('an archive that does not match SHA256SUMS is refused', { skip: !posix }, async () => {
  const { root, dist } = fakeRelease();
  appendFileSync(path.join(dist, `colophon-cli-${version}-x86_64-unknown-linux-musl.tar.gz`), 'tampered');
  await assert.rejects(
    assemble({ version, archives: dist, out: path.join(root, 'npm') }),
    /colophon-cli-.*-x86_64-unknown-linux-musl\.tar\.gz doesn't match SHA256SUMS/,
  );
});

test('a missing target or a version that is not the launcher\'s is refused', { skip: !posix }, async () => {
  const { root, dist } = fakeRelease();
  const sums = readFileSync(path.join(dist, 'SHA256SUMS'), 'utf8').split('\n').filter((line) => !line.includes('windows'));
  writeFileSync(path.join(dist, 'SHA256SUMS'), sums.join('\n'));
  await assert.rejects(
    assemble({ version, archives: dist, out: path.join(root, 'npm') }),
    /SHA256SUMS doesn't list colophon-cli-.*-x86_64-pc-windows-msvc\.zip/,
  );
  await assert.rejects(
    assemble({ version: '9.9.9', archives: dist, out: path.join(root, 'npm') }),
    /packages\/cli-npm\/package\.json is .* but the release is 9\.9\.9/,
  );
  await assert.rejects(assemble({ version: 'v1', archives: dist, out: path.join(root, 'npm') }), /not a semantic version/);
});
