<script>
	import Status from '#lib/components/Status.svelte';
</script>

You work in a **project**: one website's content, people and settings. You get into a project in one of two ways: you created it as the administrator, or someone invited you.

## Signing in

The Studio has no passwords of its own. On its first page, *Welcome to Colophon*, press **Sign in**: it sends you to your organisation's sign-in page (on a local install, the development Keycloak) and back to the Studio afterwards. New here? **Create an account** opens the sign-in provider's own form for making one, when it offers one (the local install's does), and brings you back signed in. An account by itself lets you into nothing: projects are joined by invitation. <Status done="3.7" />

A session ends 12 hours after you signed in, or after 2 hours without use. If it ends while you are writing, the Studio says *Signed out: changes kept on this device*; your unsaved text stays in this browser, and you can save it after you sign in again. There is no sign-out button in the Studio yet.

Signed in but not in any project yet, you see *You're not in a project yet*: projects are joined by invitation link. The page tells you the name you signed in with, to give to whoever invites you, and what to ask for: *Editor* to write, *Publisher* to approve and publish.

## Joining with an invitation link

A project's owner creates an invitation in **Members**, chooses the role you get, and sends you the link privately. The link:

- works **once**, for one person, and expires after **72 hours**;
- isn't tied to an email address, so anyone holding it can use it: keep it to yourself, like a key;
- can be revoked by the owner at any time.

Open the link. If you aren't signed in, the page says *Sign in first, or create an account if you don't have one; this page shows the invitation afterwards.*, with **Sign in** and **Create an account**. Then it shows who invited you, the project, and the role with what it is for, for example *Dev Owner invited you to join My site as Editor: Writes and edits content.*, with the name you are signed in as and until when the invitation works.

Nothing happens until you press **Join**. Joining makes whoever is signed in a member, so check the name first. Afterwards the page says *You joined My site as Editor.*, with a link to the project. **Don't join** leaves the invitation unused until it expires (*Not joined. The invitation stays unused until it expires.*).

| What the page says | What it means | What to do |
|---|---|---|
| *This invitation has expired, was revoked, or was already used. Ask the person who sent it for a new one.* | The link can't be used any more. | Ask for a new one. |
| *This link has no invitation in it. Ask the person who invited you for a new link.* | The link was cut short when it was copied. | Ask for the link again. |
| *you are already a member of this project* | You belong already; nothing changed. | Open the project from the Studio's first page. |

## What each role may do

Every project starts with four roles, and the server checks your role on every request, whether it comes from the Studio or anywhere else.

| | Write and edit | Review | Publish | Content types and website connections | Invite people and change roles |
|---|---|---|---|---|---|
| Owner | yes | yes | yes | yes | yes |
| Publisher | yes | yes | yes | no | no |
| Editor | yes | no | no | no | no |
| Developer | yes | no | no | yes | no |

Everyone in a project can read its content and see who else belongs. **Members** lists them; **Leave project**, beside your own name, takes you out. A project always keeps at least one owner, so the last owner can't leave.

Owners can also make roles of their own, with names, descriptions and powers they choose, for example a copy desk that may edit but not publish. <Status done="R.2" /> A role can be narrowed to some fields of an article, or to the articles you created yourself; then the editor leaves out what your role doesn't show, and greys out what it can't change ([Writing and saving](/editors/writing/)). <Status done="R.3" /> Everyone in a project can read what each of its roles may do, under **Settings**.

## Creating a project

Only an administrator creates projects, from the Studio's first page; anyone else is pointed to an invitation instead. The first administrator is made once, by the person who installs Colophon: either from a terminal, or in the Studio, where a new installation asks the first person who signs in for the one-time setup code its server printed (*Finish setting up Colophon*). Signing in by itself grants nothing. <Status done="3.7" />

## Technical details

Invitations are `POST /api/v1/projects/{project}/invitations` (owners only), looked up and accepted through `/api/v1/invitations/lookup` and `/api/v1/invitations/accept`. The token travels after the `#` in `/studio/invite#…`, which browsers never send to a server, and is stored only as a hash. The built-in roles are `owner`, `developer`, `editor` and `publisher`; roles are data, one evaluator decides every request (`colophon_domain::policy`), and owners manage roles at `/api/v1/projects/{project}/roles`. [Roles and permissions](/concepts/roles-and-permissions/) and [Projects, members and invitations](/reference/api/projects/) have the rest.
