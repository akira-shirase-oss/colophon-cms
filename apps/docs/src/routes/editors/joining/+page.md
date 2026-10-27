You work in a **project**: one website's content, people and settings. You get into a project in one of two ways: you are its administrator, or someone invited you.

## Signing in

The Studio has no passwords of its own. **Sign in** sends you to your organisation's sign-in page (on a local install, the development Keycloak), and back to the Studio afterwards. A session ends 12 hours after you signed in, or after 2 hours without use. If it ends while you are writing, your unsaved text stays on your device and is saved when you sign in again.

## Joining with an invitation link

A project's owner creates an invitation in **Members**, choosing the role you get, and sends you the link privately. The link:

- works **once**, for one person, and expires after **72 hours**;
- isn't tied to an email address, so anyone holding it can use it: treat it like a key;
- can be revoked by the owner at any time.

Open the link. If you aren't signed in, the Studio asks you to sign in first and then shows the invitation: the project, the role and who invited you. Nothing happens until you press **Join**; **Don't join** leaves the invitation unused until it expires. Joining binds the membership to whoever is signed in, so check the name in the corner first.

## What each role may do

One table decides every permission, and the server checks it on every request, whether it comes from the Studio, the command line or a script.

| | Read | Write entries | Review | Publish | Change content types | Website connections | Manage members |
|---|---|---|---|---|---|---|---|
| Owner | yes | yes | yes | yes | yes | yes | yes |
| Publisher | yes | yes | yes | yes | no | no | no |
| Editor | yes | yes | no | no | no | no | no |
| Developer | yes | yes | no | no | yes | yes | no |

Every member can see the other members and can leave. A project always keeps at least one owner: the last owner can't leave or be demoted. Custom roles, permissions per field and "only my own entries" are planned (D-058, recorded on the step 3.6 branch); today there are exactly these four.

## Creating a project

Only an administrator creates projects, from the Studio's home page. The first administrator is made once, by the person who installs Colophon, from a terminal; signing in by itself grants nothing. If you see no **New project** button, ask a project's owner for an invitation instead.
