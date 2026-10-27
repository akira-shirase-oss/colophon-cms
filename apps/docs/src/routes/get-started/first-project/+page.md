<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **project** is one website's content, people and settings, isolated from every other project by the database. This page creates one as the administrator, invites a second person into it, and shows what each role may do. It assumes the local stack and the server from [Run it locally](/get-started/quickstart/), with `dev-owner` signed in.

## Create the project in the Studio

Only an administrator creates projects. On the local stack that is `dev-owner`, whom `scripts/migrate.sh` made the administrator.

1. Open `http://localhost:58000/studio/`. The **Projects** page says *Signed in as Dev Owner, administrator*.
2. Under **New project**, type a **Name**, what people call it, for example *My site*.
3. The **Slug (in addresses and the CLI)** follows the name as you type (`my-site`) until you edit it yourself.
4. Press **Create project**. The Studio opens the new project's overview.

| Field | Rule |
|---|---|
| Name | 1 to 200 characters, without control characters; spaces around it are trimmed |
| Slug | 1 to 63 lowercase letters, digits and hyphens, not starting or ending with a hyphen; unique in the installation |

A slug that is already taken is refused with *a project with this slug already exists*. The slug is used by the command line (`--project my-site`); the API and the public addresses use the project's **id**, which **Settings** shows with a copy button.

Someone who isn't the administrator sees no **New project** form, only *Only an administrator can create projects. To work on one, ask its owner for an invitation link.*

## What a new project holds

Creating a project does three things in one transaction:

- it creates the project's one **environment**, `production`. Every content path in the API names it (`…/environments/production/…`). Other environments, such as staging, aren't built yet;
- it creates the built-in **Articles** content type: a required **Title** (one line, up to 200 characters) and a required **Body** (rich text);
- it makes you the project's **owner**.

The sidebar has **Overview**, **Search**, the **Content** list (Articles), **Changesets** and **Releases** under *Publishing*, and **Members** and **Settings** under *Project*. Its foot says what readers have now: *Nothing live yet* until the first release.

## Invite someone

Owners invite people from **Members**, with a link that works once.

1. Open **Members**. It lists who belongs, their role and when they joined.
2. Under **Invite someone**, fill in **Who is it for?** (only you see it, to recognise the invitation, for example *Sam, copy editor*) and pick a **Role**.
3. Press **Create invitation link**. The link is shown **once**: press **Copy** and send it privately.

The link works for one person, once, and expires after 72 hours. It isn't tied to an email address, so whoever holds it can use it: treat it like a key. **Pending invitations** lists the unused ones, each with **Revoke**.

To try it locally, invite `dev-editor` as an editor, open the link in a private window, sign in as `dev-editor` (password `colophon-dev`) and press **Join**. Nothing happens until **Join**; **Don't join** leaves the invitation unused. [Joining a project](/editors/joining/) describes the screen the invited person sees.

## The four roles

Every project has four roles. One table decides every permission, and the server checks it on every request, whether it comes from the Studio, the command line or a script:

| | Owner | Developer | Editor | Publisher |
|---|---|---|---|---|
| Read content, content types and members | yes | yes | yes | yes |
| Write entries | yes | yes | yes | yes |
| Review publish requests | yes | no | no | yes |
| Publish | yes | no | no | yes |
| Change content types | yes | yes | no | no |
| Manage website connections | yes | yes | no | no |
| Invite people, change roles, remove members | yes | no | no | no |

Every member can see the other members and leave the project. A project always keeps at least one owner: the last owner can't leave or be demoted (*a project must keep at least one owner*). An owner changes someone's role with the selector beside their name, and removes them with **Remove**.

Today's invitation form describes the publisher role as able to change "content (publishing comes later)". That text is out of date: publishers review and publish, as the table says.

## Roles you define

Roles that owners create and edit, under **Settings → Roles**, are being built. <Status building="R.2" /> On that branch each project starts with the same four roles and exactly their powers, the owner role can't be edited, and a new role is a grid of what it may do per content type (see, create, edit, approve, publish, change fields) plus project powers (invite people, make roles, manage where releases go). Nobody can give a role more than their own role holds. Permissions per field and "only entries I created" come after. <Status planned="R.3" /> [Roles and permissions](/concepts/roles-and-permissions/) explains the model.

## The same through the API

An administrator's session or CLI token can create a project directly:

```http
POST /api/v1/projects
Content-Type: application/json

{"slug": "my-site", "name": "My site"}
```

It answers `201 Created` with the project (`id`, `slug`, `name`, and your `role`, `owner`). Anyone who isn't the administrator gets `403` with *only an administrator can create projects; ask one to invite you to a project*; an installation nobody has bootstrapped answers *this installation has no administrator yet: its operator runs `colophon admin bootstrap`*. A taken slug is `409`.

Invitations are `POST /api/v1/projects/{project}/invitations` with `{"role": "editor", "label": "Sam, copy editor"}`; the answer carries the link, once. [Projects, members and invitations](/reference/api/projects/) has every route and error.

## Next

[Your first article](/get-started/first-article/).
