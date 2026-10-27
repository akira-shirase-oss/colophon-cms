<script>
	import Status from '#lib/components/Status.svelte';
</script>

Who may do what is decided in one place, checked on every request, and backed by the database so a bug in a handler can't leak another project's data.

## Projects are isolated by the database

Every table that belongs to a project carries its id, and PostgreSQL's row-level security hides other projects' rows from the role the server runs as (`colophon_app`), which can't bypass it. A project you aren't a member of answers `404`, exactly like one that doesn't exist. Tests deny a second project by id on every route, with colliding names.

## One policy table

`colophon_domain::policy::allows(role, action)` is the only place a role is interpreted, deny by default, and a unit test holds the whole table:

| Action | Owner | Developer | Editor | Publisher |
|---|---|---|---|---|
| Read | yes | yes | yes | yes |
| Write content | yes | yes | yes | yes |
| Change content types | yes | yes | no | no |
| Manage website connections | yes | yes | no | no |
| Review | yes | no | no | yes |
| Publish | yes | no | no | yes |
| Manage members | yes | no | no | no |

Any member lists the members and may leave. A project always keeps an owner: owner rows are locked before a change, so concurrent changes queue, and a constraint checked at commit refuses anything that would leave none (`409`). A role change re-checks the caller's own role under that lock, so a role read just before a concurrent demotion grants nothing.

## Invitations

An owner invites by creating a link, `/studio/invite#<token>`, that grants one role in one project, works once, and expires after 72 hours. The token travels after the `#`, which browsers never send to a server, so it can't end up in access logs or a `Referer` header; it is stored only as a SHA-256 hash. Accepting binds the membership to whoever is signed in. Invitations aren't tied to an email address, because an email address never establishes identity here: whoever holds the link can use it, and the owner can revoke it at any time.

## Signing in

- **People** sign in through OpenID Connect (Keycloak locally). Colophon verifies the ID token itself, with tests for each known attack. Sessions are rows in PostgreSQL; the cookie holds only a random token whose hash is stored, so a database leak yields no usable session. A session ends 12 hours after sign-in or after 2 hours unused, and revoking one takes effect on the very next request.
- **Browser writes** need an `Origin` that matches the server's public origin and an `X-CSRF-Token` equal to the session's.
- **The command line** signs in with a device code that a signed-in person approves in the Studio, and gets a personal bearer token for 30 days, revoked by `colophon logout`. A token can't approve another device, so a token can't mint more tokens.

## The first administrator

Only administrators create projects. The first one is made once, by the operator, from a terminal, with the database owner's credentials: `colophon admin bootstrap --subject S`. Run again for the same person it changes nothing; for anyone else it is refused. No HTTP route can create an administrator, so an installation behind a permissive sign-in provider can't be filled with projects by strangers.

## Not built yet

- Custom roles, permissions per field and "only entries I created", with API tokens of the same shape (D-058, decided on the step 3.6 branch). <Status planned="milestone R" />
- Asking for a fresh sign-in before sensitive changes such as role changes. <Status planned="not scheduled" />
- Scoped, budgeted credentials for scripts and agents. <Status planned="P5" />
- More than one administrator, through a grant flow. <Status planned="not scheduled" />

## Design

[Module 03, auth](/design/modules/03-auth/); decisions D-031, D-032, D-041, D-042 and D-043 in the [decisions](/design/decisions/). On GitHub: [docs/modules/03-auth.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/03-auth.md).
