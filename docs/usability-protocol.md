# Usability protocol: the five-editor gate

The exact procedure for one of v0.1's two human gates ([END_GOAL](END_GOAL.md)):

> Five representative editors attempt draft → preview → review → publish; at least four
> succeed without help (spec §05 usability gate), and the failures are written up.

The spec's own wording (§05) adds "accept one AI edit" between preview and review. The
end goal's list supersedes it (docs order of authority: `DECISIONS.md` > module docs >
`END_GOAL.md` > spec); if an AI suggestion panel has shipped by the time the sessions run,
add "accept one suggestion" as a fifth task and record it the same way.

**Status:** prepared, not run. The gate ticks when five people have been through this file
and the write-up is committed. The install side of the same sentence in the end goal is
`deploy/runbooks/install-feedback.md` with its mechanical rehearsal
`scripts/clean-install.sh`.

## What counts as the gate

| | |
|---|---|
| Cohort | Five editors from the persona table below, each in a separate session |
| Task set | Sign in, then draft → preview → review → publish (below) |
| Bar | At least four of the five complete every task with no help |
| Help | Anything the facilitator says or does beyond repeating the written brief counts as help; see the strict rule |
| Evidence | One filled session record per editor, plus the gate summary, committed under `docs/usability/` |

"Record the failures and revise the design" (spec §05): every friction event becomes an
issue or a named won't-fix, and the summary says which. A gate run on a later cohort after
fixes is honest evidence; the first cohort's record is never overwritten.

## The five editors

Spec §05 says "representative editors" and, in the same breath, "test with actual editors
and assistive technology". The cohort is built to that sentence:

| # | Persona | Why they are in the room |
|---|---|---|
| 1 | A staff writer who publishes every day; uses WordPress or Notion; non-technical | The primary editor the Studio is for |
| 2 | A marketing or content person; an occasional CMS user; comfortable in a spreadsheet | Tests whether ordinary office-tool experience transfers |
| 3 | A technical editor or docs engineer; comfortable with JSON and Git | The technical upper bound; must still not need help |
| 4 | A site owner who edits rarely, perhaps monthly; rusty on any CMS | Tests whether the flow is re-discoverable, not just learnable |
| 5 | A keyboard-only or screen-reader editor | §05's accessibility gate in human form |

Rules:

- **Nobody who has seen the Studio before.** Not the maintainer, not a reviewer of the
  feature, not someone who watched a demo. The facilitator's own dry run is preparation,
  not a session.
- One editor per session; nobody watches another's session first.
- Editors may decline recording. Say what is captured before starting (screen recording is
  optional and off by default; the friction log happens regardless).
- Pay or thank them as is ordinary for a half hour of someone's time; this is research,
  not an examination.

## What the facilitator prepares

1. **A clean installation.** `scripts/first-run.sh` on the maintainer's machine gives a
   fresh server and Studio on `http://localhost:58400` against an empty database, with the
   setup code printed to the terminal; or rehearse the newcomer path with
   `scripts/clean-install.sh` and serve the clone's Studio on :58090. Either way the
   sessions run on a Studio that has never seen the facilitator's own content.
2. **Accounts.** One account per editor: sign up through the Studio's own form where the
   gate is "a new team member joins", or invitation links from the owner where the session
   includes "you have been invited". `dev-owner`/`dev-editor` are fine inside the
   development realm; do not coach people past the sign-in page.
3. **A project.** One project per session, created from the blog starting point (so an
   Articles collection and a couple of collections exist), with the editor as a member
   with the `editor` role. For editor 5, whatever assistive technology they bring,
   configured by them, never by the facilitator.
4. **The briefs**, printed or pasted, the exact words below, identical for everyone.
5. **The friction log**: the template in this file, one per session.
6. **A second seat for the review task**: someone who can play reviewer (approving or
   requesting changes) without coaching the editor. The facilitator may be the reviewer
   only if they can stay in that role.

## The session script

Say this, verbatim, before handing over the brief:

> "This is research on the product, not on you. There are no wrong moves. Please think
> aloud: say what you are looking for, what you expect, and when something confuses you.
> I cannot answer questions while you work, but I will write down every one. If you want
> to stop, say so. You have up to forty minutes."

Then hand over the brief and start the clock:

> **Your brief.** You run the blog for a small site and it runs on this system. An
> account and a project have been set up for you; your sign-in details are on the card.
> Write a short post titled *First morning*, a few paragraphs and one list will do; check
> how it will read, put it through the review the project expects, publish it, and then
> check what a reader will see. Work the way you would at your desk.

The card carries the Studio's URL, the account name and the password, nothing else.

## The tasks, and what done means

Task names are the editor's intent, not button labels; where the product answers the
intent is part of the data.

| # | Task | Done means | Stop conditions (mark failed) |
|---|---|---|---|
| 0 | Sign in | The project's home screen is reached with the card's credentials | Cannot sign in within five minutes; ends in a loop the editor cannot escape |
| 1 | Draft | A new article exists with the title and the body the brief asks for, and the editor trusts it is saved | No entry created after ten minutes; editor believes a draft was saved that was not (record it; that is product data) |
| 2 | Preview | The editor looks at what a reader will get for the post, the entry's Reader view being the product's answer today, and can say what it shows | Never finds any way to look at the post as a reader; or looks and cannot tell published from unpublished (record the confusion verbatim) |
| 3 | Review | The post is submitted for review and a review happens, approved by the reviewer, or by the editor where the project's policy visibly allows it | No review submitted; the editor approves something without noticing they did |
| 4 | Publish | The approved change is published and a release exists | Nothing published after ten minutes; editor believes it published when it did not (record it) |
| 5 | Check the receipt | The editor can say, correctly, what state the release is in: saved, published in the CMS, website update pending, verified, or needs attention | The editor reads a pending or failed state as live, or vice versa |

Task 0 is not part of the spec's flow, but no editor reaches task 1 without it; a failure
still counts against the session. Between tasks the facilitator says only "next part of
the brief", never which button.

## "Without help": the strict rule

- The facilitator may **repeat the brief** and may say **"do what you would normally
  do"**. Nothing else.
- A question the product or its own words answers (a label, a tooltip, an error with a
  next action) is **success**, not help.
- Any other facilitator answer, gesture, or "where would you look next?" is a **help
  event**: the session continues so friction still gets recorded, but that editor cannot
  count toward the four.
- A task abandoned is a failed task. An editor may finish the flow helped for the record;
  mark it so.
- Timing is data, not a bar: record it, never enforce it beyond the forty-minute session
  and the per-task stops.

## The friction log

One line per event, taken during the session. Verbatim quotes beat paraphrase.

```markdown
| time | task | what happened (verbatim where possible) | severity | help given? | outcome |
|---|---|---|---|---|---|
| 04:12 | draft | "Where do I even make a post?", hunted the sidebar for ~40 s | confusion | no | found Articles |
| 07:31 | preview | opened the Reader view, read "not out yet" as an error | blocker candidate | no | recovered on re-read |
```

Severity, judged at write-up time, not in the room:

- **blocker**: the task could not complete without help
- **confusion**: visible hesitation or a wrong turn, recovered alone
- **rough edge**: noticed and worked around without slowing much
- **note**: an observation worth keeping that cost nothing

## The write-up

Within a day of each session, fill the record below and commit it as
`docs/usability/<YYYY-MM-DD>-<persona>-session.md`. After the fifth session, commit
`docs/usability/<YYYY-MM-DD>-gate-summary.md`.

```markdown
# Session <n>: <persona short name> (<date>)

- Editor: <persona row; nothing identifying beyond the cohort table>
- Installation: <what served the session: first-run.sh port, commit>
- Session length: <minutes>; recording: yes/no
- Result: <completed unaided | completed helped | stopped at task N>
- Help events: <count and one line each>
- Task table: one line per task: time, done/failed, notes
- Friction log: <the table>
- Editor's own words on the experience: <one or two quotes>
- Facilitator's observations: <what the log cannot show: hesitation patterns, what they read aloud>
```

The summary states the verdict honestly:

```markdown
# Usability gate summary (<date range>)

- Cohort: <the five personas, one line each>
- Verdict: <N of 5 completed draft → preview → review → publish without help; gate met / not met>
- Per-editor results: <one line each>
- Friction themes: <the events grouped; count and severity>
- What changed because of it: <issues opened, fixes landed, or "won't fix" with the reason>
- Protocol faults, if any: <where the brief or setup, not the product, caused a failure; recorded, not hidden>
- Next gate run: <planned cohort if the design is revised, per spec §05>
```

## What this protocol deliberately does not test

- Installing Colophon, the other human gate (`deploy/runbooks/install-feedback.md`).
- Developer work: schemas, roles, tokens, the CLI.
- Speed, beyond the raw timings the log keeps.
- Whether editors *like* it: satisfaction is a note in the record, never evidence the
  gate asks for.
