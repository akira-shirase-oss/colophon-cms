'use strict';
// The launcher picks the right platform package, runs its binary with the same arguments and exit
// code, and explains plainly when no binary can be found (docs/modules/08-cli.md, "Install").
//
// The first tests call findBinary directly. The rest run bin/colophon.js in a child process
// inside a throwaway node_modules tree, with process.platform and process.arch replaced by a
// preloaded module, so one machine can play every platform. The stand-in binaries are shell
// scripts, so those tests run on macOS and Linux only.

const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const { PLATFORMS, INSTALL_BROKEN, LauncherError, binaryName, findBinary } = require('../lib/platform.js');

const source = path.resolve(__dirname, '..');
const { version } = require('../package.json');

// --- findBinary, as a function -------------------------------------------------------------

/** A pretend filesystem: `files` maps each existing path to its JSON content (or true). */
function options(platform, arch, files, env = {}) {
  return {
    platform,
    arch,
    env,
    version,
    resolve(id) {
      const file = path.join('/nm', id);
      if (!(file in files)) throw Object.assign(new Error(`Cannot find module '${id}'`), { code: 'MODULE_NOT_FOUND' });
      return file;
    },
    exists: (file) => file in files,
    readJson: (file) => files[file],
  };
}

/** The files an installed platform package would have. */
function installed(platformKey, packageVersion = version) {
  const { name } = PLATFORMS[platformKey];
  return {
    [path.join('/nm', name, 'package.json')]: { name, version: packageVersion },
    [path.join('/nm', name, 'bin', binaryName(platformKey.split('-')[0]))]: true,
  };
}

test('each supported platform gets its own package and binary name', () => {
  const expected = {
    'darwin-arm64': ['@colophon-cms/cli-darwin-arm64', 'colophon'],
    'darwin-x64': ['@colophon-cms/cli-darwin-x64', 'colophon'],
    'linux-arm64': ['@colophon-cms/cli-linux-arm64', 'colophon'],
    'linux-x64': ['@colophon-cms/cli-linux-x64', 'colophon'],
    'win32-x64': ['@colophon-cms/cli-win32-x64', 'colophon.exe'],
  };
  assert.deepEqual(Object.keys(PLATFORMS).sort(), Object.keys(expected).sort());
  // Every platform package installed at once: each platform must still pick its own.
  const everything = Object.assign({}, ...Object.keys(PLATFORMS).map((key) => installed(key)));
  for (const [key, [name, binary]] of Object.entries(expected)) {
    const [platform, arch] = key.split('-');
    assert.equal(findBinary(options(platform, arch, everything)), path.join('/nm', name, 'bin', binary), key);
  }
});

test('an unsupported platform is told which ones are, and how to build it', () => {
  for (const [platform, arch] of [['freebsd', 'x64'], ['linux', 'ia32'], ['win32', 'arm64'], ['linux', 'riscv64']]) {
    assert.throws(
      () => findBinary(options(platform, arch, {})),
      (error) =>
        error instanceof LauncherError &&
        error.message.includes(`no prebuilt colophon for ${platform} ${arch}`) &&
        error.message.includes('macOS on Apple silicon') &&
        error.message.includes('cargo install --locked --git'),
      `${platform}-${arch}`,
    );
  }
});

test('a missing platform package names itself and the fix', () => {
  assert.throws(
    () => findBinary(options('linux', 'x64', installed('darwin-arm64'))),
    (error) =>
      error instanceof LauncherError &&
      error.message.includes("@colophon-cms/cli-linux-x64, which isn't installed") &&
      error.message.includes('--omit=optional') &&
      error.message.includes(`npm install @colophon-cms/cli-linux-x64@${version}`),
  );
});

test('a platform package of another version is refused', () => {
  assert.throws(
    () => findBinary(options('darwin', 'arm64', installed('darwin-arm64', '0.0.1'))),
    (error) => error instanceof LauncherError && error.message.includes('is 0.0.1; they must match'),
  );
});

test('a platform package without its binary is reported', () => {
  const files = installed('linux-arm64');
  delete files[path.join('/nm', '@colophon-cms/cli-linux-arm64', 'bin', 'colophon')];
  assert.throws(
    () => findBinary(options('linux', 'arm64', files)),
    (error) => error instanceof LauncherError && error.message.includes('has no bin/colophon'),
  );
});

test('COLOPHON_BINARY runs a binary you built yourself, if it exists', () => {
  const env = { COLOPHON_BINARY: '/opt/colophon' };
  assert.equal(findBinary(options('freebsd', 'x64', { '/opt/colophon': true }, env)), '/opt/colophon');
  assert.throws(() => findBinary(options('linux', 'x64', {}, env)), /COLOPHON_BINARY is set to \/opt\/colophon, which doesn't exist/);
});

// --- bin/colophon.js, as a process -----------------------------------------------------------

const posix = process.platform !== 'win32';

const made = [];
after(() => {
  for (const dir of made) fs.rmSync(dir, { recursive: true, force: true });
});

/** A node_modules tree with the real launcher and stand-in platform packages. */
function tree(platformKeys, { packageVersion = version } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'colophon-launcher-'));
  made.push(root);
  const launcher = path.join(root, 'node_modules', '@colophon-cms', 'cli');
  for (const file of ['package.json', 'bin/colophon.js', 'lib/platform.js']) {
    fs.mkdirSync(path.dirname(path.join(launcher, file)), { recursive: true });
    fs.copyFileSync(path.join(source, file), path.join(launcher, file));
  }
  for (const key of platformKeys) {
    const { name } = PLATFORMS[key];
    const dir = path.join(root, 'node_modules', name);
    fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name, version: packageVersion }));
    const binary = path.join(dir, 'bin', binaryName(key.split('-')[0]));
    fs.writeFileSync(
      binary,
      [
        '#!/bin/sh',
        `echo "stand-in colophon from ${name}"`,
        'for arg in "$@"; do echo "arg:[$arg]"; done',
        'case "$1" in exit) exit "$2" ;; die) kill -TERM $$ ;; esac',
        '',
      ].join('\n'),
    );
    fs.chmodSync(binary, 0o755);
  }
  // Loaded before the launcher: pretend to be another platform.
  fs.writeFileSync(
    path.join(root, 'pretend.cjs'),
    "Object.defineProperty(process, 'platform', { value: process.env.PRETEND_PLATFORM });\n" +
      "Object.defineProperty(process, 'arch', { value: process.env.PRETEND_ARCH });\n",
  );
  return root;
}

function launch(root, platformKey, args, env = {}) {
  const [platform, arch] = platformKey.split('-');
  return spawnSync(
    process.execPath,
    ['--require', path.join(root, 'pretend.cjs'), path.join(root, 'node_modules/@colophon-cms/cli/bin/colophon.js'), ...args],
    {
      encoding: 'utf8',
      env: { ...process.env, COLOPHON_BINARY: '', PRETEND_PLATFORM: platform, PRETEND_ARCH: arch, ...env },
    },
  );
}

test('on every platform the launcher runs that platform\'s binary', { skip: !posix }, () => {
  const root = tree(Object.keys(PLATFORMS));
  for (const [key, { name }] of Object.entries(PLATFORMS)) {
    const result = launch(root, key, ['--version']);
    assert.equal(result.status, 0, `${key}: ${result.stderr}`);
    assert.equal(result.stdout, `stand-in colophon from ${name}\narg:[--version]\n`, key);
  }
});

test('arguments, standard streams and exit codes pass through unchanged', { skip: !posix }, () => {
  const root = tree(['linux-x64']);
  const result = launch(root, 'linux-x64', ['content', 'list', 'two words', '', '--json']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    result.stdout,
    'stand-in colophon from @colophon-cms/cli-linux-x64\narg:[content]\narg:[list]\narg:[two words]\narg:[]\narg:[--json]\n',
  );
  for (const code of [2, 3, 4, 5, 6]) {
    assert.equal(launch(root, 'linux-x64', ['exit', String(code)]).status, code, `exit ${code}`);
  }
});

test('a binary killed by a signal makes the launcher end the same way', { skip: !posix }, () => {
  const result = launch(tree(['darwin-arm64']), 'darwin-arm64', ['die']);
  assert.ok(result.signal === 'SIGTERM' || result.status === 143, `signal ${result.signal}, status ${result.status}`);
});

test('without its platform package the launcher exits 6 and says what to install', { skip: !posix }, () => {
  const result = launch(tree(['darwin-arm64']), 'linux-x64', ['--version']);
  assert.equal(result.status, INSTALL_BROKEN);
  assert.equal(result.stdout, '', 'nothing on stdout, which is for results');
  assert.match(result.stderr, /^error: colophon for Linux on x86-64 comes in the package @colophon-cms\/cli-linux-x64, which isn't installed\./);
  assert.match(result.stderr, /--omit=optional/);
});

test('on an unsupported platform the launcher exits 6 and names the alternatives', { skip: !posix }, () => {
  const result = launch(tree(Object.keys(PLATFORMS)), 'freebsd-x64', ['--version']);
  assert.equal(result.status, INSTALL_BROKEN);
  assert.match(result.stderr, /no prebuilt colophon for freebsd x64/);
  assert.match(result.stderr, /cargo install --locked --git https:\/\/github.com\/akira-shirase-oss\/colophon-cms colophon-cli/);
});

test('a platform package from another release is refused, not run', { skip: !posix }, () => {
  const result = launch(tree(['linux-arm64'], { packageVersion: '9.9.9' }), 'linux-arm64', ['--version']);
  assert.equal(result.status, INSTALL_BROKEN);
  assert.match(result.stderr, /is 9\.9\.9; they must match/);
  assert.doesNotMatch(result.stdout, /stand-in/);
});

test('COLOPHON_BINARY overrides the search, even on an unsupported platform', { skip: !posix }, () => {
  const root = tree(['linux-x64']);
  const binary = path.join(root, 'node_modules/@colophon-cms/cli-linux-x64/bin/colophon');
  const result = launch(root, 'freebsd-x64', ['whoami'], { COLOPHON_BINARY: binary });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /arg:\[whoami\]/);
});
