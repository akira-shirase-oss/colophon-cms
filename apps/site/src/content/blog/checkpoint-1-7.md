+++
title = "Checkpoint 1.7: inviting people"
description = "Colophon stops being a one-person tool. An owner invites someone with a link that works once, the invitee sees exactly what they're joining before they join, one small table decides what owners, developers, editors and publishers may do, and a project can never be left without an owner, even when two owners step down at the same moment."
date = 2026-10-26T02:46:24Z

[extra]
kind = "release"
milestone = "P1"
step = "1.7"
short_title = "Inviting people"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 175

[extra.summary]
goal = "Let more than one person work on a project, with roles that mean something, and never lock everyone out."
built = "One policy table for four roles, a members page, single-use invitation links, and a rule that every project keeps an owner."
proof = ["an invitation works once", "a project never loses its last owner", "1,000,015 member requests, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 76aac72"
+++

Until now, whoever created a project was the only person who could ever touch it. This step lets an owner bring other
people in, and gives each of them a role. The interesting parts are the link that carries the invitation, and a rule
that sounds trivial until two people act at the same moment: there must always be an owner.

## What I built

- **Four roles, one table.** Owners can do everything. Developers can do everything except decide who belongs.
  Editors and publishers write content (publishing itself comes in the next milestone). Every action inside a project
  checks this one table, and a role the table doesn't know is allowed nothing.
- **A members page in the Studio.** Everyone sees who belongs and with which role, and can leave. Owners change roles,
  remove people, and invite.
- **Invitation links.** An owner types who it's for and picks a role, and gets a link. It's shown once, works once,
  and expires after three days. The owner can revoke it at any time.
- **Joining.** The person opens the link, signs in if they need to, and sees the project's name, the role, and who
  invited them before anything happens. Nothing changes until they press Join.
- **Every project keeps an owner.** The last owner can't demote or remove themselves. Make someone else an owner first,
  then step down.

## How it works (the short version)

The secret part of an invitation link comes after a `#`: `…/studio/invite#k3J…`. Browsers never send that part to a
server, so it can't end up in a server log, a proxy's records, or the address another site sees you came from. The
page reads it, keeps it in the tab while you sign in, takes it out of the address bar, and sends it to the server only
inside a request body. The server stores a fingerprint of it, never the link itself.

"Always keep an owner" is enforced twice. Before changing an owner, the server locks the project's owners, so two
changes to them wait in line instead of racing. And the database itself refuses to finish any change that would
leave a project with no owner, so even a future bug in the server can't get there.

## Does it actually work?

- **Two people, end to end, against the real server:** an owner invites, the second person looks the invitation up and
  joins, and the same link then does nothing. An editor can write articles but can't change content types, invite
  anyone, or promote themselves. Promoted to developer, they can change content types. The owner can't step down
  until there's a second owner. A revoked link is dead at once. Someone who leaves can no longer see the project.
- **Two owners demoting each other at the same instant:** one wins and one is refused, and the project still has an
  owner. This one runs against the database itself, both changes really in flight together. So does its cousin: an
  owner demoted halfway through their own request can no longer promote or remove anyone.
- **Expired and revoked invitations** are refused, and another project's owner can neither see nor revoke these.
- **In two real browsers:** the invitee opens the link signed out, signs in, lands back on the invitation with the
  secret gone from the address bar, and joins. The owner's page shows the refusal when they try to step down alone,
  and puts the role back. No accessibility problems on either page.
- **A million requests on members and invitations:** member lists, invitation lookups, role changes and new
  invitations, 1,000,015 of them at 1,993 a second, none failed (p99 60 ms). At a steady 1,000 a second, p99 68 ms.
- **175 Rust tests,** 12 Studio tests against the server, 15 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40260753220)).

## Where I messed up

- **A permission checked a moment too early.** The server read your role when your request began. If another owner
  demoted you at that same moment, your request could still promote someone with the rights you had just lost. I
  found it while rereading the "two owners at once" test, before anything shipped. Now the role is checked again
  after the lock, and a test demotes an owner in the middle of their request and watches it refuse.
- **A page that would have hidden someone's content.** Studio addresses look like `/projects/<id>/<content type>`,
  and I put the members page at `/projects/<id>/members`. A content type called "members" would have become
  unreachable in the Studio. I caught it before shipping; `members` and `settings` are now names a content type can't
  take.
- **A label that didn't say what I thought.** Two browser tests looked for the dropdown labelled "Role" and never
  found it: when a dropdown sits inside its label, the browser adds the chosen option to its name, so it was really
  called something like "Role editor: content". The Studio now connects the label to its dropdown explicitly; the
  testbed's test matches the label loosely.
- **My first million-request run missed its goal.** Every request succeeded, but the slowest 1% took up to 277 ms
  against a 100 ms limit, and the steady run missed by 4 ms. I kept both failed results. A shorter run that timed each
  kind of request separately found nothing slow, and the rerun on the final code passed. I didn't pin down why the
  first run was slow; every run now reports each kind of request on its own, so next time it will say which.
- **CI failed on a port nothing was using.** The server started its health checks, which connect out to the database,
  before claiming its own port. The operating system can hand an outgoing connection that same port number, and once
  in many runs it did. The server now claims its port first.
- **A pattern the framework read as code.** I wrote a small text pattern directly in a page template, and the Svelte
  compiler took its first characters for the end of a block. The type checker caught it before any test ran.

## What it doesn't do yet

- Changing a role doesn't ask you to sign in again first. The plan calls for that on sensitive actions; it comes later.
- An invitation isn't tied to an email address, on purpose: an email address never proves who someone is here. So
  whoever holds the link can use it. Send it privately; revoke it if it goes astray.
- Publishers can't publish yet. Until publishing exists, they can do what editors do.
- The very first owner of a fresh install still comes from creating a project. A one-time, safe way to set up the first
  owner is next.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**1.8, the first owner:** a one-time command that makes the first owner of a fresh install, and then can never run
again.
