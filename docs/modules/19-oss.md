# 19 · open-source hygiene (repo root, `.github/`)

> Upkeep is part of the product: licence, governance, CI security, releases and an honest support policy.

| | |
|---|---|
| Milestones | P0 (licence decision path, basic files, CI security), P6 (release process, SBOM, signed artifacts, support policy) |
| Proposed owner | The maintainer decides (licence, governance); Claude scaffolds |
| Depends on | verification (release evidence), operations (artifacts) |
| Used by | contributors, users, security reporters |
| Spec | §35 |

## Why it exists

An open-source project is judged by how it is run as much as by its code: whether its licence is clear, whether
security reports have a private path, whether releases can be verified. The spec treats maintenance as part of the
product (spec §35).

## End goal

The repository has a confirmed licence, governance files, a private security-reporting path, protected branches and
a release process that produces verifiable artifacts. Its support promises match what one maintainer can actually
keep.

## Responsibilities

- The licence and licence inventory.
- Governance files.
- Contribution rules.
- CI supply-chain security.
- Releases.
- The support policy.
- The maintenance cadence.

## Not its job

- Legal clearance. The licence choice is a proposed policy, not legal advice (spec §35).

## Breakdown

### Licence (spec §35)
- Apache-2.0 is proposed for the core, CLI and SDK, subject to the maintainer's approval before the first public
  release (O-014).
- Contributed code, templates, fonts, media and dependencies are reviewed separately.
- A licence inventory is kept.
- Redis 8 is offered under AGPLv3, RSALv2 and SSPLv1, not the old BSD licence. Redistribution and hosting arrangements
  are reviewed before shipping images (O-002). The dependency bundle is never described as uniformly permissive.

### Repository files
- README, LICENSE, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, GOVERNANCE, CHANGELOG, CODEOWNERS.
- Contributors attest to their rights through a DCO-style sign-off.
- Private security reporting goes through GitHub security advisories. Personal contact details are not exposed by
  default.

### CI and supply chain (spec §35)
- Protected release branches, and hardware-backed MFA for maintainers.
- Minimal token scopes and short-lived publishing credentials.
- Untrusted pull-request jobs are isolated from secrets.
- Third-party CI actions are pinned; images are scanned; signing identities are verified.
- Daily RustSec/OSV advisory checks and `cargo deny`, triaged by a human by severity.
- OpenSSF Scorecard is used as a diagnostic, not a certification.

### Versions (D-062, built in milestone V)
- **One version.** `version` in the root `Cargo.toml`'s `[workspace.package]` is the version of every crate, so the
  server, worker and CLI always share it; the npm packages (`@colophon-cms/cli` and its platform packages, later
  `@colophon-cms/sdk`) carry the same one, and the release tag is `v` plus it.
- **Semantic versioning 2.0.** Before 1.0 a minor release may change `/api/v1` (SemVer item 4) and a patch release
  may not. From 1.0, `/api/vN` is the contract: a breaking change ships as a new API version, served beside the old
  one for the support window below.
- **Pre-releases.** `0.1.0-alpha.N` until P6's v0.1 gate, then `-beta.N` and `-rc.N` as needed, then `0.1.0`. The
  version is `0.1.0-alpha.1` since milestone V.
- **Raising it:** change `[workspace.package] version` and `packages/cli-npm/package.json` (its `version` and its
  five `optionalDependencies`) in one commit (a test fails when they differ), then tag that commit `v<version>`.
- **What every binary reports.** `--version` prints `<version> (commit <sha>, built <YYYY-MM-DD>)`, and the server
  serves the same at `GET /version` with the API versions it serves ([api](04-api.md)). A build script stamps it
  through `crates/colophon-version`. The commit comes from `COLOPHON_COMMIT` if set (a packager building a source
  tarball), else `.cargo_vcs_info.json` (a crate built from a registry), else `git rev-parse HEAD` when that
  repository tracks the package, else `unknown`. The date is `SOURCE_DATE_EPOCH` when set (the release workflow sets
  it to the tagged commit's time, so the stamp is reproducible), else the day the stamp was made, which in a
  development build can be earlier than the latest compile. Only the two binaries' crates rebuild on a new commit.
- **Compatibility.** The CLI checks the server's version first and explains a mismatch in plain words
  ([cli](08-cli.md), "Versions"). The SDK and the MCP server will follow the same rule when they exist.

### Release workflow (D-062, milestone V): written, not yet run in CI
`.github/workflows/release.yml`. Nothing in it has run on GitHub yet; what ran locally is listed under "Tests and
evidence".
- **Triggers.** A pushed tag `v<version>` releases. Run by hand (workflow_dispatch) on any branch it is a **dry
  run**: it builds and checks every artifact, keeps them as workflow artifacts for seven days, and publishes and
  signs nothing (an attestation from a public repository goes to Sigstore's public transparency log, so only tag
  runs make one).
- **plan:** the version from `scripts/release-version.sh`, which fails a tag that isn't `v` plus the workspace
  version. A tag run also stops without a `LICENSE` file (OSS2, O-014) or a `## <version>` section in `CHANGELOG.md`
  (OSS1); a dry run reports both as warnings. `SOURCE_DATE_EPOCH` is the commit's time, so build stamps are
  reproducible.
- **build**, one job per target, without caches: `aarch64-apple-darwin` (macOS 15), `x86_64-apple-darwin`
  (cross-compiled on the same Apple-silicon runner, so it is not run there), `x86_64-unknown-linux-musl`
  (Ubuntu 24.04), `aarch64-unknown-linux-musl` (Ubuntu 24.04 on Arm) and `x86_64-pc-windows-msvc` (Windows 2025).
  Linux uses musl, with aws-lc's C code compiled by `musl-gcc`, and a step checks the binaries are static; Windows
  uses aws-lc's prebuilt NASM objects, and `.gitattributes` checks every text file out with LF there too (CRLF
  would break the scripts and change the embedded migrations' checksums, so a Windows binary would refuse a database
  a Linux one migrated). Each native job runs both binaries' `--version` and checks the version and
  commit. `scripts/release-package.sh` packs `colophon-cli-<version>-<target>.tar.gz` and
  `colophon-server-<version>-<target>.tar.gz` (`.zip` on Windows), one binary each, owned by root, each with its
  SHA-256.
- **sbom:** `cargo-cyclonedx` 0.5.9 (installed `--locked`) writes a CycloneDX 1.5 SBOM of each binary's dependencies
  for each target: `colophon-cli-<version>-<target>.cdx.json` and the server's.
- **assemble:** rechecks every file against the checksum its job recorded, writes one `SHA256SUMS` (20 files: two
  archives and two SBOMs per target) and checks it.
- **release** (tag runs only): `actions/attest-build-provenance` signs SLSA build provenance for every file in
  `SHA256SUMS`, then `gh release create --draft` attaches them all (marked pre-release for a pre-release version).
  The maintainer checks the draft and publishes it by hand ("artifacts are verified before announcement"). Users
  verify with `sha256sum -c SHA256SUMS` and `gh attestation verify <file> --repo akira-shirase-oss/colophon-cms`.
- **npm-pack** (both modes): runs `packages/cli-npm`'s tests, assembles the six npm packages from the archives
  (each checked against `SHA256SUMS`), packs them, installs the launcher and the Linux x64 package from those
  tarballs into an empty project and checks that `npx colophon --version` reports this release and commit.
- **npm-publish** (tag runs only, after **release**): runs in the `npm` environment, so it waits for a maintainer's
  approval, and first refuses to go on if that environment has no required reviewers. With `NPM_TOKEN` from the
  environment's secrets it publishes the five platform packages and then the launcher, each with `--provenance` and
  `--access public`, under the dist-tag `next` for a pre-release (`latest` otherwise); a package version already on
  npm is skipped, so a rerun after a partial failure finishes the job. Nothing is ever published from a laptop.
- Every third-party action is pinned to a full commit SHA, as in `ci.yml`. Known gaps: the Studio's static files are
  not in the server archive yet (the container images, still to build, will carry them); the musl server uses musl's
  allocator, which is slower than glibc's under load, and its throughput has not been measured.

### Releases (spec §35)
- Semantic versioning.
- Each release ships:
  - a changelog and a compatibility table;
  - an SBOM and checksums;
  - signed provenance (SLSA is referenced; no compliance level is claimed);
  - an upgrade and rollback guide.
- Artifacts are verified before announcement.

### Support policy (spec §35)
- The current stable minor and the previous minor are supported, with a proposed 90-day transition, and a published
  security matrix.
- Breaking changes are announced explicitly. Destructive upgrades never run automatically.
- No 24/7 response promise and no unlimited free support for a solo project.

### Maintenance cadence (spec §35)

| Cadence | Responsibility |
|---|---|
| Every pull request | Tests, formatting, dependency/licence changes, docs, permission and migration review |
| Daily (automated) | RustSec/OSV advisories, dependency updates, CI health; human triage by severity |
| Weekly | Bugs, security reports, stalled changes, upstream component fixes, community questions |
| Monthly | Patch release when justified; upgrade tests, docs review, published known limitations |
| Quarterly | Restore drill, performance rerun, threat-model review, supported-version review, maintainer-access audit |
| Every release | Changelog, compatibility table, SBOM, checksums, signed provenance, upgrade/rollback guide, artifact verification |

Roughly one working day in five is reserved for maintenance once users depend on releases (spec §35).

### Repository home
- The repository will live under the `akira-shirase-oss` GitHub organization. Creating it needs the maintainer's
  confirmation.

## Invariants

| ID | Invariant |
|---|---|
| OSS1 | No release without a changelog, SBOM, checksums and signed provenance. |
| OSS2 | No public release before the licence is confirmed. |
| OSS3 | Untrusted pull-request code never runs with access to secrets. |
| OSS4 | Support promises never exceed what the maintainers can keep. |

## Interfaces

- Repository files, `.github/` workflows and security policy, release artifacts.

## Failure modes

| Situation | Behaviour |
|---|---|
| A dependency gets an advisory | Daily check flags it; triaged by severity; patch release if exploitable |
| Maintainer capacity shrinks | Reduce supported scope or announce maintenance mode with migration paths (spec §35) |

## Tests and evidence

- Milestone V, run locally on 2026-10-28 (macOS, Apple silicon): `scripts/release-version.sh` with no tag, a
  mismatched tag and the right one; the `plan` step's script in its three modes (dry run warns, a mismatched tag
  fails, a matching tag without `LICENSE` and a changelog entry fails); a release build for `aarch64-apple-darwin`
  packed by `scripts/release-package.sh` (checksums verified, root-owned entries) and the `.zip` path with stand-in
  binaries; `cargo-cyclonedx` 0.5.9 on this workspace for `x86_64-unknown-linux-musl` (one SBOM per member; the
  CLI's lists 217 components, with no macOS or Windows crates); ShellCheck 0.11.0 on `scripts/*.sh` and
  actionlint 1.7.12 on the workflows, both clean. The npm packages: `npm test` in `packages/cli-npm` (19 tests);
  assembled from a release directory holding this Mac's real `aarch64-apple-darwin` archive and stand-ins for the
  other four, packed with `npm pack`, then installed into an empty project, where `npx --no-install colophon
  --version` ran the real binary (`colophon 0.1.0-alpha.1 (commit f733870…, built 2026-10-28)`); `npx` with the
  launcher alone exited 6 with the "isn't installed" message. npm 11.16 skipped the four optional packages it
  couldn't find on the registry. `npm publish` was never run. The workflow itself has not run on GitHub.
- CI checks:
  - `cargo deny` (advisories, licences, bans, sources);
  - secret scanning;
  - pinned-action verification.
- Release dry runs, with artifact signatures checked.

## Build order

1. **P0:** licence decision path, README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, CI security basics.
2. **P6:** GOVERNANCE, CODEOWNERS, release workflow, SBOM, signing, support matrix.

## Open decisions

- **O-014:** licence confirmation.
- Cache licensing is settled: Valkey (D-015).
