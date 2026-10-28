#!/usr/bin/env node
'use strict';
// `colophon` from npm: finds the prebuilt binary for this platform (lib/platform.js) and runs it
// with the same arguments and standard streams, then exits as it did. The binary's exit codes are
// the CLI contract (docs/modules/08-cli.md), so they pass through unchanged; when no binary can be
// found this exits 6 and says what to do.

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const { INSTALL_BROKEN, LauncherError, findBinary } = require('../lib/platform.js');
const { version } = require('../package.json');

let binary;
try {
  binary = findBinary({
    platform: process.platform,
    arch: process.arch,
    env: process.env,
    version,
    resolve: require.resolve,
    exists: fs.existsSync,
    readJson: (file) => JSON.parse(fs.readFileSync(file, 'utf8')),
  });
} catch (error) {
  if (!(error instanceof LauncherError)) throw error;
  process.stderr.write(`error: ${error.message}\n`);
  process.exit(INSTALL_BROKEN);
}

const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: true });

// A terminal's Ctrl-C reaches both processes; a signal sent to this one alone (a supervisor
// stopping it) is passed on, so colophon is never left running without its launcher.
const forwarded = ['SIGINT', 'SIGTERM', 'SIGHUP'];
for (const signal of forwarded) {
  process.on(signal, () => child.kill(signal));
}

child.on('error', (error) => {
  process.stderr.write(`error: could not run ${binary}: ${error.message}\n`);
  process.exit(INSTALL_BROKEN);
});

child.on('exit', (code, signal) => {
  if (signal) {
    // Die the same way, so the shell sees what happened to colophon.
    for (const name of forwarded) process.removeAllListeners(name);
    // If the signal somehow doesn't end this process, exit with the shell's code for it.
    process.exitCode = 128 + (os.constants.signals[signal] ?? 0);
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? INSTALL_BROKEN);
});
