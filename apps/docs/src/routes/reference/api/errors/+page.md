Every error the server sends, from any route and from its middleware (a wrong method, an oversized body, a timeout, a panic), has the same shape:

```json
{
  "error": {
    "code": "conflict",
    "message": "the draft has changed since that revision; load the current one and try again",
    "details": { "reason": "stale_revision", "pointer": "" },
    "request_id": "0192f3c4-7d1e-7a51-9c2b-5d3e8f6a1b20"
  }
}
```

- **Branch on `code`**, and on `details.reason` when there is one. Never on `message`: it is for people and may change.
- `details` appears when the error is about part of the request: `reason` is a specific code, and `pointer` is an [RFC 6901 JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) into the body (empty for the whole body).
- `request_id` matches the `x-request-id` header. Quote it in a bug report and the logs find the request.
- Internal errors never show their cause: the message is `internal error`, and the cause goes to the server's log.

## Codes

Renaming a code is a breaking change; they are defined in `crates/colophon-api/src/error.rs`, and a test checks that every one is listed here.

| Code | Status | Meaning |
|---|---|---|
| `bad_request` | 400 | A malformed request or header that no more specific code describes, such as a malformed `If-Match`. |
| `malformed_json` | 400 | The body isn't valid JSON, or doesn't have the expected shape. |
| `sign_in_failed` | 400 | Sign-in failed, was cancelled, or its attempt expired or was already used. Details are logged, never shown. |
| `unauthorized` | 401 | Not signed in, or the session or token ended. |
| `forbidden` | 403 | Signed in, but not allowed: a role without the permission, a missing CSRF token, a foreign `Origin`. |
| `not_found` | 404 | No such resource, or one you may not see; the two look the same. |
| `route_not_found` | 404 | No route has this path. |
| `method_not_allowed` | 405 | The path exists, but not for this method. |
| `conflict` | 409 | The request conflicts with the current state; `details.reason` says how, when it can. |
| `gone` | 410 | Delivery only: it was published, and has been taken down. Never cached. |
| `payload_too_large` | 413 | The body is over the limit. |
| `unsupported_media_type` | 415 | The body needs `Content-Type: application/json`. |
| `validation_failed` | 422 | Well-formed, but breaks a rule; `details` names the rule and where. |
| `precondition_required` | 428 | A required `If-Match` or `Idempotency-Key` is missing; the message names it. |
| `internal` | 500 | A bug or an unexpected failure. |
| `unavailable` | 503 | Temporarily unable to serve: the database pool is full (with `Retry-After: 1`) or a dependency is down. |
| `deadline_exceeded` | 504 | The request ran out of time. |

## Conflicts

A `409` carries a reason when the client can act on it, and a header naming the current state:

| `details.reason` | When | Also in the response |
|---|---|---|
| `stale_revision` | A save named a revision that is no longer the draft. | `ETag`: the current revision |
| `stale_version` | A content-type update named an old version. | `ETag`: the current version |
| `stale_changeset` | A changeset action named a hash the changeset no longer has. | `ETag`: the current hash |
| `generation_moved` | Another release committed first. | `Colophon-Generation`: the current generation |
| `idempotency_key_reused` | The key was used before for a different request. | |

Other conflicts have no reason, only a message: a project or content-type slug that is taken, a project that would lose its last owner, publishing a changeset that isn't approved, joining a project you are already in.

## Validation reasons

A `422` always has `details`. The reasons come from three places:

- **Request fields**: `invalid_client_name`, `invalid_comment`, `invalid_decision`, `invalid_entries`, `invalid_generation`, `invalid_kind`, `invalid_label`, `invalid_location`, `invalid_name`, `invalid_role`, `invalid_title`, `invalid_url`, `not_published`, `reserved_slug`, `unknown_entry`.
- **Content and content types**, checked against the definition: `duplicate_field`, `invalid_datetime`, `invalid_fields`, `invalid_id`, `invalid_label`, `invalid_name`, `invalid_text`, `invalid_title_field`, `invalid_type`, `invalid_values`, `missing_field`, `needs_migration`, `not_an_integer`, `not_an_object`, `not_an_option`, `out_of_range`, `too_long`, `unknown_field`, `wrong_type`.
- **Rich text**, with a pointer into the document: the [format's error codes](/reference/rich-text/#error-codes), such as `unsafe_link` or `too_deep`.
