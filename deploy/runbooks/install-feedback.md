# Runbook: the independent install, and its friction record

The human half of END_GOAL's v0.1 install gate: *one person other than the maintainer
installs Colophon from the README alone, and the friction is recorded.* This file is the
instructions for running that, and the template the record is written into. Its
mechanical rehearsal is `scripts/clean-install.sh` (D-132), which proves the README's
commands reach `/readyz` verbatim on an isolated stack; the runbook exists because a
person still brings the confusion a script cannot feel.

## Who counts, and the rules

- **The installer** is anyone who has never installed or developed Colophon. A friend, a
  colleague, a contributor new to the repository. One person is the gate; more are
  welcome data.
- **Allowed material:** the repository's own documents: `README.md` first; whatever the
  README itself links to (`deploy/README.md`, the module docs) counts, because a README's
  links are the README. Search engines and the error messages the tools print are part
  of a normal install too.
- **Not allowed:** asking the maintainer anything. Every question that has to be asked is
  a friction event, not a conversation; write the question down verbatim and let the
  installer answer it from the documents or by experiment.
- The installer narrates or keeps their own notes; the observer writes the log. One
  observer, silent, is the ideal: the observer is the friction log, not a guide.
- If the installer gives up, that is a result. Record where and why; it is exactly what
  the gate exists to find.

## The session record

Copy this template to `docs/install-sessions/<YYYY-MM-DD>-<installer>.md` and fill it (a
directory to create on first use). Keep the times honest: the clock, not memory.

```markdown
# Independent install: <installer label> (<date>)

- Installer: <who they are to the project, e.g. "backend developer, no Rust"; nothing identifying>
- Machine: <OS, arch; e.g. "macOS 15, Apple silicon" or "Ubuntu 24.04, x86-64">
- Prior context: <what they knew before starting, e.g. "has used Docker, never Keycloak">
- Allowed material used: <README only | plus linked docs | plus external search (list)>
- Started: <time>  /readyz: <time or never>  finished or gave up: <time>
- Outcome: <installed, /readyz answered | stopped at <step>: <why>>

## The log

| time | README section | what they did | what happened (verbatim where possible) | severity | resolved how |
|---|---|---|---|---|---|
| 0:03 | Development | ran `scripts/stack.sh up` | "docker: command not found" and installed Docker Desktop | environment, not friction | continued |
| 0:41 | Run the server | `cargo run --release` | built for 18 min; asked "is it stuck?" | confusion | kept waiting |

## Friction summary

- <each event in one line, grouped by cause: README gap, environment, tool behaviour>
- What the installer expected to be easier: <their words>
- What surprised them in a good way: <their words; honest records keep both kinds>
```

Severity uses the same scale as the usability protocol (`docs/usability-protocol.md`):
blocker, confusion, rough edge, note. An environment step the README correctly tells them
to take (installing Docker) is time, not friction; record it anyway, marked as
environment.

## The maintainer's post-mortem

After the session, the maintainer answers each friction event in the same file:

```markdown
## Post-mortem

| friction | disposition |
|---|---|
| `npm ci` missing from the README's Studio block | fixed: README now lists it (this branch) |
| "is it stuck?" during a 20-minute build | open: the README should say a cold build takes this long; issue # |
```

Every event ends as a fix, a tracked issue, or a written won't-fix with the reason. Then
the verdict line goes in `docs/END_GOAL.md` only when the record exists: *installed from
the README alone, friction recorded in `docs/install-sessions/…`.*

## What to do with the mechanical rehearsal's record

`scripts/clean-install.sh` writes `perf/results/<stamp>-clean-install-<machine>.json`:
the steps it ran, the seconds each took, the audit entries it re-checked against the
clone's README, and its verdict. Quote its filename in the session record; the
installer's machine details sit next to the rehearsal's, and a gap the rehearsal swears
is closed stays proven closed.
