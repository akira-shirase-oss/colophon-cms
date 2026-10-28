<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **project** is one website's content, people and settings, isolated from every other project by the database. This page creates one as the administrator, invites a second person into it, and shows what each role may do. It assumes the local stack and the server from [Run it locally](/get-started/quickstart/), with `dev-owner` signed in.

## Create the project in the Studio

Only an administrator creates projects. On the local stack that is `dev-owner`, whom `scripts/migrate.sh` made the administrator.

1. Open `http://localhost:58000/studio/`. The **Projects** page says *Signed in as Dev Owner, administrator*. With no project yet, it shows **Create your first project**; later, the **New project** button opens the same form.
2. Type a **Name**, what people call it, for example *My site*.
3. The **URL name** follows the name as you type (`my-site`) until you edit it yourself. It is used in addresses and by the command line.
4. Under **Start with**, choose a starting point: **A small blog** (Articles with a title, a URL name and a body), **A documentation site** (Pages with a title, a URL name and a body, beside Articles) or **Start blank** (Articles with a title and a body). You can add and change kinds of content later. <Status done="3.7" />
5. Press **Create project**. The Studio opens the new project's overview.

| Field | Rule |
|---|---|
| Name | 1 to 200 characters, without control characters; spaces around it are trimmed |
| URL name | 1 to 63 lowercase letters, digits and hyphens, not starting or ending with a hyphen; unique in the installation |

A URL name that is already taken is refused with *a project with this slug already exists*, the server's own words. The command line names a project by it (`--project my-site`); the API and the public addresses use the project's **id**, which **Settings** shows with a copy button, beside the project's **Public API** address.

The starting point is applied right after the project is made, through the same content-type API anyone could call. If that part fails, the project exists anyway, and a message says how to finish it in **Settings → Content types**.

Someone who isn't the administrator sees no **New project** button: with no project, the page says *You're not in a project yet* and how to ask for an invitation.

## What a new project holds

Creating a project does three things in one transaction:

- it creates the project's one **environment**, `production`. Every content path in the API names it (`…/environments/production/…`). Other environments, such as staging, aren't built yet;
- it creates the built-in **Articles** content type: a required **Title** (one line, up to 200 characters) and a required **Body** (rich text);
- it makes you the project's **owner**.

Then the starting point adds what it adds: the blog gives Articles a required **URL name** made from the title; the documentation site adds a **Pages** type.

The sidebar has **Overview**, **Search**, the **Content** list (Articles, and Pages if you chose the documentation site), **Publish requests** and **Releases** under *Publishing*, and **Members** and **Settings** under *Project*. Its foot says what readers have now: *Nothing live yet* until the first release.

The overview starts with a checklist of first things to do: write the first article, publish it (or ask for review), connect the website, invite a teammate, and see the first receipt. Each item ticks itself off from what really happened, and you can hide the list. <Status done="3.7" />

## Invite someone

Owners invite people from **Members**, with a link that works once.

1. Open **Members**. It lists who belongs, their role and when they joined.
2. Under **Invite someone**, fill in **Who is it for?** (only you see it, to recognise the invitation, for example *Sam, copy editor*) and pick a **Role**.
3. Press **Create invitation link**. The link is shown **once**: press **Copy** and send it privately.

The link works for one person, once, and expires after 72 hours. It isn't tied to an email address, so whoever holds it can use it: treat it like a key. **Pending invitations** lists the unused ones, each with **Revoke**.

To try it locally, invite `dev-editor` as an editor, open the link in a private window, sign in as `dev-editor` (password `colophon-dev`) and press **Join**. Nothing happens until **Join**; **Don't join** leaves the invitation unused. [Joining a project](/editors/joining/) describes the screen the invited person sees.

## The four roles

Every project starts with four built-in roles. One evaluator decides every permission, and the server checks it on every request, whether it comes from the Studio, the command line or a script:

| | Owner | Developer | Editor | Publisher |
|---|---|---|---|---|
| Read content, content types and members | yes | yes | yes | yes |
| Write entries | yes | yes | yes | yes |
| Review publish requests | yes | no | no | yes |
| Publish | yes | no | no | yes |
| Change content types | yes | yes | no | no |
| Manage website connections | yes | yes | no | no |
| Invite people, change roles, remove members | yes | no | no | no |
| Make and change roles | yes | no | no | no |

Every member can see the other members and leave the project. A project always keeps at least one owner: the last owner can't leave or be demoted (*a project must keep at least one owner*). An owner changes someone's role with the selector beside their name, and removes them with **Remove**.

## Roles you define <Status done="R.3" />

Owners make roles of their own under **Settings → Roles**: give it a name, press **Create and choose what it may do**, and the role's page shows a grid of what it may do, for *Everything* (every content type, including ones added later) and for each content type: **See**, **Create**, **Edit**, **Approve**, **Publish** and **Change fields**. Below the grid are the powers over the whole project: **Invite people and change their roles**, **Make and change roles**, **Manage where releases go** and **Manage API tokens** (API tokens arrive in a later step <Status planned="R.4" />). Press **Save role**.

**Limited permissions** are narrower than the grid: only some fields, or only the entries a member created themselves, for example *edit the titles of any article and every field of their own*. Add one with **What** (see, create or edit), **Where**, **Only their own entries** and the fields, then **Add this limit**; the role's page lists each in words, such as *Edit: Articles, only their own entries*.

The owner role can't be changed, a role someone holds can't be deleted, and nobody can give a role more than their own role holds. Members and invitations offer every role of the project. [Roles and permissions](/concepts/roles-and-permissions/) explains the model.

## The same through the API

An administrator's session or CLI token can create a project directly:

```http
POST /api/v1/projects
Content-Type: application/json

{"slug": "my-site", "name": "My site"}
```

It answers `201 Created` with the project (`id`, `slug`, `name`, and your `role`, `owner`). Anyone who isn't the administrator gets `403` with *only an administrator can create projects; ask one to invite you to a project*; an installation nobody has set up answers *this installation has no administrator yet: enter the setup code its server printed when it started, or its operator runs `colophon admin bootstrap`*. A taken slug is `409`. The starting points are ordinary content-type calls made after this one ([Content types](/reference/api/content-types/)).

Invitations are `POST /api/v1/projects/{project}/invitations` with `{"role": "editor", "label": "Sam, copy editor"}`; the answer carries the link, once. [Projects, members and invitations](/reference/api/projects/) has every route and error.

## Next

[Your first article](/get-started/first-article/).
