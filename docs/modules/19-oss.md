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
- **O-002:** cache licensing (Redis or Valkey).
