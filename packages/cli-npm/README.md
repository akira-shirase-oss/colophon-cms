# @colophon-cms/cli

`colophon`, the command line for [Colophon CMS](https://github.com/akira-shirase-oss/colophon-cms), prebuilt for
macOS (Apple silicon and Intel), Linux (x86-64 and 64-bit Arm) and Windows (x86-64). No Rust toolchain needed.

```sh
npx @colophon-cms/cli --version
npm install --global @colophon-cms/cli
colophon login --server https://cms.example.com
```

This package is a small launcher. npm also installs the one package that holds the binary for your platform
(`@colophon-cms/cli-darwin-arm64` and so on, as optional dependencies) and the launcher runs it with your arguments,
passing its exit code through. If you install with `--omit=optional`, the binary is missing and `colophon` says so.
To run a binary you built yourself, set `COLOPHON_BINARY` to its path.

`colophon` checks the server's version before it does anything and tells you plainly when it is too old or too new
for that server. Every release's archives, checksums and signed build provenance are on the
[GitHub releases page](https://github.com/akira-shirase-oss/colophon-cms/releases).
