'use strict';
// Which prebuilt `colophon` binary this machine needs, and where npm put it.
//
// @colophon-cms/cli carries no binary itself. Each platform's binary is its own package with `os`
// and `cpu` fields, listed as an optional dependency, so npm installs only the one that matches
// (the way esbuild and Biome ship theirs). Everything here is a pure function of what it is given,
// so the tests can pretend to be any platform.

const path = require('node:path');

/** One package per `${process.platform}-${process.arch}`, built from the Rust target beside it. */
const PLATFORMS = Object.freeze({
  'darwin-arm64': { name: '@colophon-cms/cli-darwin-arm64', target: 'aarch64-apple-darwin', label: 'macOS on Apple silicon' },
  'darwin-x64': { name: '@colophon-cms/cli-darwin-x64', target: 'x86_64-apple-darwin', label: 'macOS on Intel' },
  'linux-arm64': { name: '@colophon-cms/cli-linux-arm64', target: 'aarch64-unknown-linux-musl', label: 'Linux on 64-bit Arm' },
  'linux-x64': { name: '@colophon-cms/cli-linux-x64', target: 'x86_64-unknown-linux-musl', label: 'Linux on x86-64' },
  'win32-x64': { name: '@colophon-cms/cli-win32-x64', target: 'x86_64-pc-windows-msvc', label: 'Windows on x86-64' },
});

/** The CLI contract's exit code for an internal failure (docs/modules/08-cli.md): the install is incomplete. */
const INSTALL_BROKEN = 6;

const CARGO_INSTALL =
  'cargo install --locked --git https://github.com/akira-shirase-oss/colophon-cms colophon-cli';

/** A failure to find the binary, with a message that says what to do. */
class LauncherError extends Error {}

function binaryName(platform) {
  return platform === 'win32' ? 'colophon.exe' : 'colophon';
}

/**
 * Finds the `colophon` binary to run.
 *
 * @param {object} options
 * @param {string} options.platform `process.platform`
 * @param {string} options.arch `process.arch`
 * @param {Record<string, string | undefined>} options.env `process.env`; `COLOPHON_BINARY` overrides the search
 * @param {string} options.version this launcher's version; the platform package must match it
 * @param {(id: string) => string} options.resolve `require.resolve` from the launcher
 * @param {(file: string) => boolean} options.exists `fs.existsSync`
 * @param {(file: string) => any} options.readJson reads and parses a JSON file
 * @returns {string} the binary's path
 * @throws {LauncherError}
 */
function findBinary({ platform, arch, env, version, resolve, exists, readJson }) {
  const override = env.COLOPHON_BINARY;
  if (override) {
    if (!exists(override)) {
      throw new LauncherError(`COLOPHON_BINARY is set to ${override}, which doesn't exist.`);
    }
    return override;
  }

  const platformKey = `${platform}-${arch}`;
  const wanted = PLATFORMS[platformKey];
  if (!wanted) {
    const supported = Object.values(PLATFORMS).map((p) => p.label).join(', ');
    throw new LauncherError(
      `@colophon-cms/cli has no prebuilt colophon for ${platform} ${arch}; it has one for ${supported}. ` +
        `Build it with Rust instead: ${CARGO_INSTALL}`,
    );
  }

  let manifest;
  try {
    manifest = resolve(`${wanted.name}/package.json`);
  } catch {
    throw new LauncherError(
      `colophon for ${wanted.label} comes in the package ${wanted.name}, which isn't installed. ` +
        'npm installs it with @colophon-cms/cli unless optional dependencies are turned off: reinstall ' +
        `without --omit=optional (or --no-optional), or install it yourself: npm install ${wanted.name}@${version}`,
    );
  }

  const installed = readJson(manifest).version;
  if (installed !== version) {
    throw new LauncherError(
      `@colophon-cms/cli is ${version} but ${wanted.name} is ${installed}; they must match. ` +
        `Reinstall: npm install @colophon-cms/cli@${version}`,
    );
  }

  const binary = path.join(path.dirname(manifest), 'bin', binaryName(platform));
  if (!exists(binary)) {
    throw new LauncherError(
      `${wanted.name} is installed but has no bin/${binaryName(platform)}; reinstall @colophon-cms/cli.`,
    );
  }
  return binary;
}

module.exports = { PLATFORMS, INSTALL_BROKEN, LauncherError, binaryName, findBinary };
