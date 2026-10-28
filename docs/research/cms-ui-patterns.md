# How Strapi and Sanity lay out their admin, and what Colophon takes (2026-10-28)

The maintainer looked at the content-type builder (one long form with every setting of every field open, code names
beside each label) and asked for "a CMS", built the way Strapi and Sanity build their UI. These notes record what
those two do, from their documentation and source, and what the Studio borrows and why. Where a claim couldn't be
checked against a source it says so. The builder that came out of this is described in
[modules/11-studio.md](../modules/11-studio.md) and D-079.

## Strapi v5

**Main navigation.** A rail of icons: Home, Content Manager, Releases, Media Library, Content-Type Builder,
plugins, Marketplace, and Settings at the bottom ([useMenu.ts](https://github.com/strapi/strapi/blob/develop/packages/core/admin/admin/src/hooks/useMenu.ts),
[quick start](https://docs.strapi.io/cms/quick-start)). On desktop the rail has no labels, only tooltips
([LeftMenu.tsx](https://github.com/strapi/strapi/blob/develop/packages/core/admin/admin/src/components/LeftMenu.tsx));
users call losing v4's labelled menu a regression ([forum](https://forum.strapi.io/t/strapi-v5-cannot-expand-left-menu/52619)).
Links carry the permissions they need and are hidden from users without them, which the docs note doesn't protect
the page itself ([admin navigation](https://docs.strapi.io/cms/plugins-development/admin-navigation-settings)).

**Content-Type Builder** ([docs](https://docs.strapi.io/cms/features/content-type-builder)). Its own item in the main
navigation, with a sub-navigation of Collection Types, Single Types and Components (grouped by category, searchable),
and "Create new collection type" and "Create new component". It edits only in development; in production it is
read-only with a message to restart in development mode
([strings](https://github.com/strapi/strapi/blob/develop/packages/core/content-type-builder/admin/src/translations/en.json)).

- **A type is a list of rows** ([AttributeRow.tsx](https://github.com/strapi/strapi/blob/develop/packages/core/content-type-builder/admin/src/components/AttributeRow.tsx)):
  a drag handle, the type's icon, the name in bold with a red `*` when required, the type in words (linking a
  relation's target or a component), a "conditional" badge, a New / Modified / Deleted badge until saved, and Edit and
  Delete. Components and dynamic zones expand as nested rows. Rows are reordered by dragging.
- **Adding a field** opens "Select a field for your collection type": a grid of cards, each an icon, a name and one
  line ("Relation: Refers to a Collection Type", "Enumeration: List of values, then pick one", "Dynamic zone:
  Dynamically pick component when editing content").
- **A field's settings** are a modal with Basic settings (name, and the choice that defines the kind, such as short
  or long text) and Advanced settings (default, RegExp pattern, private, required, unique, length limits), plus a
  condition. Its buttons are "Add another field" and "Finish".
- **One Save** applies everything pending (with undo, redo and discard beside it), then the server restarts. The
  type's own settings (display name, API IDs, draft and publish) are in an Edit modal, which warns that renaming an
  API ID orphans the old data.
- Labels, help text and the layout of the edit form are set elsewhere, in the Content Manager's "Configure the
  view" ([Content Manager](https://docs.strapi.io/cms/features/content-manager)).

**Content Manager.** A list with search, filters (status among them), a column picker and bulk publish; an edit view
with the form on the left and a narrow right panel holding the entry's status, Save and Publish, Draft and Published
tabs, and Cmd/Ctrl+Enter to save ([draft and publish](https://docs.strapi.io/cms/features/draft-and-publish),
[EditViewPage.tsx](https://github.com/strapi/strapi/blob/develop/packages/core/content-manager/admin/src/pages/EditView/EditViewPage.tsx)).

## Sanity Studio (v3 to v6)

**The Structure tool** is panes left to right: document types, then a list of documents, then the form; "the new
pane will open to the right, and the current pane will stack to the left"
([Structure tool](https://www.sanity.io/docs/studio/structure-tool), [Structure Builder](https://www.sanity.io/docs/studio/structure-builder-introduction)).
The top bar holds the workspace menu, new document, search, the tools, releases and the user menu
([StudioNavbar.tsx](https://github.com/sanity-io/sanity/blob/main/packages/sanity/src/core/studio/components/navbar/StudioNavbar.tsx)).

**Lists** show a title, a subtitle and an image per document, chosen in the schema's `preview`
([previews](https://www.sanity.io/docs/studio/previews-list-views)), with a status mark: a ring for unpublished
edits, a filled dot for published ([editor guide](https://www.sanity.io/docs/user-guides/content-operations-cheatsheet)).

**The form** groups fields as tabs with an automatic "All fields" tab ([field groups](https://www.sanity.io/docs/studio/field-groups)),
collapsible fieldsets ([object type](https://www.sanity.io/docs/studio/object-type)), a title and a description per
field, required ones starred. Validation errors block publishing and warnings don't
([validation](https://www.sanity.io/docs/studio/validation)).

**The footer** says the document's status and when it was last changed; Publish is the one button, other actions are
in a menu ([status bar actions](https://github.com/sanity-io/sanity/blob/main/packages/sanity/src/structure/panes/document/statusBar/DocumentStatusBarActions.tsx));
the status opens Review changes and History ([history](https://www.sanity.io/docs/user-guides/history-experience)).

**Schemas are code** (`defineType`, `defineField`; [schema types](https://www.sanity.io/docs/studio/schema-types)).
There is no visual builder, which is the main difference from Strapi.

## What Colophon borrows

| Pattern | From | In the Studio |
|---|---|---|
| The builder as its own place in the main navigation, with its own list of types and components | Strapi | *Content types*, beside Members and Settings, shown only to roles that may change content types (D-079) |
| A type as a compact list of rows: handle, icon, label, kind in words, badges, Edit and Delete; structures nested under their row | Strapi | `FieldList.svelte`, with Title, Required, Unique, *Shown only when…* and New badges |
| Add a field as a grid of cards with one plain line each, grouped | Strapi | `FieldPicker.svelte`: Basics, Links, Structure, Advanced; what can't go in a place is greyed with the reason |
| A field's settings in a dialog with Basic and Advanced tabs | Strapi | `FieldDialog.svelte`: Basics (label, required, help, what defines the kind) and Advanced (limits, allowed form with examples, unique, default, *Show only when*, code name) |
| Staged changes and one Save | Strapi | One save bar, "You have unsaved changes · Discard · Save", and New badges; leaving the page with changes asks first |
| The type's own settings in a small dialog | Strapi | *Edit type*: its name and *Just one entry* (the address name is shown, fixed) |
| One primary action, status said in words, a description under each field | Sanity, Strapi | Already the Studio's rules (D-059); the builder keeps one black button, Save, and says each kind in a line |

## What it doesn't, and why

- **An icon-only main rail.** Editors who aren't developers need labels; Strapi's own users miss them. The Studio's
  sidebar keeps labels and collapses to icons only on request.
- **Editing schemas only in development, with a restart.** Colophon changes content types on the running server, and
  the server checks every change against stored entries before it lands (D-055); there is nothing to restart.
- **Developer words in the builder.** No "API ID (singular/plural)", "RegExp" or "Dynamic zone": a field has a
  *code name*, shown only in its advanced settings and fixed once saved, so renaming a label can never cut off data;
  patterns are an *allowed form* with examples; a dynamic zone is *page sections*.
- **Labels and layout set in a second tool.** Strapi sets a field's type in the builder and its label and help in
  "Configure the view"; here they are all in the field's own settings.
- **Schemas only in code.** Sanity's choice suits developer teams; Colophon's maintainer wants a non-technical owner
  to be able to add a field. Definitions stay plain JSON in the API, so code can still write them.
- **Panes that stack left to right** (Sanity). The Studio is a page per thing with breadcrumbs, which reads better on
  a phone, where reviewing has to work (D-059, rule 7).
