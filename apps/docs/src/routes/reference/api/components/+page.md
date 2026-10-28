<script>
	import Status from '#lib/components/Status.svelte';
</script>

A shared component is a named group of fields that several content types use, like an SEO block, a call to action or an address (step M.4, D-073, migration 0029). It follows an object's rules: nothing in it is unique, it has no defaults or conditions, and it uses no other component. A content type uses one with `{"type": "component", "component": "<slug>"}` anywhere an object can go (a field, a list's items, a block's fields), and the server replaces that with an object marked `"component": "<slug>"` holding the component's current fields. So every stored content type version is a complete description of its content, and publish requests and revisions never follow a pointer. Handler: `crates/colophon-api/src/components.rs`; rules: `crates/colophon-domain/src/schema.rs`. <Status building="M.4" />

## The definition

```json
{
  "name": "SEO",
  "fields": [
    { "id": "title", "label": "Search title", "type": "text", "max_chars": 60 },
    { "id": "description", "label": "Search description", "type": "text", "max_chars": 160, "multiline": true }
  ]
}
```

`name` and `fields` follow a content type's rules (1 to 100 characters; 1 to 100 fields with distinct ids). A component has no `title_field` and no `single`. A field inside it may not be `unique`, have a `default` or a `when` condition, or be another component: those are refused with `invalid_unique`, `invalid_default`, `invalid_condition` and `invalid_component`.

## Changing a component changes every type that uses it, or none

A change writes the component's new version and, in the same transaction and in slug order, a new version of every content type that uses it, each checked against its stored content like any change to it ([Changing a content type](/reference/api/content-types/#changing-a-content-type)). The first type that can't take the change refuses all of it, and the refusal names that type in `details.content_type` (`needs_migration`, or the type's own refusal). A component and the types that use it never disagree. Writes to a content type hold the components it uses shared before taking its own lock; a component change holds the component exclusively before taking the types' locks, so the two never wait on each other in a circle.

## Routes

`{P}` is `/api/v1/projects/{project}/environments/{environment}`. Any member may read components. Creating or changing one takes `schema.change` (owners and developers), since a change reaches every content type that uses it.

### `GET {P}/components`

The current version of each component, by slug: `{"data": [{"slug", "version", "definition", "definition_hash", "created_at"}], "next_cursor": null}`.

### `POST {P}/components`

`{"slug": "seo", "definition": {...}}` with an `Idempotency-Key`. Answers 201 with the component's first version and `ETag: "v1"`. A slug another component has is 409 `component_exists`; a definition that breaks the rules above is 422 with the pointer into `/definition`.

### `GET {P}/components/{slug}`

The current version, with `ETag: "v<version>"`. A slug no component has is 404.

### `PUT {P}/components/{slug}`

A new definition, with `If-Match: "v<version>"` (the version you read) and an `Idempotency-Key`. Answers 200 with the new version, and `content_types`: the content types it gave a new version, as `{"slug", "version"}` in slug order. A stale `If-Match` is 409 `stale_version` with the current `ETag`; a content type that can't take the change refuses all of it, as above, and nothing changes.

## Refusals in a content type that uses one

| Reason | When |
|---|---|
| `unknown_component` | `component` names no component in this environment. |
| `invalid_component` | `component` isn't a slug, or a component uses another component. |
| `unexpanded_component` | Only in checks of an already stored definition: a component field that wasn't replaced by the component's fields. A client never causes it; it means a bug. |

## Not built yet

Deleting a component, components inside components, and a content type built from a component (Strapi's dynamic zones are page-builder blocks here) come later.
