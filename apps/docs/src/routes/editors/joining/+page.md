<script>
	import Status from '#lib/components/Status.svelte';
</script>

You work in a **project**: one website's content, people and settings. You get into a project in one of two ways: you created it as the administrator, or someone invited you.

## Signing in

The Studio has no passwords of its own. On its first page, *Welcome to Colophon*, press **Sign in**: it sends you to your organisation's sign-in page (on a local install, the development Keycloak) and back to the Studio afterwards.

A session ends 12 hours after you signed in, or after 2 hours without use. If it ends while you are writing, the Studio says *Signed out: changes kept on this device*; your unsaved text stays in this browser, and you can save it after you sign in again. There is no sign-out button in the Studio yet.

Signed in but not in any project yet, you see *No projects yet* and *Ask a project owner for an invitation link.*

## Joining with an invitation link

A project's owner creates an invitation in **Members**, chooses the role you get, and sends you the link privately. The link:

- works **once**, for one person, and expires after **72 hours**;
- isn't tied to an email address, so anyone holding it can use it: keep it to yourself, like a key;
- can be revoked by the owner at any time.

Open the link. If you aren't signed in, the page says *Sign in first; this page will show the invitation afterwards*. Then it shows who invited you, the project and the role, for example *Dev Owner invited you to join My site as editor, which lets you change content*, with your name and until when the invitation works.

Nothing happens until you press **Join**. Joining makes whoever is signed in a member, so check the name first. Afterwards the page says *You're now an editor of My site*, with a link to the project. **Don't join** leaves the invitation unused until it expires.

| What the page says | What it means | What to do |
|---|---|---|
| *This invitation has expired, was revoked, or was already used.* | The link can't be used any more. | Ask the person who sent it for a new one. |
| *This link has no invitation in it.* | The link was cut short when it was copied. | Ask for the link again. |
| *you are already a member of this project* | You belong already; nothing changed. | Open the project from the Studio's first page. |

## What each role may do

Every project has four roles, and the server checks them on every request, whether it comes from the Studio or anywhere else.

| | Write and edit | Review | Publish | Content types and website connections | Invite people and change roles |
|---|---|---|---|---|---|
| Owner | yes | yes | yes | yes | yes |
| Publisher | yes | yes | yes | no | no |
| Editor | yes | no | no | no | no |
| Developer | yes | no | no | yes | no |

Everyone in a project can read its content and see who else belongs. **Members** lists them; **Leave project**, beside your own name, takes you out. A project always keeps at least one owner, so the last owner can't leave.

Today's invitation screens describe the publisher role as able to change "content (publishing comes later)". That text is out of date: publishers review and publish, as the table says.

Roles that owners create, with names and powers of their own, are being built. <Status building="R.2" /> There, the invitation shows the role's own name and description, such as *Editor: Writes and edits content*. Rules per field and "only my own articles" come later. <Status planned="R.3" />

## Creating a project

Only an administrator creates projects, from the Studio's first page. The first administrator is made once, by the person who installs Colophon, from a terminal; signing in by itself grants nothing. If you see *Only an administrator can create projects*, ask a project's owner for an invitation instead.

## Technical details

Invitations are `POST /api/v1/projects/{project}/invitations` (owners only), looked up and accepted through `/api/v1/invitations/lookup` and `/api/v1/invitations/accept`. The token travels after the `#` in `/studio/invite#…`, which browsers never send to a server, and is stored only as a hash. Roles are `owner`, `developer`, `editor` and `publisher`, decided by one policy table on `main` (`colophon_domain::policy`). [Roles and permissions](/concepts/roles-and-permissions/) and [Projects, members and invitations](/reference/api/projects/) have the rest.
