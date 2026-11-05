//! The `OpenAPI` 3.1 document for the whole HTTP surface — and the route table the router and
//! the document are both built from (D-110).
//!
//! [`TABLE`] is the one declaration of Colophon's routes. Each [`Op`] names its [`Verb`],
//! builds the [`MethodRouter`] for exactly that verb, and carries its own [`OpDoc`]: the
//! operation id, summary, security, parameters, body, success response and refusals the
//! document emits. `lib.rs`'s `routes()` is a loop over this table and nothing else, and
//! [`document()`] renders the same table — so the served document and the served routes
//! cannot disagree about which paths and methods exist. What can still drift is what a doc
//! literal *says* about a route — a hand-scribed schema can lag a serde change, a summary can
//! go stale — and the tests at the bottom mark the boundary honestly: methods and paths are
//! checked against the built router request by request; wording is not.
//!
//! The document is served as `GET /openapi.json`, unauthenticated like `/version`: a client
//! reads it before it signs in, and it names shapes only, never data. The bytes are computed
//! once when the router is built ([`Served`]) and carry an `ETag` for conditional requests.

use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{MethodFilter, MethodRouter, on},
};
use serde_json::{Map, Value, json};

use crate::{
    AppState, BuildInfo, assets, audience, audit, auth, bulk, changesets, collaboration,
    components, content_types, delivery, destinations, device, entries, graphql, health, mcp,
    members, migrations, projects, receipts, rich_text, roles, schedules, setup, tokens, version,
};

/// A verb Colophon's routes use. Naming them in a small enum keeps the table total: the
/// served methods and the documented methods are always the same values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Verb {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Verb {
    /// Every verb, for the drift probe.
    #[cfg(test)]
    const ALL: [Self; 5] = [Self::Get, Self::Post, Self::Put, Self::Patch, Self::Delete];

    /// The `OpenAPI` operation key.
    const fn name(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Put => "put",
            Self::Patch => "patch",
            Self::Delete => "delete",
        }
    }

    /// The filter the router runs the handler under. A GET route also answers HEAD — axum
    /// falls a HEAD request back to its GET endpoint — but the document lists `get` alone.
    const fn filter(self) -> MethodFilter {
        match self {
            Self::Get => MethodFilter::GET,
            Self::Post => MethodFilter::POST,
            Self::Put => MethodFilter::PUT,
            Self::Patch => MethodFilter::PATCH,
            Self::Delete => MethodFilter::DELETE,
        }
    }
}

/// One operation: a verb, the handler that serves it, and what the document says about it.
/// `build` is handed the [`MethodFilter`] `verb` produced, so the method the router serves
/// and the method the document emits cannot be written as two different things.
struct Op {
    verb: Verb,
    build: fn(MethodFilter) -> MethodRouter<AppState>,
    doc: OpDoc,
}

/// A path, written as a shared prefix and a suffix so the table stays short: `route()`
/// joins them at startup. Rust has no const string concatenation, so the join happens when
/// the router or the document is built — cheap, and once per process.
type Path = (&'static str, &'static str);

/// One path and its operations.
pub(crate) struct Route {
    path: Path,
    ops: &'static [Op],
}

impl Route {
    /// The `MethodRouter` holding every operation on this path.
    pub(crate) fn router(&self) -> MethodRouter<AppState> {
        self.ops.iter().fold(MethodRouter::new(), |router, op| {
            router.merge((op.build)(op.verb.filter()))
        })
    }

    /// The whole path, joined.
    pub(crate) fn path(&self) -> String {
        let mut path = String::with_capacity(self.path.0.len() + self.path.1.len());
        path.push_str(self.path.0);
        path.push_str(self.path.1);
        path
    }
}

/// A path or query string parameter, or a request header, as the document lists it.
struct Param {
    name: &'static str,
    kind: Kind,
    required: bool,
    note: &'static str,
}

/// The shape of a parameter or header value.
#[derive(Clone, Copy)]
enum Kind {
    /// Any string; the note says more.
    Str,
    /// A UUID.
    Uuid,
    /// A non-negative integer.
    Int,
    /// `true` or `false`.
    Bool,
    /// An opaque cursor a previous page handed back.
    Cursor,
}

/// A request body.
struct Body {
    /// The name of a `#/components/schemas/` component.
    schema: &'static str,
    /// Accepted content types; the first is what the schema describes.
    media: &'static [&'static str],
    /// What the body is.
    note: &'static str,
}

/// A success response.
struct Res {
    status: u16,
    shape: Shape,
    /// Response headers worth naming (`ETag`, `Location`, …).
    headers: &'static [&'static str],
    /// The content type when it isn't `application/json`.
    media: &'static str,
    note: &'static str,
}

/// What a success body looks like.
enum Shape {
    /// No body.
    Empty,
    /// `{"data": <schema>}`.
    Data(&'static str),
    /// `{"data": [<schema>], "next_cursor": <opaque|string|null>}`.
    Page(&'static str),
    /// `{"data": [<schema>]}` — a whole list without a cursor.
    List(&'static str),
    /// The schema itself, unwrapped.
    It(&'static str),
}

/// A documented failure, on top of the shared `default` response every operation lists.
struct Refusal {
    status: u16,
    /// The stable codes the error envelope can carry (`error.code`, `details.reason`).
    codes: &'static str,
    note: &'static str,
}

/// Who may call an operation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Security {
    /// No credential: health, `/version`, this document, sign-in, the device flow's public
    /// half, and public delivery.
    Public,
    /// A browser session cookie, a personal bearer token, or a project API token — the
    /// credentials every `SignedIn` route takes (an API token only on its own project's
    /// `/environments` routes and the rich-text validator).
    Any,
    /// A person only: a session cookie or a personal bearer token. Project API tokens are
    /// refused (`api_token_not_allowed`, or 404 on another project's routes).
    Person,
    /// A browser session only (device approval happens from a signed-in browser).
    Session,
    /// A project API token only — `{P}/mcp` (D-121). People are refused, whichever credential
    /// they carry: agents authenticate as agents, never as people (AU2).
    ApiToken,
    /// A site user's session — the `colophon_audience` cookie or `Authorization: Bearer`
    /// (D-137). A visitor credential, never a staff one.
    Audience,
}

/// What the document says about one operation.
struct OpDoc {
    /// The `operationId`: stable and unique, so generated clients can name it.
    id: &'static str,
    /// One line, imperative.
    summary: &'static str,
    /// The group the operation belongs to.
    tag: &'static str,
    security: Security,
    query: &'static [Param],
    headers: &'static [Param],
    body: Option<Body>,
    ok: Res,
    /// A second success the operation also answers — a query flag's other shape, like an
    /// import dry run's plan (D-114).
    also: Option<Res>,
    extra: &'static [Refusal],
}

const fn op(verb: Verb, build: fn(MethodFilter) -> MethodRouter<AppState>, doc: OpDoc) -> Op {
    Op { verb, build, doc }
}

const fn route(path: Path, ops: &'static [Op]) -> Route {
    Route { path, ops }
}

// ---------------------------------------------------------------------------
// Shared doc literals

/// A query parameter.
const fn query(name: &'static str, kind: Kind, note: &'static str) -> Param {
    Param {
        name,
        kind,
        required: false,
        note,
    }
}

/// A required header parameter.
const fn header(name: &'static str, kind: Kind, note: &'static str) -> Param {
    Param {
        name,
        kind,
        required: true,
        note,
    }
}

const CURSOR: Param = query(
    "cursor",
    Kind::Cursor,
    "the `next_cursor` a previous page answered",
);
const LIMIT: Param = query("limit", Kind::Int, "page size: 1–100, default 20");
const GENERATION: Param = query(
    "generation",
    Kind::Int,
    "pin the read to one release generation",
);

/// `If-Match`, required — the `ETag` a read carried.
const IF_MATCH: Param = header(
    "if-match",
    Kind::Str,
    "the resource's `ETag`, exactly as a response carried it",
);
/// `Idempotency-Key`, required for retriable writes (D-035).
const IDEMPOTENCY: Param = header(
    "idempotency-key",
    Kind::Str,
    "16–128 of `A-Z a-z 0-9 - _`, scoped to the actor, operation and environment",
);

/// A header that is only sometimes required — its note says when (import's `?dry_run=true`
/// takes no key, D-114).
const fn header_maybe(name: &'static str, kind: Kind, note: &'static str) -> Param {
    Param {
        name,
        kind,
        required: false,
        note,
    }
}

/// `Idempotency-Key` on the import: required for an applied import, unneeded on a dry run.
const IDEMPOTENCY_MAYBE: Param = header_maybe(
    "idempotency-key",
    Kind::Str,
    "16–128 of `A-Z a-z 0-9 - _`, scoped to the actor, operation and environment; required unless `?dry_run=true`, which plans without one",
);

/// The optional site-session credential a delivery read upgrades with (D-137).
const AUDIENCE_BEARER: Param = header_maybe(
    "authorization",
    Kind::Str,
    "`Bearer <token>` — a site user's session; the read gains its audience role, and the answer is marked `private`. Without it the read is anonymous",
);

/// `X-CSRF-Token` on a site session's cookie-carried writes (signout); bearer needs none.
const AUDIENCE_CSRF: Param = header_maybe(
    "x-csrf-token",
    Kind::Str,
    "the session's CSRF token — required when it arrives as the `colophon_audience` cookie, unneeded on `Authorization: Bearer`",
);

/// `?visibility=` on an entry write (D-137): the gate the entry's next publication carries.
const VISIBILITY: Param = query(
    "visibility",
    Kind::Str,
    "`public`, `authenticated` or `role:<name>` — who may read it once published; absent keeps the draft's gate",
);

const fn refusal(status: u16, codes: &'static str, note: &'static str) -> Refusal {
    Refusal {
        status,
        codes,
        note,
    }
}

const NOT_FOUND: Refusal = refusal(
    404,
    "not_found",
    "no such resource — or one the caller may not see, which looks the same",
);
const FORBIDDEN: Refusal = refusal(
    403,
    "missing_permission and friends",
    "signed in, but the role does not allow this",
);
const P428: Refusal = refusal(
    428,
    "precondition_required",
    "a required `If-Match` or `Idempotency-Key` is missing",
);
const BAD_MATCH: Refusal = refusal(400, "bad_request", "a malformed `If-Match`");
const REUSED_KEY: Refusal = refusal(
    409,
    "idempotency_key_reused",
    "this `Idempotency-Key` was used before for a different request",
);

/// The refusals every operation can give even when it doesn't list them: `static` so the
/// response merger can hold them by reference alongside each op's own list.
static UNAUTHORIZED: Refusal = Refusal {
    status: 401,
    codes: "unauthorized",
    note: "not signed in, the credential is invalid, or the token was revoked",
};
static RATE_LIMITED: Refusal = Refusal {
    status: 429,
    codes: "rate_limited",
    note: "over the per-credential or per-address limit; `Retry-After`",
};
static RATE_LIMITED_DELIVERY: Refusal = Refusal {
    status: 429,
    codes: "rate_limited",
    note: "over the per-address delivery limit; `Retry-After`",
};

const fn data(schema: &'static str) -> Res {
    Res {
        status: 200,
        shape: Shape::Data(schema),
        headers: &[],
        media: "",
        note: "",
    }
}

const fn page(schema: &'static str) -> Res {
    Res {
        status: 200,
        shape: Shape::Page(schema),
        headers: &[],
        media: "",
        note: "",
    }
}

const fn created(schema: &'static str) -> Res {
    Res {
        status: 201,
        shape: Shape::Data(schema),
        headers: &[],
        media: "",
        note: "",
    }
}

const fn ok(status: u16, shape: Shape, note: &'static str) -> Res {
    Res {
        status,
        shape,
        headers: &[],
        media: "",
        note,
    }
}

const fn etagd(res: Res) -> Res {
    Res {
        headers: &["ETag"],
        ..res
    }
}

const fn json(schema: &'static str) -> Body {
    Body {
        schema,
        media: &["application/json"],
        note: "",
    }
}

/// The doc fields every operation fills; `..OpDoc::MIN` starts a literal.
const MIN: OpDoc = OpDoc {
    id: "",
    summary: "",
    tag: "",
    security: Security::Person,
    query: &[],
    headers: &[],
    body: None,
    ok: Res {
        status: 200,
        shape: Shape::Empty,
        headers: &[],
        media: "",
        note: "",
    },
    also: None,
    extra: &[],
};

// ---------------------------------------------------------------------------
// The table. `lib.rs` serves exactly these routes; `document()` emits exactly these ops.

/// `/api/v1/projects/{project}`.
const PR: &str = "/api/v1/projects/{project}";
/// `/api/v1/projects/{project}/environments/{environment}` — `P` in module 04.
const P: &str = "/api/v1/projects/{project}/environments/{environment}";
/// `…/collections/{collection}`.
const C: &str = "/api/v1/projects/{project}/environments/{environment}/collections/{collection}";
/// `…/entries/{entry}`.
const E: &str = "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}";

/// Every route the server has. The document describes all of them.
pub(crate) static TABLE: &[Route] = &[
    // Meta: health, version, and this document's own route is emitted by `document()`.
    route(
        ("", "/livez"),
        &[op(
            Verb::Get,
            |f| on(f, health::livez),
            OpDoc {
                id: "liveness",
                summary: "The process responds",
                tag: "meta",
                security: Security::Public,
                ok: ok(200, Shape::It("Liveness"), "`Cache-Control: no-store`"),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/readyz"),
        &[op(
            Verb::Get,
            |f| on(f, health::readyz),
            OpDoc {
                id: "readiness",
                summary: "Whether this role can safely serve now",
                tag: "meta",
                security: Security::Public,
                ok: ok(
                    200,
                    Shape::It("ReadinessReport"),
                    "`ready` or `degraded`; `Cache-Control: no-store`",
                ),
                extra: &[refusal(
                    503,
                    "",
                    "`starting`, `not_ready` or `draining` — the same report, not an error envelope",
                )],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/version"),
        &[op(
            Verb::Get,
            |f| on(f, version::get),
            OpDoc {
                id: "serverVersion",
                summary: "The release this server is and the API versions it serves",
                tag: "meta",
                security: Security::Public,
                ok: ok(
                    200,
                    Shape::It("VersionReport"),
                    "fields may be added, never removed or renamed; `Cache-Control: no-store`",
                ),
                ..MIN
            },
        )],
    ),
    // Sign-in: OIDC browser flow, email and password, sessions, the device flow, `/me`.
    route(
        ("", "/auth/providers"),
        &[op(
            Verb::Get,
            |f| on(f, auth::providers),
            OpDoc {
                id: "authProviders",
                summary: "What sign-in methods this server offers (whether an external provider is configured)",
                tag: "auth",
                security: Security::Public,
                ok: ok(
                    200,
                    Shape::Data("AuthProviders"),
                    "never cached: it is configuration",
                ),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/login"),
        &[op(
            Verb::Get,
            |f| on(f, auth::login),
            OpDoc {
                id: "login",
                summary: "Start OpenID Connect sign-in: redirect to the provider",
                tag: "auth",
                security: Security::Public,
                query: &[
                    query(
                        "return_to",
                        Kind::Str,
                        "where to send the browser afterwards",
                    ),
                    query(
                        "start",
                        Kind::Str,
                        "`create` opens the provider's sign-up form when it offers one",
                    ),
                ],
                ok: Res {
                    status: 302,
                    shape: Shape::Empty,
                    headers: &["Location", "Set-Cookie"],
                    media: "",
                    note: "redirect to the identity provider",
                },
                extra: &[refusal(
                    503,
                    "unavailable",
                    "sign-in is not configured on this server",
                )],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/callback"),
        &[op(
            Verb::Get,
            |f| on(f, auth::callback),
            OpDoc {
                id: "callback",
                summary: "The provider's redirect back: exchanges the code, sets the session cookies",
                tag: "auth",
                security: Security::Public,
                query: &[
                    query("code", Kind::Str, "the authorization code"),
                    query("state", Kind::Str, "the state this flow started with"),
                    query("error", Kind::Str, "the provider's refusal"),
                ],
                ok: Res {
                    status: 302,
                    shape: Shape::Empty,
                    headers: &["Location", "Set-Cookie"],
                    media: "",
                    note: "redirect back into the Studio",
                },
                extra: &[refusal(
                    400,
                    "sign_in_failed",
                    "sign-in failed, was cancelled, or its state expired or was already used",
                )],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/logout"),
        &[op(
            Verb::Post,
            |f| on(f, auth::logout),
            OpDoc {
                id: "logout",
                summary: "End the session or revoke the bearer credential",
                tag: "auth",
                ok: ok(
                    200,
                    Shape::Data("SignedOut"),
                    "for a browser session; a bearer credential answers 204",
                ),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/signup"),
        &[op(
            Verb::Post,
            |f| on(f, auth::sign_up),
            OpDoc {
                id: "signUp",
                summary: "Make an email-and-password account and start its session",
                tag: "auth",
                security: Security::Public,
                body: Some(json("SignUp")),
                ok: Res {
                    status: 201,
                    shape: Shape::Empty,
                    headers: &["Set-Cookie"],
                    media: "",
                    note: "the session's `Set-Cookie` pair",
                },
                extra: &[
                    refusal(409, "email_taken", "an account with this email exists"),
                    refusal(422, "invalid_email, weak_password", "the fields refused"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/signin"),
        &[op(
            Verb::Post,
            |f| on(f, auth::sign_in),
            OpDoc {
                id: "signIn",
                summary: "Sign in with email and password",
                tag: "auth",
                security: Security::Public,
                body: Some(json("PasswordSignIn")),
                ok: Res {
                    status: 204,
                    shape: Shape::Empty,
                    headers: &["Set-Cookie"],
                    media: "",
                    note: "the session's `Set-Cookie` pair",
                },
                extra: &[
                    refusal(
                        400,
                        "sign_in_failed",
                        "a wrong password and a missing account alike",
                    ),
                    refusal(409, "too_many_attempts", "the account is paused"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/device"),
        &[op(
            Verb::Post,
            |f| on(f, device::start),
            OpDoc {
                id: "deviceStart",
                summary: "Start device sign-in (`colophon login`)",
                tag: "auth",
                security: Security::Public,
                body: Some(json("DeviceStart")),
                ok: created("DeviceStarted"),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/device/approve"),
        &[op(
            Verb::Post,
            |f| on(f, device::approve),
            OpDoc {
                id: "deviceApprove",
                summary: "Approve a device sign-in as the signed-in person",
                tag: "auth",
                security: Security::Session,
                body: Some(json("DeviceApprove")),
                ok: ok(204, Shape::Empty, "the device may now poll for its token"),
                extra: &[
                    NOT_FOUND,
                    refusal(403, "browser_session_required", "not a browser session"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/device/token"),
        &[op(
            Verb::Post,
            |f| on(f, device::token),
            OpDoc {
                id: "deviceToken",
                summary: "Poll for the device flow's access token",
                tag: "auth",
                security: Security::Public,
                body: Some(json("DevicePoll")),
                ok: ok(
                    200,
                    Shape::Data("DevicePolled"),
                    "`pending` until a browser approves, `approved` with the token",
                ),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/auth/device/{code}"),
        &[op(
            Verb::Get,
            |f| on(f, device::show),
            OpDoc {
                id: "deviceStatus",
                summary: "What is asking to sign in, for the approval page",
                tag: "auth",
                ok: data("DevicePending"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/me"),
        &[op(
            Verb::Get,
            |f| on(f, auth::me),
            OpDoc {
                id: "me",
                summary: "Who the caller is, and every project they belong to",
                tag: "auth",
                ok: data("Me"),
                ..MIN
            },
        )],
    ),
    route(
        ("", "/api/v1/setup"),
        &[op(
            Verb::Post,
            |f| on(f, setup::claim),
            OpDoc {
                id: "claimSetup",
                summary: "Claim the first administrator with the one-time setup code",
                tag: "setup",
                body: Some(json("SetupCode")),
                ok: data("SetUp"),
                extra: &[
                    refusal(422, "setup_code_invalid", "wrong, used or expired, alike"),
                    refusal(409, "already_set_up", "an administrator exists already"),
                    refusal(429, "rate_limited", "too many attempts; `Retry-After`"),
                ],
                ..MIN
            },
        )],
    ),
    // Projects, membership, roles, tokens, the audit log.
    route(
        ("", "/api/v1/projects"),
        &[
            op(
                Verb::Get,
                |f| on(f, projects::list),
                OpDoc {
                    id: "listProjects",
                    summary: "The projects the caller belongs to, by name",
                    tag: "projects",
                    query: &[
                        LIMIT,
                        CURSOR,
                        query("q", Kind::Str, "matches name or slug, any case"),
                    ],
                    ok: page("ProjectView"),
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, projects::create),
                OpDoc {
                    id: "createProject",
                    summary: "Create a project (administrators only)",
                    tag: "projects",
                    body: Some(json("NewProject")),
                    ok: created("ProjectView"),
                    extra: &[
                        refusal(
                            403,
                            "administrators_only, not_set_up",
                            "only an administrator creates projects",
                        ),
                        refusal(409, "project_exists", "a project with this slug exists"),
                        refusal(422, "invalid_slug, invalid_name", "the fields refused"),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        ("", "/api/v1/projects/{project}"),
        &[op(
            Verb::Get,
            |f| on(f, projects::get),
            OpDoc {
                id: "getProject",
                summary: "One project, with what the caller's role allows in it",
                tag: "projects",
                ok: data("ProjectView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (PR, "/members"),
        &[op(
            Verb::Get,
            |f| on(f, members::list),
            OpDoc {
                id: "listMembers",
                summary: "The project's members",
                tag: "members",
                ok: page("MemberView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (PR, "/members/{identity}"),
        &[
            op(
                Verb::Put,
                |f| on(f, members::change),
                OpDoc {
                    id: "changeMemberRole",
                    summary: "Change a member's role",
                    tag: "members",
                    body: Some(json("MemberRole")),
                    ok: ok(204, Shape::Empty, "the role changed"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(409, "last_owner", "the last owner cannot be demoted"),
                        refusal(422, "invalid_role", "no such role key"),
                    ],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, members::remove),
                OpDoc {
                    id: "removeMember",
                    summary: "Remove a member; anyone may leave",
                    tag: "members",
                    ok: ok(204, Shape::Empty, "removed"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(409, "last_owner", "the last owner cannot leave"),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/invitations"),
        &[
            op(
                Verb::Get,
                |f| on(f, members::invitations),
                OpDoc {
                    id: "listInvitations",
                    summary: "The project's pending invitations",
                    tag: "members",
                    ok: page("InvitationView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, members::invite),
                OpDoc {
                    id: "createInvitation",
                    summary: "Create a single-use invitation link",
                    tag: "members",
                    body: Some(json("NewInvitation")),
                    ok: created("InvitationView"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(409, "already_member", "they are already in the project"),
                        refusal(422, "invalid_role, invalid_label", "the fields refused"),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/invitations/{invitation}"),
        &[op(
            Verb::Delete,
            |f| on(f, members::revoke),
            OpDoc {
                id: "revokeInvitation",
                summary: "Revoke a pending invitation",
                tag: "members",
                ok: ok(204, Shape::Empty, "revoked"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/api/v1/invitations/lookup"),
        &[op(
            Verb::Post,
            |f| on(f, members::lookup),
            OpDoc {
                id: "lookupInvitation",
                summary: "What an invitation link offers, before accepting",
                tag: "members",
                body: Some(json("InvitationToken")),
                ok: data("OfferView"),
                extra: &[refusal(
                    404,
                    "not_found",
                    "no such invitation — used, revoked or invented",
                )],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/api/v1/invitations/accept"),
        &[op(
            Verb::Post,
            |f| on(f, members::accept),
            OpDoc {
                id: "acceptInvitation",
                summary: "Join the project an invitation names",
                tag: "members",
                body: Some(json("InvitationToken")),
                ok: data("JoinedView"),
                extra: &[
                    refusal(
                        404,
                        "not_found",
                        "no such invitation — used, revoked or invented",
                    ),
                    refusal(409, "already_member", "already in the project"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (PR, "/roles"),
        &[
            op(
                Verb::Get,
                |f| on(f, roles::list),
                OpDoc {
                    id: "listRoles",
                    summary: "The project's roles, built-in and custom",
                    tag: "roles",
                    ok: page("RoleView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, roles::create),
                OpDoc {
                    id: "createRole",
                    summary: "Create a custom role",
                    tag: "roles",
                    body: Some(json("NewRole")),
                    ok: created("RoleView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(409, "role_exists", "a role with this key exists"),
                        refusal(
                            422,
                            "invalid_key, invalid_name, cannot_grant and friends",
                            "the fields refused, or a permission the role can't offer",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/roles/{role}"),
        &[
            op(
                Verb::Get,
                |f| on(f, roles::get),
                OpDoc {
                    id: "getRole",
                    summary: "One role, `ETag`'d as `\"v<version>\"`",
                    tag: "roles",
                    ok: etagd(data("RoleView")),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Put,
                |f| on(f, roles::update),
                OpDoc {
                    id: "updateRole",
                    summary: "Replace a role's name, description and permissions",
                    tag: "roles",
                    headers: &[IF_MATCH],
                    body: Some(json("RoleChange")),
                    ok: etagd(data("RoleView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(428, "precondition_required", "missing `If-Match`"),
                        refusal(400, "bad_request", "a malformed `If-Match`"),
                        refusal(409, "stale_role", "the role moved; the `ETag` names it now"),
                        refusal(409, "owner_role_fixed", "the owner role cannot change"),
                        refusal(422, "invalid_*", "the fields refused"),
                    ],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, roles::delete),
                OpDoc {
                    id: "deleteRole",
                    summary: "Delete a custom role",
                    tag: "roles",
                    headers: &[IF_MATCH],
                    ok: ok(204, Shape::Empty, "deleted"),
                    extra: &[
                        NOT_FOUND,
                        refusal(428, "precondition_required", "missing `If-Match`"),
                        refusal(409, "stale_role", "the role moved; the `ETag` names it now"),
                        refusal(
                            409,
                            "role_in_use, owner_role_fixed, built_in_role",
                            "members hold it, invitations offer it, or it is built in",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/tokens"),
        &[
            op(
                Verb::Get,
                |f| on(f, tokens::list),
                OpDoc {
                    id: "listTokens",
                    summary: "The project's API tokens",
                    tag: "tokens",
                    ok: page("TokenView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, tokens::create),
                OpDoc {
                    id: "createToken",
                    summary: "Create a project API token; `secret` is shown only here",
                    tag: "tokens",
                    body: Some(json("NewToken")),
                    ok: created("TokenView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            403,
                            "missing_permission, cannot_grant, not_for_tokens",
                            "`tokens.manage` needed, or a permission the creator lacks",
                        ),
                        refusal(
                            422,
                            "invalid_expiry, permissions_required, too_many_tokens and friends",
                            "the fields refused",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/tokens/{token}"),
        &[
            op(
                Verb::Get,
                |f| on(f, tokens::get),
                OpDoc {
                    id: "getToken",
                    summary: "One project API token, its limits and its usage",
                    tag: "tokens",
                    ok: data("TokenView"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Patch,
                |f| on(f, tokens::update),
                OpDoc {
                    id: "updateToken",
                    summary: "Replace (`{\"limits\": {…}}`) or remove (`null`) a token's budget limits",
                    tag: "tokens",
                    body: Some(json("PatchToken")),
                    ok: data("TokenView"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(
                            422,
                            "invalid_limit, limits_empty, nothing_to_change",
                            "the fields refused",
                        ),
                    ],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, tokens::revoke),
                OpDoc {
                    id: "revokeToken",
                    summary: "Revoke a project API token — the exact kill switch",
                    tag: "tokens",
                    ok: data("TokenView"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/audit"),
        &[op(
            Verb::Get,
            |f| on(f, audit::list),
            OpDoc {
                id: "listAuditEvents",
                summary: "The project's audit log, newest first",
                tag: "audit",
                query: &[
                    LIMIT,
                    CURSOR,
                    query(
                        "action",
                        Kind::Str,
                        "keep one action, such as `entry.deleted`",
                    ),
                    query("actor", Kind::Uuid, "keep one actor's events"),
                ],
                ok: page("EventView"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/permissions"),
        &[op(
            Verb::Get,
            |f| on(f, roles::own),
            OpDoc {
                id: "ownPermissions",
                summary: "The caller's own permissions in this environment",
                tag: "permissions",
                security: Security::Any,
                ok: data("OwnView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/events"),
        &[op(
            Verb::Get,
            |f| on(f, crate::events::events),
            OpDoc {
                id: "streamEvents",
                summary: "Live change hints as server-sent events",
                tag: "events",
                security: Security::Any,
                ok: Res {
                    status: 200,
                    shape: Shape::It("EventStream"),
                    headers: &[],
                    media: "text/event-stream",
                    note: "`hello`, then `change` frames, `resync` on a missed hint — never a replay",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        429,
                        "rate_limited",
                        "past the per-credential stream limit; `Retry-After`",
                    ),
                    refusal(
                        503,
                        "unavailable",
                        "past the per-project or per-server stream limit",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/mcp"),
        &[
            op(
                Verb::Post,
                |f| on(f, mcp::post),
                OpDoc {
                    id: "mcpPost",
                    summary: "One JSON-RPC message on the remote-MCP session (D-121)",
                    tag: "mcp",
                    security: Security::ApiToken,
                    headers: &[
                        header_maybe(
                            "mcp-session-id",
                            Kind::Uuid,
                            "the session `initialize` minted; required on every message after it",
                        ),
                        header_maybe(
                            "mcp-protocol-version",
                            Kind::Str,
                            "`2025-11-25`; required once a session exists",
                        ),
                        header_maybe(
                            "origin",
                            Kind::Str,
                            "when present it must be an origin this server trusts, exactly — the transport's DNS-rebinding check",
                        ),
                    ],
                    body: Some(json("McpMessage")),
                    ok: Res {
                        status: 200,
                        shape: Shape::It("McpResponse"),
                        headers: &["Mcp-Session-Id"],
                        media: "",
                        note: "the request's own JSON-RPC response; `Mcp-Session-Id` rides on the `initialize` answer only",
                    },
                    also: Some(ok(
                        202,
                        Shape::Empty,
                        "a notification or a stray response — accepted, nothing to answer",
                    )),
                    extra: &[
                        refusal(
                            400,
                            "bad_request",
                            "not acceptable JSON-RPC, or `MCP-Protocol-Version`/`Accept` failed the transport's checks",
                        ),
                        refusal(
                            403,
                            "api_token_required",
                            "a person's credential — a browser session or a personal access token; agents sign in as project API tokens",
                        ),
                        refusal(
                            403,
                            "origin_not_allowed",
                            "an `Origin` this server does not trust",
                        ),
                        refusal(
                            404,
                            "not_found",
                            "no such session — or one another token holds, which looks the same",
                        ),
                        refusal(
                            429,
                            "rate_limited",
                            "the token's session cap is full; `Retry-After`",
                        ),
                        refusal(
                            503,
                            "unavailable",
                            "the project's or the server's session cap is full",
                        ),
                    ],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, mcp::delete),
                OpDoc {
                    id: "mcpDelete",
                    summary: "End the remote-MCP session early",
                    tag: "mcp",
                    security: Security::ApiToken,
                    headers: &[
                        header("mcp-session-id", Kind::Uuid, "the session to end"),
                        header("mcp-protocol-version", Kind::Str, "`2025-11-25`"),
                    ],
                    ok: ok(
                        200,
                        Shape::Empty,
                        "the session ended; its in-flight requests answer `request cancelled`",
                    ),
                    extra: &[
                        refusal(
                            400,
                            "bad_request",
                            "the `Mcp-Session-Id` or `MCP-Protocol-Version` header is missing or wrong",
                        ),
                        refusal(
                            403,
                            "api_token_required",
                            "a person's credential — a browser session or a personal access token; agents sign in as project API tokens",
                        ),
                        refusal(
                            403,
                            "origin_not_allowed",
                            "an `Origin` this server does not trust",
                        ),
                        refusal(
                            404,
                            "not_found",
                            "no such session — or one another token holds, which looks the same",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        ("", "/api/v1/rich-text/validate"),
        &[op(
            Verb::Post,
            |f| on(f, rich_text::validate),
            OpDoc {
                id: "validateRichText",
                summary: "The server's verdict on a `colophon/rich-text` v1 document",
                tag: "rich-text",
                security: Security::Any,
                body: Some(json("RichTextDocument")),
                ok: ok(
                    200,
                    Shape::It("RichTextVerdict"),
                    "the canonical document and its measurements; nothing is stored",
                ),
                extra: &[refusal(
                    422,
                    "validation_failed",
                    "the document was refused; `details` name the rejection code and JSON Pointer",
                )],
                ..MIN
            },
        )],
    ),
    // Schema: content types, shared components, migration plans.
    route(
        (P, "/content-types"),
        &[
            op(
                Verb::Get,
                |f| on(f, content_types::list),
                OpDoc {
                    id: "listContentTypes",
                    summary: "The environment's content types, each at its current version",
                    tag: "schema",
                    security: Security::Any,
                    ok: page("TypeVersionView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, content_types::create),
                OpDoc {
                    id: "createContentType",
                    summary: "Create a collection or single type",
                    tag: "schema",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewType")),
                    ok: created("TypeVersionView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(409, "content_type_exists", "a type with this slug exists"),
                        refusal(422, "invalid_*", "the definition refused"),
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/content-types/{slug}"),
        &[
            op(
                Verb::Get,
                |f| on(f, content_types::get),
                OpDoc {
                    id: "getContentType",
                    summary: "One content type at its current version, `ETag: \"v<version>\"`",
                    tag: "schema",
                    security: Security::Any,
                    ok: etagd(data("TypeVersionView")),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Put,
                |f| on(f, content_types::update),
                OpDoc {
                    id: "updateContentType",
                    summary: "Write a new version, checked against every stored revision",
                    tag: "schema",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("TypeDefinition")),
                    ok: etagd(data("TypeVersionView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "needs_migration",
                            "stored content wouldn't fit; `details.entries` names the first 20 in the way",
                        ),
                        refusal(
                            409,
                            "stale_version",
                            "the type moved; the `ETag` names it now",
                        ),
                        refusal(
                            409,
                            "migration_in_progress",
                            "a migration plan is running on the collection",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, content_types::delete),
                OpDoc {
                    id: "deleteContentType",
                    summary: "Delete a collection and every version of its definition",
                    tag: "schema",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    ok: data("RemovedView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            409,
                            "stale_version",
                            "the type moved; the `ETag` names it now",
                        ),
                        refusal(
                            409,
                            "still_referenced, collection_not_empty, collection_has_history, collection_has_plans, migration_in_progress",
                            "something still depends on the collection",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/components"),
        &[
            op(
                Verb::Get,
                |f| on(f, components::list),
                OpDoc {
                    id: "listComponents",
                    summary: "The environment's shared components",
                    tag: "schema",
                    security: Security::Any,
                    ok: page("ComponentView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, components::create),
                OpDoc {
                    id: "createComponent",
                    summary: "Create a shared component",
                    tag: "schema",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewComponent")),
                    ok: created("ComponentView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(409, "component_exists", "a component with this slug exists"),
                        refusal(422, "invalid_*", "the definition refused"),
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/components/{slug}"),
        &[
            op(
                Verb::Get,
                |f| on(f, components::get),
                OpDoc {
                    id: "getComponent",
                    summary: "One shared component, `ETag: \"v<version>\"`",
                    tag: "schema",
                    security: Security::Any,
                    ok: etagd(data("ComponentView")),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Put,
                |f| on(f, components::update),
                OpDoc {
                    id: "updateComponent",
                    summary: "Rewrite a component; every type that uses it gets a new version",
                    tag: "schema",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("ComponentDefinition")),
                    ok: etagd(data("ComponentView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "needs_migration",
                            "a using type's stored content wouldn't fit; `details.content_type` names it",
                        ),
                        refusal(
                            409,
                            "stale_version",
                            "the component moved; the `ETag` names it now",
                        ),
                        refusal(
                            409,
                            "migration_in_progress",
                            "a migration runs on a collection that uses it",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/migrations"),
        &[
            op(
                Verb::Get,
                |f| on(f, migrations::list),
                OpDoc {
                    id: "listMigrations",
                    summary: "The environment's migration plans, newest first",
                    tag: "migrations",
                    security: Security::Any,
                    query: &[
                        query("collection", Kind::Str, "keep one collection's plans"),
                        query(
                            "state",
                            Kind::Str,
                            "`proposed`, `approved`, `running`, `done`, `blocked`, `superseded`, `cancelled`",
                        ),
                    ],
                    ok: page("MigrationSummaryView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, migrations::create),
                OpDoc {
                    id: "createMigration",
                    summary: "Plan a schema migration: what the target definition would change",
                    tag: "migrations",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewPlan")),
                    ok: Res {
                        status: 201,
                        shape: Shape::Data("PlanView"),
                        headers: &["ETag"],
                        media: "",
                        note: "`ETag: \"sha256:<plan hash>\"` — what `If-Match` binds",
                    },
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "no_migration_needed",
                            "a plain content-type update could apply",
                        ),
                        refusal(
                            409,
                            "migration_in_progress",
                            "a migration is running on the collection",
                        ),
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/migrations/{migration}"),
        &[op(
            Verb::Get,
            |f| on(f, migrations::get),
            OpDoc {
                id: "getMigration",
                summary: "One plan, `ETag`'d with its hash",
                tag: "migrations",
                security: Security::Any,
                ok: etagd(data("PlanView")),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/migrations/{migration}/items"),
        &[op(
            Verb::Get,
            |f| on(f, migrations::items),
            OpDoc {
                id: "listMigrationItems",
                summary: "A plan's items in entry order",
                tag: "migrations",
                security: Security::Any,
                query: &[
                    query("outcome", Kind::Str, "`ok` or `needs_input`"),
                    query("after", Kind::Cursor, "continue past this item"),
                    LIMIT,
                ],
                ok: page("MigrationItemView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/migrations/{migration}/items/{entry}"),
        &[op(
            Verb::Put,
            |f| on(f, migrations::resolve),
            OpDoc {
                id: "resolveMigrationItem",
                summary: "Give a `needs_input` item its resolution document",
                tag: "migrations",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                body: Some(json("NewResolution")),
                ok: etagd(data("ResolvedView")),
                extra: &[
                    NOT_FOUND,
                    refusal(409, "plan_moved", "the plan moved; the `ETag` names it now"),
                    refusal(
                        409,
                        "item_not_needs_input, plan_not_open",
                        "the item is resolved, or the plan left its open states",
                    ),
                    refusal(
                        422,
                        "invalid_*",
                        "the resolution wouldn't validate against the target",
                    ),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/migrations/{migration}/approve"),
        &[op(
            Verb::Post,
            |f| on(f, migrations::approve),
            OpDoc {
                id: "approveMigration",
                summary: "Approve a proposed plan on its hash",
                tag: "migrations",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                ok: etagd(data("PlanView")),
                extra: &[
                    NOT_FOUND,
                    refusal(409, "plan_moved", "the plan moved; the `ETag` names it now"),
                    refusal(
                        409,
                        "plan_not_proposed",
                        "only a proposed plan can be approved",
                    ),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/migrations/{migration}/apply"),
        &[op(
            Verb::Post,
            |f| on(f, migrations::apply),
            OpDoc {
                id: "applyMigration",
                summary: "Start an approved plan; a worker drives its items",
                tag: "migrations",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                ok: Res {
                    status: 202,
                    shape: Shape::Data("PlanRunView"),
                    headers: &["ETag"],
                    media: "",
                    note: "accepted — `state: \"running\"`, never \"done\"",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(409, "plan_moved", "the plan moved; the `ETag` names it now"),
                    refusal(
                        409,
                        "plan_not_approved",
                        "only an approved plan can be applied",
                    ),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/migrations/{migration}/cancel"),
        &[op(
            Verb::Post,
            |f| on(f, migrations::cancel),
            OpDoc {
                id: "cancelMigration",
                summary: "Cancel a plan before it runs",
                tag: "migrations",
                security: Security::Any,
                headers: &[IDEMPOTENCY],
                ok: etagd(data("PlanView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "cannot_cancel_running, plan_finished",
                        "running plans can't be cancelled; finished ones are done",
                    ),
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    // Bounded bulk work on drafts: chunks of named items, per-item outcomes (D-116).
    route(
        (P, "/bulk"),
        &[
            op(
                Verb::Get,
                |f| on(f, bulk::list),
                OpDoc {
                    id: "listBulkJobs",
                    summary: "The environment's bulk jobs, newest first",
                    tag: "bulk",
                    security: Security::Any,
                    query: &[
                        query(
                            "state",
                            Kind::Str,
                            "`accepting`, `running`, `done`, `blocked`, `cancelled`",
                        ),
                        CURSOR,
                    ],
                    ok: page("BulkJobView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, bulk::create),
                OpDoc {
                    id: "submitBulkJob",
                    summary: "Submit a manifest of exact entries and base revisions (a chunk is at most 100 items and 1 MiB)",
                    tag: "bulk",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewBulkJob")),
                    ok: Res {
                        status: 202,
                        shape: Shape::Data("BulkJobView"),
                        headers: &["ETag"],
                        media: "",
                        note: "`ETag: \"sha256:<manifest hash>\"` — what `If-Match` binds",
                    },
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "invalid_*, chunk_too_large, manifest_empty",
                            "a manifest item wouldn't validate",
                        ),
                        refusal(
                            403,
                            "missing_permission, budget_exceeded",
                            "a permission the manifest asks for isn't the caller's, or a budget refuses",
                        ),
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/bulk/{job}"),
        &[op(
            Verb::Get,
            |f| on(f, bulk::get),
            OpDoc {
                id: "getBulkJob",
                summary: "One bulk job, `ETag`'d with its manifest hash",
                tag: "bulk",
                security: Security::Any,
                ok: etagd(data("BulkJobView")),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/bulk/{job}/items"),
        &[
            op(
                Verb::Get,
                |f| on(f, bulk::items),
                OpDoc {
                    id: "listBulkItems",
                    summary: "A job's items in key order — the resumable cursor",
                    tag: "bulk",
                    security: Security::Any,
                    query: &[
                        query(
                            "outcome",
                            Kind::Str,
                            "`pending`, `applied`, `conflict`, `refused`, `skipped`",
                        ),
                        query("after", Kind::Cursor, "continue past this item"),
                        LIMIT,
                    ],
                    ok: page("BulkItemView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, bulk::append),
                OpDoc {
                    id: "appendBulkItems",
                    summary: "Append the next chunk while the job is `accepting`",
                    tag: "bulk",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("AppendBulkItems")),
                    ok: Res {
                        status: 202,
                        shape: Shape::Data("BulkAppendView"),
                        headers: &["ETag"],
                        media: "",
                        note: "the job after the chunk; `duplicates` names keys that landed already",
                    },
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            409,
                            "job_moved",
                            "the manifest changed; the `ETag` names it now",
                        ),
                        refusal(409, "job_not_accepting", "the manifest is closed"),
                        refusal(
                            422,
                            "invalid_*, chunk_too_large, manifest_too_large",
                            "the chunk wouldn't validate",
                        ),
                        refusal(
                            403,
                            "missing_permission, budget_exceeded",
                            "a permission the chunk asks for isn't the caller's, or a budget refuses",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/bulk/{job}/cancel"),
        &[op(
            Verb::Post,
            |f| on(f, bulk::cancel),
            OpDoc {
                id: "cancelBulkJob",
                summary: "Cancel an open job; pending items skip, reservations release",
                tag: "bulk",
                security: Security::Any,
                headers: &[IDEMPOTENCY],
                ok: etagd(data("BulkJobView")),
                extra: &[
                    NOT_FOUND,
                    refusal(409, "job_finished", "the job is already finished"),
                    FORBIDDEN,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    // Entries: drafts, history, locales, the query language, transfer.
    route(
        (C, "/entries"),
        &[
            op(
                Verb::Get,
                |f| on(f, entries::list),
                OpDoc {
                    id: "listEntries",
                    summary: "A collection's drafts, cursored",
                    tag: "entries",
                    security: Security::Any,
                    query: &[
                        LIMIT,
                        CURSOR,
                        query("q", Kind::Str, "title search"),
                        query(
                            "created_by",
                            Kind::Str,
                            "`me` or a creator's id, within what the role may list",
                        ),
                        query("locale", Kind::Str, "keep one locale's entries"),
                    ],
                    ok: page("EntrySummaryView"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, entries::create),
                OpDoc {
                    id: "createEntry",
                    summary: "Create a draft, validated against the collection's type",
                    tag: "entries",
                    security: Security::Any,
                    query: &[
                        query(
                            "locale",
                            Kind::Str,
                            "the entry's locale; absent or empty leaves it unlocalized",
                        ),
                        VISIBILITY,
                    ],
                    headers: &[IDEMPOTENCY],
                    body: Some(json("EntryDocument")),
                    ok: etagd(created("DraftView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            403,
                            "missing_permission, fields_not_writable, budget_exceeded",
                            "the role, or a token's `documents_per_day` budget, refused it",
                        ),
                        refusal(
                            409,
                            "slug_taken, value_taken",
                            "the slug or a unique value is another entry's; `details.entries` names it",
                        ),
                        refusal(
                            422,
                            "validation_failed, reference_not_found, single_exists and friends",
                            "the document refused, with `details.reason` and a JSON Pointer",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    also: None,
                },
            ),
        ],
    ),
    route(
        (C, "/entries/query"),
        &[op(
            Verb::Post,
            |f| on(f, entries::query),
            OpDoc {
                id: "queryEntries",
                summary: "Run the bounded query language over current drafts",
                tag: "entries",
                security: Security::Any,
                query: &[CURSOR],
                body: Some(json("QueryDocument")),
                ok: ok(
                    200,
                    Shape::It("QueryPage"),
                    "`applied` echoes the canonical query and its hash",
                ),
                extra: &[
                    NOT_FOUND,
                    FORBIDDEN,
                    refusal(
                        422,
                        "field_not_readable",
                        "a filter or sort names a field the role can't read on every row",
                    ),
                    refusal(
                        422,
                        "invalid_cursor, foreign_cursor, too_large, position_mismatch",
                        "the cursor is not this query's",
                    ),
                    refusal(422, "invalid_*", "the query document refused"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (C, "/graphql"),
        &[op(
            Verb::Post,
            |f| on(f, graphql::graphql),
            OpDoc {
                id: "queryEntriesGraphQL",
                summary: "GraphQL over the same read `entries/query` runs (read-only)",
                tag: "entries",
                security: Security::Any,
                body: Some(json("GraphQLRequest")),
                ok: ok(
                    200,
                    Shape::It("GraphQLResponse"),
                    "the document's `data`, with `errors[].extensions` carrying the query language's `code`/`pointer` on a refusal",
                ),
                extra: &[
                    NOT_FOUND,
                    FORBIDDEN,
                    refusal(
                        422,
                        "too_large, wrong_type",
                        "the `query` string over 8 KiB, or `variables` not an object",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (C, "/export"),
        &[op(
            Verb::Get,
            |f| on(f, entries::export),
            OpDoc {
                id: "exportEntries",
                summary: "Every draft's canonical content as JSON Lines",
                tag: "entries",
                security: Security::Any,
                ok: Res {
                    status: 200,
                    shape: Shape::It("JsonLines"),
                    headers: &["Content-Disposition"],
                    media: "application/jsonlines",
                    note: "one entry per line — what `import` takes back",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        403,
                        "fields_not_readable",
                        "an export holds every field, so it needs a role that reads them all",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (C, "/import"),
        &[op(
            Verb::Post,
            |f| on(f, entries::import),
            OpDoc {
                id: "importEntries",
                summary: "Import entries: native JSONL, or Strapi and Sanity documents",
                tag: "entries",
                security: Security::Any,
                query: &[
                    query(
                        "dry_run",
                        Kind::Bool,
                        "set `true` to run every item and roll back — the 200 plan names each `would_create`, `would_update`, `skipped` or `invalid` item; a plan is a prediction, not a reservation, and needs no `Idempotency-Key`",
                    ),
                    query(
                        "type",
                        Kind::Str,
                        "the Sanity `_type` to take from a mixed file; documents of other types are `skipped`, reported",
                    ),
                ],
                headers: &[IDEMPOTENCY_MAYBE],
                body: Some(Body {
                    schema: "ImportBody",
                    media: &["application/jsonlines", "application/json"],
                    note: "one object per line, one array, or a Strapi `{\"data\": […], \"meta\": {…}}`; lands in one transaction or not at all",
                }),
                ok: created("ImportedView"),
                also: Some(Res {
                    status: 200,
                    shape: Shape::Data("ImportPlanView"),
                    headers: &[],
                    media: "",
                    note: "the dry run's per-item plan; nothing was written",
                }),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        403,
                        "missing_permission, fields_not_writable, budget_exceeded",
                        "the role, or a token's `documents_per_day` budget, refused it",
                    ),
                    refusal(409, "single_exists", "a single type already has its entry"),
                    refusal(
                        422,
                        "too_many_items, type_mismatch, invalid_*, slug_taken, value_taken",
                        "the file refused, with the failing item's index",
                    ),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
            },
        )],
    ),
    route(
        (E, ""),
        &[
            op(
                Verb::Get,
                |f| on(f, entries::get),
                OpDoc {
                    id: "getEntry",
                    summary: "One draft: its content narrowed to the role, `referenced_by`, `ETag`",
                    tag: "entries",
                    security: Security::Any,
                    ok: etagd(data("DraftView")),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Put,
                |f| on(f, entries::save),
                OpDoc {
                    id: "saveEntry",
                    summary: "Save a draft on its expected revision",
                    tag: "entries",
                    security: Security::Any,
                    query: &[
                        query(
                            "locale",
                            Kind::Str,
                            "a tag sets the locale; an empty value clears it; absent leaves it alone",
                        ),
                        VISIBILITY,
                    ],
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("EntryDocument")),
                    ok: etagd(data("DraftView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            403,
                            "missing_permission, fields_not_writable, budget_exceeded",
                            "the role, or a token's `documents_per_day` budget, refused it",
                        ),
                        refusal(
                            409,
                            "stale_revision",
                            "the draft moved; the `ETag` names it now",
                        ),
                        refusal(
                            409,
                            "slug_taken, value_taken",
                            "the slug or a unique value is another entry's",
                        ),
                        refusal(422, "validation_failed and friends", "the document refused"),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    also: None,
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, entries::delete),
                OpDoc {
                    id: "deleteEntry",
                    summary: "Delete an entry, leaving a tombstone",
                    tag: "entries",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    ok: data("DeletedView"),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            409,
                            "stale_revision",
                            "the draft moved; the `ETag` names it now",
                        ),
                        refusal(
                            409,
                            "still_referenced, entry_in_changeset",
                            "something still depends on it; `details` names it",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (E, "/localizations"),
        &[op(
            Verb::Post,
            |f| on(f, entries::localize),
            OpDoc {
                id: "createLocalization",
                summary: "Create a locale sibling holding a copy of this draft",
                tag: "entries",
                security: Security::Any,
                query: &[
                    Param {
                        name: "locale",
                        kind: Kind::Str,
                        required: true,
                        note: "the sibling's locale tag, such as `fr`",
                    },
                    VISIBILITY,
                ],
                headers: &[IDEMPOTENCY],
                ok: etagd(created("DraftView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        403,
                        "budget_exceeded",
                        "a token's `documents_per_day` budget refused it",
                    ),
                    refusal(
                        409,
                        "locale_taken, slug_taken, value_taken, single_exists",
                        "the locale is already in the group, or the copy hit a uniqueness rule",
                    ),
                    refusal(422, "missing_locale, invalid_locale and friends", ""),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (E, "/revisions"),
        &[op(
            Verb::Get,
            |f| on(f, entries::history),
            OpDoc {
                id: "entryHistory",
                summary: "The entry's revisions, newest first",
                tag: "entries",
                security: Security::Any,
                ok: ok(200, Shape::List("RevisionView"), "every revision kept"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        (E, "/revisions/{revision}"),
        &[op(
            Verb::Get,
            |f| on(f, entries::revision),
            OpDoc {
                id: "getRevision",
                summary: "One revision's content, narrowed to the role",
                tag: "entries",
                security: Security::Any,
                ok: data("RevisionContentView"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    // Collaboration: comments and tasks (E1, D-102).
    route(
        (E, "/comments"),
        &[
            op(
                Verb::Get,
                |f| on(f, collaboration::comments),
                OpDoc {
                    id: "listComments",
                    summary: "An entry's comments, revision-anchored and verbatim",
                    tag: "collaboration",
                    security: Security::Any,
                    query: &[
                        LIMIT,
                        CURSOR,
                        query("revision", Kind::Uuid, "keep one revision's comments"),
                        query("state", Kind::Str, "`open` or `resolved`"),
                    ],
                    ok: page("CommentView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, collaboration::create_comment),
                OpDoc {
                    id: "createComment",
                    summary: "Anchor a comment to an immutable revision",
                    tag: "collaboration",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewComment")),
                    ok: etagd(created("CommentView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "revision_not_found, invalid_field_path, empty_body, body_too_long",
                            "the comment refused",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (E, "/comments/{comment}"),
        &[op(
            Verb::Get,
            |f| on(f, collaboration::comment),
            OpDoc {
                id: "getComment",
                summary: "One comment, `ETag`'d by its resolution state",
                tag: "collaboration",
                security: Security::Any,
                ok: etagd(data("CommentView")),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (E, "/comments/{comment}/resolve"),
        &[op(
            Verb::Post,
            |f| on(f, collaboration::resolve),
            OpDoc {
                id: "resolveComment",
                summary: "Mark a comment resolved",
                tag: "collaboration",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                ok: etagd(data("CommentView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "comment_moved",
                        "the comment moved; the `ETag` names it now",
                    ),
                    refusal(409, "already_resolved", "it is already resolved"),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (E, "/comments/{comment}/reopen"),
        &[op(
            Verb::Post,
            |f| on(f, collaboration::reopen),
            OpDoc {
                id: "reopenComment",
                summary: "Reopen a resolved comment",
                tag: "collaboration",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                ok: etagd(data("CommentView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "comment_moved",
                        "the comment moved; the `ETag` names it now",
                    ),
                    refusal(409, "already_open", "it is already open"),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/tasks"),
        &[
            op(
                Verb::Get,
                |f| on(f, collaboration::tasks),
                OpDoc {
                    id: "listTasks",
                    summary: "The environment's editorial tasks",
                    tag: "collaboration",
                    security: Security::Any,
                    query: &[
                        LIMIT,
                        CURSOR,
                        query("entry", Kind::Uuid, "keep one entry's tasks"),
                        query("collection", Kind::Str, "with `entry`, its collection"),
                        query("assignee", Kind::Str, "`me`, `none`, or an identity id"),
                        query("status", Kind::Str, "`open`, `done` or `cancelled`"),
                    ],
                    ok: page("TaskView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, collaboration::create_task),
                OpDoc {
                    id: "createTask",
                    summary: "Create a task on an entry",
                    tag: "collaboration",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewTask")),
                    ok: etagd(created("TaskView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "revision_not_found, changeset_not_found, assignee_not_member, empty_title, title_too_long",
                            "the task refused",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/tasks/{task}"),
        &[
            op(
                Verb::Get,
                |f| on(f, collaboration::task),
                OpDoc {
                    id: "getTask",
                    summary: "One task, `ETag`'d by its state",
                    tag: "collaboration",
                    security: Security::Any,
                    ok: etagd(data("TaskView")),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Patch,
                |f| on(f, collaboration::update_task),
                OpDoc {
                    id: "updateTask",
                    summary: "Change a task's title, assignee or status",
                    tag: "collaboration",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("TaskUpdate")),
                    ok: etagd(data("TaskView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(409, "task_moved", "the task moved; the `ETag` names it now"),
                        refusal(
                            422,
                            "invalid_status, nothing_to_change, assignee_not_member",
                            "the change refused",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    // Publishing: changesets, review stages, schedules, releases, receipts, destinations, jobs.
    route(
        (P, "/changesets"),
        &[
            op(
                Verb::Get,
                |f| on(f, changesets::list),
                OpDoc {
                    id: "listChangesets",
                    summary: "The environment's changesets",
                    tag: "publishing",
                    security: Security::Any,
                    ok: page("ChangesetSummaryView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, changesets::create),
                OpDoc {
                    id: "createChangeset",
                    summary: "Pin entries at their current revisions for review and release",
                    tag: "publishing",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    body: Some(json("NewChangeset")),
                    ok: etagd(created("ChangesetView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            422,
                            "unknown_entry, entries_moved_on, too_many_entries, empty_changeset and friends",
                            "the changeset refused",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/changesets/{changeset}"),
        &[op(
            Verb::Get,
            |f| on(f, changesets::get),
            OpDoc {
                id: "getChangeset",
                summary: "One changeset: items, status, reviews, stage states; `ETag`'d by its hash",
                tag: "publishing",
                security: Security::Any,
                ok: etagd(data("ChangesetView")),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/changesets/{changeset}/submit"),
        &[op(
            Verb::Post,
            |f| on(f, changesets::submit),
            OpDoc {
                id: "submitChangeset",
                summary: "Submit a changeset for review",
                tag: "publishing",
                security: Security::Any,
                headers: &[IF_MATCH],
                ok: etagd(data("ChangesetView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "stale_changeset",
                        "the changeset moved; the `ETag` names it now",
                    ),
                    refusal(409, "already_submitted, already_published", "wrong state"),
                    P428,
                    BAD_MATCH,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/changesets/{changeset}/refresh"),
        &[op(
            Verb::Post,
            |f| on(f, changesets::refresh),
            OpDoc {
                id: "refreshChangeset",
                summary: "Re-pin the changeset's items at their current revisions",
                tag: "publishing",
                security: Security::Any,
                headers: &[IF_MATCH],
                ok: etagd(data("ChangesetView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "stale_changeset",
                        "the changeset moved; the `ETag` names it now",
                    ),
                    refusal(409, "not_editable, already_published", "wrong state"),
                    P428,
                    BAD_MATCH,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/changesets/{changeset}/reviews"),
        &[op(
            Verb::Post,
            |f| on(f, changesets::review),
            OpDoc {
                id: "reviewChangeset",
                summary: "Approve, or request changes on, the changeset's current hash",
                tag: "publishing",
                security: Security::Any,
                headers: &[IF_MATCH],
                body: Some(json("NewReview")),
                ok: etagd(data("ChangesetView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        403,
                        "missing_permission, agent_self_review",
                        "the role can't review it, or a token tried to approve a token's changeset",
                    ),
                    refusal(
                        409,
                        "stale_changeset",
                        "the changeset moved; the `ETag` names it now",
                    ),
                    refusal(
                        409,
                        "not_submitted, already_published, different_reviewer",
                        "wrong state, or a stage needs another reviewer",
                    ),
                    refusal(
                        422,
                        "invalid_decision, invalid_comment, stage_required, unknown_stage",
                        "the review refused",
                    ),
                    P428,
                    BAD_MATCH,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/changesets/{changeset}/schedule"),
        &[op(
            Verb::Post,
            |f| on(f, schedules::create),
            OpDoc {
                id: "scheduleChangeset",
                summary: "Schedule the publish in an IANA time zone",
                tag: "publishing",
                // A schedule runs later as the person who made it, so project API tokens can't.
                security: Security::Person,
                headers: &[IF_MATCH],
                body: Some(json("NewSchedule")),
                ok: created("ScheduleView"),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "stale_changeset",
                        "the changeset moved; the `ETag` names it now",
                    ),
                    refusal(409, "already_scheduled", "one schedule is pending already"),
                    refusal(
                        422,
                        "ambiguous_local_time, skipped_local_time, in_the_past, too_far_ahead, after_approval_expires, unknown_time_zone, invalid_local_time, invalid_choice",
                        "the time refused; `details.candidates` offers the two instants",
                    ),
                    P428,
                    BAD_MATCH,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/changesets/{changeset}/publish"),
        &[op(
            Verb::Post,
            |f| on(f, changesets::publish),
            OpDoc {
                id: "publishChangeset",
                summary: "Commit the release on the publisher's base generation",
                tag: "publishing",
                security: Security::Any,
                headers: &[IF_MATCH, IDEMPOTENCY],
                body: Some(json("NewRelease")),
                ok: created("PublishedView"),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        403,
                        "budget_exceeded",
                        "a token's `max_concurrent_jobs` budget refused the release's dispatch",
                    ),
                    refusal(
                        409,
                        "stale_changeset",
                        "the changeset moved; the `ETag` names it now",
                    ),
                    refusal(
                        409,
                        "generation_moved",
                        "another release committed first; `Colophon-Generation` names the current one",
                    ),
                    refusal(409, "not_approved, already_published", "wrong state"),
                    refusal(422, "invalid_generation, slug_taken", ""),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/schedules"),
        &[op(
            Verb::Get,
            |f| on(f, schedules::list),
            OpDoc {
                id: "listSchedules",
                summary: "Pending schedules, then the latest finished, each with `delayed`",
                tag: "publishing",
                security: Security::Any,
                query: &[query(
                    "changeset",
                    Kind::Uuid,
                    "keep one changeset's schedules",
                )],
                ok: page("ScheduleView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/schedules/{schedule}"),
        &[op(
            Verb::Delete,
            |f| on(f, schedules::cancel),
            OpDoc {
                id: "cancelSchedule",
                summary: "Cancel a pending schedule",
                tag: "publishing",
                security: Security::Any,
                ok: data("ScheduleView"),
                extra: &[
                    NOT_FOUND,
                    refusal(409, "schedule_ran", "it already ran"),
                    FORBIDDEN,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/review-stages"),
        &[
            op(
                Verb::Get,
                |f| on(f, changesets::stages_list),
                OpDoc {
                    id: "listReviewStages",
                    summary: "The environment's ordered review stages, `ETag`'d as a list",
                    tag: "publishing",
                    security: Security::Any,
                    ok: etagd(data("StageListView")),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Put,
                |f| on(f, changesets::stages_replace),
                OpDoc {
                    id: "replaceReviewStages",
                    summary: "Replace the whole stage list in the body's order",
                    tag: "publishing",
                    security: Security::Any,
                    headers: &[IF_MATCH, IDEMPOTENCY],
                    body: Some(json("NewStages")),
                    ok: etagd(data("StageListView")),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            409,
                            "stale_stages",
                            "the list moved; the `ETag` names it now",
                        ),
                        refusal(
                            422,
                            "too_many_stages, invalid_stage_key, invalid_stage_name, duplicate_stage",
                            "the list refused, naming the offending position",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/releases"),
        &[op(
            Verb::Get,
            |f| on(f, changesets::releases),
            OpDoc {
                id: "listReleases",
                summary: "The environment's releases, newest first",
                tag: "publishing",
                security: Security::Any,
                ok: Res {
                    status: 200,
                    shape: Shape::Page("ReleaseView"),
                    headers: &["Colophon-Generation"],
                    media: "",
                    note: "the current generation is in `Colophon-Generation`",
                },
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/releases/{generation}"),
        &[op(
            Verb::Get,
            |f| on(f, receipts::get),
            OpDoc {
                id: "getReceipt",
                summary: "One release's receipt: deliveries and observations",
                tag: "publishing",
                security: Security::Any,
                ok: data("ReceiptView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/releases/{generation}/rollback"),
        &[op(
            Verb::Post,
            |f| on(f, changesets::rollback),
            OpDoc {
                id: "rollbackRelease",
                summary: "Open a changeset restoring what that release published",
                tag: "publishing",
                security: Security::Any,
                headers: &[IDEMPOTENCY],
                body: Some(json("NewChangeset")),
                ok: etagd(created("ChangesetView")),
                extra: &[
                    NOT_FOUND,
                    refusal(
                        422,
                        "invalid_field",
                        "a rollback takes no `entries` or `unpublish`",
                    ),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/destinations"),
        &[
            op(
                Verb::Get,
                |f| on(f, destinations::list),
                OpDoc {
                    id: "listDestinations",
                    summary: "The environment's delivery destinations",
                    tag: "publishing",
                    security: Security::Any,
                    ok: page("DestinationView"),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, destinations::create),
                OpDoc {
                    id: "createDestination",
                    summary: "Add a webhook destination (https, public addresses)",
                    tag: "publishing",
                    security: Security::Any,
                    body: Some(json("NewDestination")),
                    ok: created("DestinationView"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(
                            422,
                            "invalid_kind, invalid_name, invalid_url, private_address and friends",
                            "the destination refused",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/destinations/{destination}"),
        &[op(
            Verb::Put,
            |f| on(f, destinations::set_enabled),
            OpDoc {
                id: "setDestinationEnabled",
                summary: "Enable or pause a destination",
                tag: "publishing",
                security: Security::Any,
                body: Some(json("DestinationEnabled")),
                ok: ok(204, Shape::Empty, "the flag changed"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/jobs"),
        &[op(
            Verb::Get,
            |f| on(f, destinations::jobs),
            OpDoc {
                id: "listJobs",
                summary: "Delivery jobs, newest first",
                tag: "publishing",
                security: Security::Any,
                query: &[query("generation", Kind::Int, "keep one release's jobs")],
                ok: page("JobView"),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    // Assets.
    route(
        (P, "/assets"),
        &[
            op(
                Verb::Get,
                |f| on(f, assets::list),
                OpDoc {
                    id: "listAssets",
                    summary: "The environment's assets, newest first",
                    tag: "assets",
                    security: Security::Any,
                    ok: ok(200, Shape::List("AssetView"), ""),
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, assets::upload),
                OpDoc {
                    id: "uploadAsset",
                    summary: "Upload an image; the verify job decides what it becomes",
                    tag: "assets",
                    security: Security::Any,
                    headers: &[
                        IDEMPOTENCY,
                        header("x-colophon-file-name", Kind::Str, "the file's name"),
                        header(
                            "content-type",
                            Kind::Str,
                            "the claimed image type, verified by the worker",
                        ),
                    ],
                    body: Some(Body {
                        schema: "UploadBody",
                        media: &["application/octet-stream"],
                        note: "the file's bytes",
                    }),
                    ok: accepted_upload(),
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            503,
                            "unavailable",
                            "uploads are not configured on this server",
                        ),
                        refusal(422, "invalid_upload", "the name, type or size refused"),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/assets/{asset}"),
        &[
            op(
                Verb::Get,
                |f| on(f, assets::get),
                OpDoc {
                    id: "getAsset",
                    summary: "One asset with every verified version, oldest first",
                    tag: "assets",
                    security: Security::Any,
                    ok: Res {
                        status: 200,
                        shape: Shape::It("AssetDetailView"),
                        headers: &[],
                        media: "",
                        note: "`data` is the asset; `versions` its byte history (D-136)",
                    },
                    extra: &[NOT_FOUND],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, assets::delete),
                OpDoc {
                    id: "deleteAsset",
                    summary: "Ask for the asset to go — 202 is requested, not gone",
                    tag: "assets",
                    security: Security::Any,
                    headers: &[IDEMPOTENCY],
                    ok: Res {
                        status: 202,
                        shape: Shape::Data("AssetView"),
                        headers: &[],
                        media: "",
                        note: "the worker removes the objects and lands `deleted`",
                    },
                    extra: &[
                        NOT_FOUND,
                        refusal(
                            409,
                            "already_deleted, asset_quarantined",
                            "on its way out already, or still being verified",
                        ),
                        refusal(
                            409,
                            "still_referenced",
                            "content still names it; `details.referenced_by` names them",
                        ),
                        P428,
                        BAD_MATCH,
                        REUSED_KEY,
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (P, "/assets/{asset}/replace"),
        &[op(
            Verb::Post,
            |f| on(f, assets::replace),
            OpDoc {
                id: "replaceAsset",
                summary: "New bytes for a usable asset — a new version, the old one keeps its objects",
                tag: "assets",
                security: Security::Any,
                headers: &[
                    IDEMPOTENCY,
                    header("x-colophon-file-name", Kind::Str, "the file's name"),
                    header(
                        "content-type",
                        Kind::Str,
                        "the claimed image type, verified by the worker",
                    ),
                ],
                body: Some(Body {
                    schema: "UploadBody",
                    media: &["application/octet-stream"],
                    note: "the file's bytes",
                }),
                ok: Res {
                    status: 202,
                    shape: Shape::Data("AssetView"),
                    headers: &[],
                    media: "",
                    note: "the reverify job swings the current version only if the bytes verify",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        409,
                        "asset_quarantined, asset_rejected, already_deleted",
                        "its first verdict is still out, it has no version to replace, or it is gone",
                    ),
                    refusal(
                        503,
                        "unavailable",
                        "uploads are not configured on this server",
                    ),
                    refusal(422, "invalid_upload", "the name, type or size refused"),
                    P428,
                    BAD_MATCH,
                    REUSED_KEY,
                ],
                ..MIN
            },
        )],
    ),
    route(
        (P, "/assets/{asset}/references"),
        &[op(
            Verb::Get,
            |f| on(f, assets::references),
            OpDoc {
                id: "assetReferences",
                summary: "The entries that name this asset, in collections the role reads",
                tag: "assets",
                security: Security::Any,
                ok: ok(200, Shape::List("ReferrerView"), ""),
                extra: &[NOT_FOUND],
                ..MIN
            },
        )],
    ),
    // Public delivery and media: no credential, the published projection only.
    // The audience (D-137): a project's site visitors — public signup and sessions on
    // `/audience/v1`, staff role and user management under `audience.manage` on `{P}`.
    route(
        ("", "/audience/v1/{project}/signup"),
        &[op(
            Verb::Post,
            |f| on(f, audience::sign_up),
            OpDoc {
                id: "audienceSignUp",
                summary: "Make a site-user account and start its session",
                tag: "audience",
                security: Security::Public,
                body: Some(json("AudienceSignUp")),
                ok: Res {
                    status: 201,
                    shape: Shape::Data("AudienceSession"),
                    headers: &["Set-Cookie"],
                    media: "",
                    note: "the session's `Set-Cookie` pair: `colophon_audience` and its CSRF sibling",
                },
                extra: &[
                    refusal(409, "email_taken", "an account with this email exists"),
                    refusal(
                        409,
                        "audience_full",
                        "the project is at its site-user bound",
                    ),
                    refusal(
                        422,
                        "invalid_email, weak_password, invalid_role, unavailable_role",
                        "the fields refused — a role not open to signup among them",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/audience/v1/{project}/signin"),
        &[op(
            Verb::Post,
            |f| on(f, audience::sign_in),
            OpDoc {
                id: "audienceSignIn",
                summary: "Sign a site user in with email and password",
                tag: "audience",
                security: Security::Public,
                body: Some(json("AudienceSignIn")),
                ok: Res {
                    status: 200,
                    shape: Shape::Data("AudienceSession"),
                    headers: &["Set-Cookie"],
                    media: "",
                    note: "the session's `Set-Cookie` pair",
                },
                extra: &[
                    refusal(
                        400,
                        "sign_in_failed",
                        "a wrong password and a missing account alike",
                    ),
                    refusal(409, "too_many_attempts", "the account is paused"),
                    refusal(403, "account_disabled", "staff disabled the account"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/audience/v1/{project}/signout"),
        &[op(
            Verb::Post,
            |f| on(f, audience::sign_out),
            OpDoc {
                id: "audienceSignOut",
                summary: "End the site session and clear its cookies",
                tag: "audience",
                security: Security::Public,
                headers: &[AUDIENCE_CSRF],
                ok: Res {
                    status: 204,
                    shape: Shape::Empty,
                    headers: &["Set-Cookie"],
                    media: "",
                    note: "the cleared `Set-Cookie` pair; signing out twice answers the same",
                },
                extra: &[refusal(
                    403,
                    "csrf_token_invalid",
                    "a cookie-carried session asked for the CSRF header",
                )],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/audience/v1/{project}/me"),
        &[op(
            Verb::Get,
            |f| on(f, audience::me),
            OpDoc {
                id: "audienceMe",
                summary: "The site user the session belongs to",
                tag: "audience",
                security: Security::Audience,
                ok: data("SiteUser"),
                ..MIN
            },
        )],
    ),
    route(
        (PR, "/audience/roles"),
        &[
            op(
                Verb::Get,
                |f| on(f, audience::list_roles),
                OpDoc {
                    id: "audienceRoles",
                    summary: "The project's audience roles",
                    tag: "audience",
                    ok: ok(200, Shape::List("AudienceRole"), "by name"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, audience::create_role),
                OpDoc {
                    id: "audienceRoleCreate",
                    summary: "Make an audience role",
                    tag: "audience",
                    body: Some(json("NewAudienceRole")),
                    ok: created("AudienceRole"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(409, "role_exists", "the project has this role already"),
                        refusal(422, "invalid_role", "the name refused"),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/audience/roles/{name}"),
        &[
            op(
                Verb::Patch,
                |f| on(f, audience::update_role),
                OpDoc {
                    id: "audienceRoleUpdate",
                    summary: "Open the role to signup, or close it",
                    tag: "audience",
                    body: Some(json("AudienceRoleChange")),
                    ok: data("AudienceRole"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Delete,
                |f| on(f, audience::delete_role),
                OpDoc {
                    id: "audienceRoleDelete",
                    summary: "Delete the audience role",
                    tag: "audience",
                    ok: ok(204, Shape::Empty, "deleted"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(
                            409,
                            "role_in_use",
                            "site users hold it — change their roles first",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/audience/users"),
        &[
            op(
                Verb::Get,
                |f| on(f, audience::list_users),
                OpDoc {
                    id: "audienceUsers",
                    summary: "The project's site users, newest first",
                    tag: "audience",
                    query: &[LIMIT, CURSOR],
                    ok: page("SiteUser"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Post,
                |f| on(f, audience::create_user),
                OpDoc {
                    id: "audienceUserCreate",
                    summary: "Make a site user, in any audience role",
                    tag: "audience",
                    body: Some(json("NewSiteUser")),
                    ok: created("SiteUser"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(409, "email_taken", "an account with this email exists"),
                        refusal(
                            409,
                            "audience_full",
                            "the project is at its site-user bound",
                        ),
                        refusal(
                            422,
                            "invalid_email, weak_password, unknown_audience_role",
                            "the fields refused",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/audience/users/{user}"),
        &[
            op(
                Verb::Get,
                |f| on(f, audience::get_user),
                OpDoc {
                    id: "audienceUser",
                    summary: "One site user",
                    tag: "audience",
                    ok: data("SiteUser"),
                    extra: &[NOT_FOUND, FORBIDDEN],
                    ..MIN
                },
            ),
            op(
                Verb::Patch,
                |f| on(f, audience::update_user),
                OpDoc {
                    id: "audienceUserUpdate",
                    summary: "Change a site user's role, or disable it",
                    tag: "audience",
                    body: Some(json("SiteUserChange")),
                    ok: data("SiteUser"),
                    extra: &[
                        NOT_FOUND,
                        FORBIDDEN,
                        refusal(
                            422,
                            "unknown_audience_role, empty_change",
                            "the change refused",
                        ),
                    ],
                    ..MIN
                },
            ),
        ],
    ),
    route(
        (PR, "/audience/users/{user}/sessions"),
        &[op(
            Verb::Get,
            |f| on(f, audience::list_sessions),
            OpDoc {
                id: "audienceUserSessions",
                summary: "The site user's sessions, live and dead",
                tag: "audience",
                ok: ok(200, Shape::List("SiteSession"), "newest first"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        (PR, "/audience/sessions/{session}"),
        &[op(
            Verb::Delete,
            |f| on(f, audience::revoke_session),
            OpDoc {
                id: "audienceSessionRevoke",
                summary: "Revoke one site session",
                tag: "audience",
                ok: ok(204, Shape::Empty, "revoked"),
                extra: &[NOT_FOUND, FORBIDDEN],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/content/v1/{project}/{environment}/{collection}"),
        &[op(
            Verb::Get,
            |f| on(f, delivery::list),
            OpDoc {
                id: "deliveryList",
                summary: "The published entries of a collection at a generation",
                tag: "delivery",
                security: Security::Public,
                query: &[GENERATION, LIMIT, CURSOR],
                headers: &[AUDIENCE_BEARER],
                ok: Res {
                    status: 200,
                    shape: Shape::Page("PublishedEntryView"),
                    headers: &["ETag", "Colophon-Generation"],
                    media: "",
                    note: "a matching `If-None-Match` gets 304",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        410,
                        "gone",
                        "the collection was published and has been taken down",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/content/v1/{project}/{environment}/{collection}/query"),
        &[op(
            Verb::Post,
            |f| on(f, delivery::query),
            OpDoc {
                id: "deliveryQuery",
                summary: "Run the bounded query language over the published projection",
                tag: "delivery",
                security: Security::Public,
                query: &[GENERATION, CURSOR],
                headers: &[AUDIENCE_BEARER],
                body: Some(json("QueryDocument")),
                ok: Res {
                    status: 200,
                    shape: Shape::It("DeliveryQueryPage"),
                    headers: &["ETag", "Colophon-Generation"],
                    media: "",
                    note: "a matching `If-None-Match` gets 304",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        422,
                        "invalid_cursor, invalid_*, too_large",
                        "the document or cursor refused",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/content/v1/{project}/{environment}/events"),
        &[op(
            Verb::Get,
            |f| on(f, crate::events::delivery_events),
            OpDoc {
                id: "streamDeliveryEvents",
                summary: "Live change hints for published content, anonymously",
                tag: "delivery",
                security: Security::Public,
                ok: Res {
                    status: 200,
                    shape: Shape::It("EventStream"),
                    headers: &[],
                    media: "text/event-stream",
                    note: "`hello`, then a `change` per release burst; no ids or content, so a reader only learns to re-read",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        503,
                        "unavailable",
                        "this server is carrying as many live streams as it can",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        (
            "",
            "/content/v1/{project}/{environment}/{collection}/{entry}",
        ),
        &[op(
            Verb::Get,
            |f| on(f, delivery::get),
            OpDoc {
                id: "deliveryGet",
                summary: "One published entry at a generation",
                tag: "delivery",
                security: Security::Public,
                query: &[GENERATION],
                headers: &[AUDIENCE_BEARER],
                ok: Res {
                    status: 200,
                    shape: Shape::Data("PublishedEntryView"),
                    headers: &["ETag", "Colophon-Generation"],
                    media: "",
                    note: "a matching `If-None-Match` gets 304",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(
                        410,
                        "gone",
                        "the entry was published and has been taken down",
                    ),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/media/v1/{project}/{environment}/{asset}"),
        &[op(
            Verb::Get,
            |f| on(f, assets::serve),
            OpDoc {
                id: "serveAsset",
                summary: "The asset's verified origin bytes, publicly and immutable",
                tag: "media",
                security: Security::Public,
                ok: Res {
                    status: 200,
                    shape: Shape::Empty,
                    headers: &[],
                    media: "*/*",
                    note: "the stored `Content-Type`; caches forever — bytes under an id never change",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(503, "unavailable", "the object store is not configured"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/media/v1/{project}/{environment}/{asset}/{name}"),
        &[op(
            Verb::Get,
            |f| on(f, assets::serve_rendition),
            OpDoc {
                id: "serveRendition",
                summary: "A rendition the worker wrote — or, when `name` is a 64-hex sha, that pinned version's original",
                tag: "media",
                security: Security::Public,
                ok: Res {
                    status: 200,
                    shape: Shape::Empty,
                    headers: &[],
                    media: "*/*",
                    note: "the stored `Content-Type`; caches forever",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(503, "unavailable", "the object store is not configured"),
                ],
                ..MIN
            },
        )],
    ),
    route(
        ("", "/media/v1/{project}/{environment}/{asset}/{sha}/{name}"),
        &[op(
            Verb::Get,
            |f| on(f, assets::serve_versioned),
            OpDoc {
                id: "serveVersionedRendition",
                summary: "A rendition of a pinned version: the worker wrote it under the version's key",
                tag: "media",
                security: Security::Public,
                ok: Res {
                    status: 200,
                    shape: Shape::Empty,
                    headers: &[],
                    media: "*/*",
                    note: "the stored `Content-Type`; caches forever",
                },
                extra: &[
                    NOT_FOUND,
                    refusal(503, "unavailable", "the object store is not configured"),
                ],
                ..MIN
            },
        )],
    ),
];

const fn accepted_upload() -> Res {
    Res {
        status: 202,
        shape: Shape::Data("AssetUploadView"),
        headers: &[],
        media: "",
        note: "quarantined; the verify job decides what it becomes",
    }
}

// ---------------------------------------------------------------------------
// Emitting the document

/// Where the document is served. Its own operation is emitted by [`document()`] rather than
/// living in [`TABLE`], because the handler answers the bytes `routes()` precomputed.
pub(crate) const SELF_PATH: &str = "/openapi.json";

/// The `OpenAPI` 3.1 document for `build`'s API surface, from the same [`TABLE`] the router is.
pub(crate) fn document(build: &BuildInfo) -> Value {
    let mut paths = Map::new();
    for route in TABLE {
        let mut operations = Map::new();
        for op in route.ops {
            operations.insert(op.verb.name().to_owned(), operation(route, op));
        }
        paths.insert(route.path(), Value::Object(operations));
    }
    paths.insert(SELF_PATH.to_owned(), self_operation());

    // `serde_json`'s `preserve_order` feature — which `colophon-domain` turns on, so it differs
    // between `cargo test -p` and `cargo test --workspace` builds — decides whether a `Value`'s
    // maps keep insertion order or sort. `sorted` rebuilds every object with its keys in order,
    // which both `Map` implementations serialize identically: the served bytes, the ETag over
    // them and the committed artifact are the same however the crate was built.
    sorted(&json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": {
            "title": "Colophon",
            "version": build.version,
            "description": "Colophon's HTTP API: the /api/v1 management surface the Studio, CLI, SDK and agents share, the public /content/v1 delivery and /media/v1 origins, and the server's own meta routes. Emitted from the route table the server itself is built from (D-110), so the document and the routes cannot drift.",
        },
        "paths": Value::Object(paths),
        "components": {
            "schemas": schemas(),
            "securitySchemes": {
                "sessionCookie": {
                    "type": "apiKey",
                    "in": "cookie",
                    "name": "colophon_session",
                    "description": "a browser session. Cookie-authenticated writes (anything but GET, HEAD, OPTIONS) also need an `Origin` the server trusts exactly and an `X-CSRF-Token` header matching the session's CSRF token, which lives in the readable `colophon_csrf` cookie."
                },
                "bearerToken": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "a personal access token, as `colophon login` gets from the device flow. A bearer request is authenticated by the token alone — no CSRF."
                },
                "apiToken": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "colophon_api_",
                    "description": "a project API token (`colophon_api_…`). It acts as its own identity with its own permissions, only on its own project's `/environments` routes and `/api/v1/rich-text/validate`: another project is 404, other routes are 403 `api_token_not_allowed`, and so is scheduling, which runs later as a person."
                },
                "audienceCookie": {
                    "type": "apiKey",
                    "in": "cookie",
                    "name": "colophon_audience",
                    "description": "a site user's session (D-137). `Authorization: Bearer <token>` works everywhere the cookie does; a cookie-carried write also echoes the CSRF token — the readable `colophon_audience_csrf` cookie — in `X-CSRF-Token`."
                }
            }
        },
        "tags": TAGS
            .iter()
            .map(|(name, description)| json!({ "name": name, "description": description }))
            .collect::<Vec<_>>(),
    }))
}

/// `value` with every object's keys in sorted order, recursively. `serde_json`'s `Map` is a
/// `BTreeMap` by default — always sorted — but an insertion-ordered map under the
/// `preserve_order` feature (D-110's note in [`document()`]); inserting keys sorted makes the
/// serialized bytes identical under either.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let mut out = Map::with_capacity(map.len());
            for key in keys {
                out.insert(key.clone(), sorted(&map[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        scalar => scalar.clone(),
    }
}

/// The tags the operations group under, in document order.
const TAGS: &[(&str, &str)] = &[
    (
        "meta",
        "health, version and this document — unauthenticated",
    ),
    ("auth", "sign-in, sessions, the device flow and `/me`"),
    ("setup", "the one-time first-administrator claim"),
    ("projects", "project lifecycle"),
    ("members", "members and single-use invitations"),
    ("roles", "roles as data, and the caller's own permissions"),
    (
        "permissions",
        "the caller's own permissions in an environment",
    ),
    ("tokens", "project API tokens and their kill switch"),
    ("audit", "the append-only audit log"),
    ("events", "live change hints over server-sent events"),
    (
        "mcp",
        "the remote-MCP endpoint — the agent protocol over HTTP (D-121)",
    ),
    (
        "audience",
        "the project's site visitors: public signup and sessions, staff-managed roles and users (D-137)",
    ),
    (
        "schema",
        "content types, shared components, migration plans",
    ),
    ("migrations", "schema migration plans and their items"),
    ("entries", "drafts, revisions, locales, queries, transfer"),
    ("collaboration", "comments and editorial tasks"),
    (
        "publishing",
        "changesets, reviews, stages, schedules, releases, receipts, destinations, jobs",
    ),
    ("assets", "uploads, verification and asset deletion"),
    ("rich-text", "the `colophon/rich-text` v1 validator"),
    (
        "delivery",
        "the published projection, publicly, at a generation",
    ),
    ("media", "verified asset bytes, publicly and immutable"),
];

/// The operation object for one `Op` on `route`'s path.
#[expect(
    clippy::too_many_lines,
    reason = "one operation's fields, emitted in one pass; splitting hides the shape"
)]
fn operation(route: &Route, op: &Op) -> Value {
    let doc = &op.doc;
    let mut parameters = Vec::new();
    for (name, kind) in path_params(&route.path()) {
        parameters.push(parameter(&name, kind, "path", true, path_note(&name)));
    }
    for param in doc.query {
        parameters.push(parameter(
            param.name,
            param.kind,
            "query",
            param.required,
            param.note,
        ));
    }
    for param in doc.headers {
        parameters.push(parameter(
            param.name,
            param.kind,
            "header",
            param.required,
            param.note,
        ));
    }

    let mut responses = Map::new();
    for res in [Some(&doc.ok), doc.also.as_ref()].into_iter().flatten() {
        let mut success = json!({ "description": res.note });
        if res.note.is_empty() {
            success["description"] = Value::String(match res.status {
                201 => "created".to_owned(),
                202 => "accepted".to_owned(),
                204 => "done".to_owned(),
                302 => "redirect".to_owned(),
                _ => "ok".to_owned(),
            });
        }
        if let Some(content) = content(res) {
            success["content"] = content;
        }
        if !res.headers.is_empty() {
            success["headers"] = response_headers(res.headers);
        }
        responses.insert(res.status.to_string(), success);
    }

    // The refusals this operation declares, merged into one response per status.
    let mut by_status: Vec<(u16, Vec<&'static Refusal>)> = Vec::new();
    for refusal in doc.extra {
        if let Some((_, group)) = by_status
            .iter_mut()
            .find(|(status, _)| *status == refusal.status)
        {
            group.push(refusal);
        } else {
            by_status.push((refusal.status, vec![refusal]));
        }
    }
    if doc.security != Security::Public {
        by_status.insert(0, (401, vec![&UNAUTHORIZED]));
    }
    let path = route.path();
    if path.starts_with("/api/") || path.starts_with("/auth/") || path.starts_with("/audience/") {
        by_status.push((429, vec![&RATE_LIMITED]));
    } else if path.starts_with("/content/") {
        by_status.push((429, vec![&RATE_LIMITED_DELIVERY]));
    }
    for (status, refusals) in by_status {
        if let Some(existing) = responses.get_mut(&status.to_string()) {
            // A declared refusal on the success status: merge notes.
            let mut description = existing["description"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            for refusal in refusals {
                if !description.is_empty() {
                    description.push_str("; ");
                }
                description.push_str(refusal.note);
            }
            existing["description"] = Value::String(description);
            continue;
        }
        let description = refusals
            .iter()
            .map(
                |refusal| match (refusal.codes.is_empty(), refusal.note.is_empty()) {
                    (false, false) => format!("`{}` — {}", refusal.codes, refusal.note),
                    (false, true) => format!("`{}`", refusal.codes),
                    (true, false) => refusal.note.to_owned(),
                    (true, true) => "refused".to_owned(),
                },
            )
            .collect::<Vec<_>>()
            .join("; ");
        responses.insert(
            status.to_string(),
            json!({
                "description": description,
                "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } },
            }),
        );
    }
    // Everything else the envelope can carry: 400 malformed_json, 413, 500, 503.
    responses.insert(
        "default".to_owned(),
        json!({
            "description": "any other failure, in the one error envelope",
            "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } },
        }),
    );

    let mut operation = json!({
        "operationId": doc.id,
        "summary": doc.summary,
        "tags": [doc.tag],
        "responses": Value::Object(responses),
    });
    if !parameters.is_empty() {
        operation["parameters"] = Value::Array(parameters);
    }
    if let Some(body) = &doc.body {
        let content = body
            .media
            .iter()
            .map(|media| {
                (
                    (*media).to_owned(),
                    json!({ "schema": { "$ref": format!("#/components/schemas/{}", body.schema) } }),
                )
            })
            .collect::<Map<_, _>>();
        let mut request_body = json!({
            "required": true,
            "content": Value::Object(content),
        });
        if !body.note.is_empty() {
            request_body["description"] = Value::String(body.note.to_owned());
        }
        operation["requestBody"] = request_body;
    }
    match doc.security {
        Security::Public => {
            operation["security"] = json!([]);
        }
        Security::Any => {
            operation["security"] =
                json!([{ "sessionCookie": [] }, { "bearerToken": [] }, { "apiToken": [] }]);
        }
        Security::Person => {
            operation["security"] = json!([{ "sessionCookie": [] }, { "bearerToken": [] }]);
        }
        Security::Session => {
            operation["security"] = json!([{ "sessionCookie": [] }]);
        }
        Security::ApiToken => {
            operation["security"] = json!([{ "apiToken": [] }]);
        }
        Security::Audience => {
            operation["security"] = json!([{ "audienceCookie": [] }, { "bearerToken": [] }]);
        }
    }
    operation
}

/// This document's own operation on [`SELF_PATH`].
fn self_operation() -> Value {
    json!({
        "get": {
            "operationId": "openapiDocument",
            "summary": "This OpenAPI document",
            "tags": ["meta"],
            "security": [],
            "responses": {
                "200": {
                    "description": "the document; `ETag`'d for conditional requests",
                    "headers": {
                        "ETag": { "schema": { "type": "string" }, "description": "the document's own hash" },
                    },
                    "content": { "application/json": { "schema": { "$ref": "#/components/schemas/OpenApiDocument" } } },
                },
                "304": { "description": "unchanged since the caller's `If-None-Match`" },
            },
        },
    })
}

/// A parameter object: `kind` shapes its schema, `note` its description.
fn parameter(name: &str, kind: Kind, location: &str, required: bool, note: &str) -> Value {
    let schema = match kind {
        Kind::Str | Kind::Cursor => json!({ "type": "string" }),
        Kind::Uuid => json!({ "type": "string", "format": "uuid" }),
        Kind::Int => json!({ "type": "integer", "minimum": 0 }),
        Kind::Bool => json!({ "type": "boolean" }),
    };
    json!({
        "name": name,
        "in": location,
        "required": required,
        "schema": schema,
        "description": note,
    })
}

/// The `{param}` placeholders a path declares, in order, with their kind.
fn path_params(path: &str) -> Vec<(String, Kind)> {
    let mut params = Vec::new();
    for segment in path.split('/') {
        if let Some(name) = segment
            .strip_prefix('{')
            .and_then(|segment| segment.strip_suffix('}'))
        {
            let kind = match name {
                "project" | "identity" | "invitation" | "token" | "entry" | "revision"
                | "comment" | "task" | "migration" | "changeset" | "schedule" | "destination"
                | "asset" | "user" | "session" => Kind::Uuid,
                "generation" => Kind::Int,
                _ => Kind::Str,
            };
            params.push((name.to_owned(), kind));
        }
    }
    params
}

/// The prose a path parameter carries, by name.
fn path_note(name: &str) -> &'static str {
    match name {
        "project" => "the project's id",
        "environment" => "the environment's slug, such as `production`",
        "collection" => "the collection's slug",
        "slug" => "the type's or component's slug",
        "role" => "the role's key",
        "identity" => "the member's identity id",
        "invitation" => "the invitation's id",
        "token" => "the token's id",
        "entry" => "the entry's id",
        "revision" => "the revision's id",
        "comment" => "the comment's id",
        "task" => "the task's id",
        "migration" => "the plan's id",
        "changeset" => "the changeset's id",
        "schedule" => "the schedule's id",
        "destination" => "the destination's id",
        "asset" => "the asset's id",
        "generation" => "the release's generation",
        "code" => "the user code the device showed",
        "name" => "a rendition name the worker wrote, such as `w320`; or an audience role's",
        "user" => "the site user's id",
        "session" => "the session's id",
        _ => "the path parameter",
    }
}

/// The `content` map for a success response whose shape has a body; `None` for empty.
fn content(res: &Res) -> Option<Value> {
    let media = if res.media.is_empty() {
        "application/json"
    } else {
        res.media
    };
    let schema = match &res.shape {
        Shape::Empty => return None,
        Shape::Data(schema) => json!({
            "type": "object",
            "required": ["data"],
            "properties": { "data": { "$ref": format!("#/components/schemas/{schema}") } },
        }),
        Shape::List(schema) => json!({
            "type": "object",
            "required": ["data"],
            "properties": {
                "data": {
                    "type": "array",
                    "items": { "$ref": format!("#/components/schemas/{schema}") },
                },
            },
        }),
        Shape::Page(schema) => json!({
            "type": "object",
            "required": ["data", "next_cursor"],
            "properties": {
                "data": {
                    "type": "array",
                    "items": { "$ref": format!("#/components/schemas/{schema}") },
                },
                "next_cursor": {
                    "type": ["string", "null"],
                    "description": "pass back as `?cursor=`; `null` on the last page",
                },
            },
        }),
        Shape::It(schema) => json!({ "$ref": format!("#/components/schemas/{schema}") }),
    };
    Some(json!({ media: { "schema": schema } }))
}

/// Response `headers` objects from their names.
fn response_headers(names: &[&'static str]) -> Value {
    names
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                json!({ "schema": { "type": "string" } }),
            )
        })
        .collect::<Map<_, _>>()
        .into()
}

// ---------------------------------------------------------------------------
// Serving

/// The bytes `GET /openapi.json` answers, computed once when the router is built.
pub(crate) struct Served {
    body: Bytes,
    etag: String,
}

/// `Cache-Control`: the document changes only when the binary does, so clients may keep it a
/// while and revalidate with the `ETag`.
const CACHE_CONTROL: &str = "public, max-age=300";

impl Served {
    /// Builds the document for `build` and fixes its bytes and `ETag`.
    pub(crate) fn new(build: &BuildInfo) -> Self {
        // `serde_json::to_vec` on a `Value` cannot fail.
        let body: Bytes = serde_json::to_vec(&document(build))
            .unwrap_or_else(|_| Vec::new())
            .into();
        let etag = format!("\"sha256:{}\"", hex(&crate::entries::sha256(&body)));
        Self { body, etag }
    }

    /// The handler's answer: the document, or 304 against a matching `If-None-Match`.
    pub(crate) fn respond(&self, headers: &HeaderMap) -> Response {
        if headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| none_match(value, &self.etag))
        {
            return (
                StatusCode::NOT_MODIFIED,
                [
                    (header::ETAG, self.etag.as_str()),
                    (header::CACHE_CONTROL, CACHE_CONTROL),
                ],
            )
                .into_response();
        }
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::ETAG, self.etag.as_str()),
                (header::CACHE_CONTROL, CACHE_CONTROL),
            ],
            self.body.clone(),
        )
            .into_response()
    }
}

/// RFC 9110 §13.1.2 weak comparison, over the whole `If-None-Match` list.
fn none_match(header: &str, etag: &str) -> bool {
    header
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag == etag)
}

/// Lowercase hex, for `ETag` values.
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    })
}

// ---------------------------------------------------------------------------
// Schemas: the serde view and body types, scribed by hand as JSON Schema (D-110).

/// The `components.schemas` object, assembled from family parts: one `json!` per family keeps
/// the macro below the recursion limit and keeps each schema near its kin.
fn schemas() -> Value {
    let mut schemas = Map::new();
    for part in [
        shared(),
        identity(),
        membership(),
        audience_parts(),
        schema_parts(),
        entry_parts(),
        collaboration_parts(),
        publishing_parts(),
        delivery_parts(),
        mcp_parts(),
    ] {
        if let Value::Object(map) = part {
            schemas.extend(map);
        }
    }
    Value::Object(schemas)
}

/// The error envelope, its details, the meta routes and this document itself.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn shared() -> Value {
    json!({
        "Error": {
            "type": "object",
            "description": "the one error envelope every failure answers in, middleware-made or handler-made",
            "required": ["error"],
            "properties": {
                "error": {
                    "type": "object",
                    "required": ["code", "message", "request_id"],
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "stable, machine-readable; branch on this, never on `message`",
                            "enum": [
                                "bad_request", "malformed_json", "validation_failed",
                                "unauthorized", "forbidden", "not_found", "gone", "conflict",
                                "precondition_required", "sign_in_failed", "route_not_found",
                                "method_not_allowed", "payload_too_large",
                                "unsupported_media_type", "deadline_exceeded", "rate_limited",
                                "unavailable", "internal"
                            ],
                        },
                        "message": { "type": "string", "description": "safe to show a person" },
                        "request_id": { "type": "string", "format": "uuid", "description": "matches the `x-request-id` header" },
                        "details": { "$ref": "#/components/schemas/ErrorDetails" },
                    },
                },
            },
        },
        "ErrorDetails": {
            "type": "object",
            "description": "machine-readable detail; present fields depend on `reason`",
            "properties": {
                "reason": { "type": "string", "description": "a stable code, such as `slug_taken` or `missing_permission`" },
                "pointer": { "type": "string", "description": "an RFC 6901 JSON Pointer into the request body; empty means the whole body" },
                "entries": { "type": "array", "items": { "$ref": "#/components/schemas/EntryInTheWay" } },
                "total": { "type": "integer" },
                "candidates": { "type": "array", "items": { "type": "string", "format": "date-time" } },
                "permission": { "type": "string" },
                "content_type": { "type": "string" },
                "referenced_by": { "type": "array", "items": { "$ref": "#/components/schemas/ReferrerView" } },
                "changesets": { "type": "array", "items": { "$ref": "#/components/schemas/PendingChangeset" } },
            },
        },
        "EntryInTheWay": {
            "type": "object",
            "properties": {
                "entry": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "state": { "type": "string", "enum": ["draft", "published"] },
                "problem": { "$ref": "#/components/schemas/Problem" },
            },
        },
        "Problem": {
            "type": "object",
            "properties": {
                "reason": { "type": "string" },
                "pointer": { "type": "string" },
                "message": { "type": "string" },
            },
        },
        "PendingChangeset": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "scheduled": { "type": "boolean" },
            },
        },
        "Liveness": {
            "type": "object",
            "required": ["status"],
            "properties": { "status": { "type": "string", "enum": ["live"] } },
        },
        "ReadinessReport": {
            "type": "object",
            "required": ["status", "role", "checks"],
            "properties": {
                "status": { "type": "string", "enum": ["starting", "ready", "degraded", "not_ready", "draining"] },
                "role": { "type": "string", "enum": ["api", "worker", "all"] },
                "checks": {
                    "type": "object",
                    "properties": {
                        "database": { "type": "string", "enum": ["unknown", "up", "down", "not_configured"] },
                        "cache": { "type": "string", "enum": ["unknown", "up", "down", "not_configured"] },
                    },
                },
            },
        },
        "VersionReport": {
            "type": "object",
            "description": "the shape never changes: fields may be added, none removed or renamed",
            "required": ["version", "commit", "build_date", "api"],
            "properties": {
                "version": { "type": "string", "description": "the release, such as `0.1.0-alpha.1`" },
                "commit": { "type": "string", "description": "the git commit, or `unknown`" },
                "build_date": { "type": "string", "description": "the day it was built, or `unknown`" },
                "api": { "type": "array", "items": { "type": "string" }, "description": "the API versions served, oldest first" },
            },
        },
        "OpenApiDocument": {
            "type": "object",
            "description": "an OpenAPI 3.1 document — this one",
        },
    })
}

/// Who the caller is, sign-in, the device flow, setup.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn identity() -> Value {
    json!({
        "Me": {
            "type": "object",
            "required": ["id", "issuer", "subject", "administrator", "installation", "projects"],
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "issuer": { "type": "string" },
                "subject": { "type": "string" },
                "display_name": { "type": ["string", "null"] },
                "administrator": { "type": "boolean" },
                "installation": { "$ref": "#/components/schemas/Installation" },
                "projects": { "type": "array", "items": { "$ref": "#/components/schemas/ProjectView" } },
            },
        },
        "Installation": {
            "type": "object",
            "required": ["has_administrator"],
            "properties": { "has_administrator": { "type": "boolean" } },
        },
        "AuthProviders": {
            "type": "object",
            "required": ["sso"],
            "properties": { "sso": { "type": "boolean", "description": "an external identity provider is configured" } },
        },
        "SignedOut": {
            "type": "object",
            "properties": { "provider_sign_out": { "type": ["string", "null"] } },
        },
        "DeviceStart": {
            "type": "object",
            "required": ["client_name"],
            "additionalProperties": false,
            "properties": { "client_name": { "type": "string", "minLength": 1, "maxLength": 100 } },
        },
        "DeviceStarted": {
            "type": "object",
            "required": ["device_secret", "user_code", "verification_uri", "interval", "expires_in"],
            "properties": {
                "device_secret": { "type": "string", "description": "what `device/token` polls with" },
                "user_code": { "type": "string" },
                "verification_uri": { "type": "string" },
                "interval": { "type": "integer" },
                "expires_in": { "type": "integer" },
            },
        },
        "DeviceApprove": {
            "type": "object",
            "required": ["user_code"],
            "additionalProperties": false,
            "properties": { "user_code": { "type": "string" } },
        },
        "DevicePoll": {
            "type": "object",
            "required": ["device_secret"],
            "additionalProperties": false,
            "properties": { "device_secret": { "type": "string" } },
        },
        "DevicePolled": {
            "type": "object",
            "required": ["status"],
            "properties": {
                "status": { "type": "string", "enum": ["pending", "approved"] },
                "access_token": { "type": "string", "description": "only once approved" },
                "expires_at": { "type": "string", "format": "date-time" },
            },
        },
        "DevicePending": {
            "type": "object",
            "properties": {
                "client_name": { "type": "string" },
                "expires_at": { "type": "string", "format": "date-time" },
                "approved": { "type": "boolean" },
            },
        },
        "SignUp": {
            "type": "object",
            "required": ["email", "password"],
            "additionalProperties": false,
            "properties": {
                "email": { "type": "string", "format": "email" },
                "password": { "type": "string", "minLength": 8 },
                "display_name": { "type": "string" },
            },
        },
        "PasswordSignIn": {
            "type": "object",
            "required": ["email", "password"],
            "additionalProperties": false,
            "properties": {
                "email": { "type": "string", "format": "email" },
                "password": { "type": "string" },
            },
        },
        "SetupCode": {
            "type": "object",
            "required": ["code"],
            "additionalProperties": false,
            "properties": { "code": { "type": "string" } },
        },
        "SetUp": {
            "type": "object",
            "properties": { "administrator": { "type": "boolean" } },
        },
    })
}

/// Projects, members, invitations, roles, tokens, the audit log, live events, rich text.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn membership() -> Value {
    json!({
        "NewProject": {
            "type": "object",
            "required": ["slug", "name"],
            "additionalProperties": false,
            "properties": {
                "slug": { "type": "string" },
                "name": { "type": "string" },
            },
        },
        "ProjectView": {
            "type": "object",
            "required": ["id", "slug", "name", "role", "role_name"],
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "slug": { "type": "string" },
                "name": { "type": "string" },
                "role": { "type": "string" },
                "role_name": { "type": "string" },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionView" }, "description": "only when one project is read" },
                "self_review": { "type": "boolean", "description": "only when one project is read" },
            },
        },
        "PermissionView": {
            "type": "object",
            "required": ["action", "subject"],
            "properties": {
                "action": { "type": "string", "description": "such as `content.update`" },
                "subject": { "type": "string", "description": "a collection slug, or the whole project" },
                "fields": { "type": "array", "items": { "type": "string" }, "description": "a field list; absent means all fields" },
                "condition": { "type": "string", "enum": ["own"], "description": "absent means anyone's entries" },
            },
        },
        "PermissionBody": {
            "type": "object",
            "required": ["action", "subject"],
            "additionalProperties": false,
            "properties": {
                "action": { "type": "string" },
                "subject": { "type": "string" },
                "fields": { "type": "array", "items": { "type": "string" } },
                "condition": { "type": "string", "enum": ["own"] },
            },
        },
        "MemberView": {
            "type": "object",
            "required": ["identity_id", "role", "joined_at"],
            "properties": {
                "identity_id": { "type": "string", "format": "uuid" },
                "display_name": { "type": ["string", "null"] },
                "role": { "type": "string" },
                "joined_at": { "type": "string", "format": "date-time" },
            },
        },
        "MemberRole": {
            "type": "object",
            "required": ["role"],
            "additionalProperties": false,
            "properties": { "role": { "type": "string", "description": "the role's key" } },
        },
        "InvitationView": {
            "type": "object",
            "required": ["id", "role", "label", "created_at", "expires_at"],
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "role": { "type": "string" },
                "label": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
                "expires_at": { "type": "string", "format": "date-time" },
                "link": { "type": "string", "description": "the usable URL; only in the create answer" },
            },
        },
        "NewInvitation": {
            "type": "object",
            "required": ["role", "label"],
            "additionalProperties": false,
            "properties": {
                "role": { "type": "string" },
                "label": { "type": "string", "minLength": 1, "maxLength": 100 },
            },
        },
        "InvitationToken": {
            "type": "object",
            "required": ["token"],
            "additionalProperties": false,
            "properties": { "token": { "type": "string" } },
        },
        "OfferView": {
            "type": "object",
            "properties": {
                "project_id": { "type": "string", "format": "uuid" },
                "project_name": { "type": "string" },
                "role": { "type": "string" },
                "role_name": { "type": "string" },
                "role_description": { "type": "string" },
                "invited_by": { "type": ["string", "null"] },
                "expires_at": { "type": "string", "format": "date-time" },
            },
        },
        "JoinedView": {
            "type": "object",
            "properties": {
                "project_id": { "type": "string", "format": "uuid" },
                "role": { "type": "string" },
            },
        },
        "RoleView": {
            "type": "object",
            "required": ["key", "name", "description", "built_in", "fixed", "version", "permissions", "members", "invitations", "updated_at"],
            "properties": {
                "key": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "built_in": { "type": "boolean" },
                "fixed": { "type": "boolean" },
                "version": { "type": "integer" },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionView" } },
                "members": { "type": "integer" },
                "invitations": { "type": "integer" },
                "updated_at": { "type": "string", "format": "date-time" },
            },
        },
        "NewRole": {
            "type": "object",
            "required": ["key", "name", "permissions"],
            "additionalProperties": false,
            "properties": {
                "key": { "type": "string" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionBody" } },
            },
        },
        "RoleChange": {
            "type": "object",
            "required": ["name", "permissions"],
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionBody" } },
            },
        },
        "OwnView": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "description": "`person` or `token`" },
                "role": { "type": ["string", "null"] },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionView" } },
            },
        },
        "TokenView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "identity_id": { "type": "string", "format": "uuid" },
                "name": { "type": "string" },
                "description": { "type": "string" },
                "kind": { "type": "string", "enum": ["read-only", "full-access", "custom"] },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionView" } },
                "state": { "type": "string", "enum": ["active", "expired", "revoked"] },
                "created_by": { "$ref": "#/components/schemas/PersonView" },
                "created_at": { "type": "string", "format": "date-time" },
                "expires_at": { "type": "string", "format": "date-time" },
                "last_used_at": { "type": ["string", "null"], "format": "date-time" },
                "revoked_at": { "type": ["string", "null"], "format": "date-time" },
                "limits": { "anyOf": [{ "$ref": "#/components/schemas/TokenLimits" }, { "type": "null" }], "description": "budget ceilings; `null` is unconstrained at the quota layer" },
                "usage": { "$ref": "#/components/schemas/TokenUsage", "description": "what it holds or spent; only on the single-token answer" },
                "secret": { "type": "string", "description": "`colophon_api_…`; only in the create answer, shown once" },
            },
        },
        "TokenLimits": {
            "type": "object",
            "properties": {
                "documents_per_day": { "type": ["integer", "null"], "minimum": 1, "maximum": 1_000_000, "description": "entry writes per rolling day" },
                "max_concurrent_jobs": { "type": ["integer", "null"], "minimum": 1, "maximum": 64, "description": "delivery jobs held at once" },
                "requests_per_minute": { "type": ["integer", "null"], "minimum": 1, "maximum": 100_000, "description": "management requests per rolling minute; over it, 429 `rate_limited` with `Retry-After` (D-118)" },
                "tokens_per_day": { "type": ["integer", "null"], "minimum": 1, "maximum": 1_000_000_000, "description": "provider tokens per rolling day (D-122)" },
                "cost_per_day_micros": { "type": ["integer", "null"], "minimum": 1, "maximum": 100_000_000_000i64, "description": "provider cost per rolling day, in micros of the accounting unit — the unit's millionths, a fixed-point integer (D-122)" },
            },
        },
        "TokenUsage": {
            "type": "object",
            "properties": {
                "documents_last_day": { "type": "integer" },
                "jobs_open": { "type": "integer" },
                "requests_last_minute": { "type": "integer", "description": "the limiter's current rolling window (D-118); 0 without the ceiling or when unreachable" },
                "tokens_last_day": { "type": "integer", "description": "provider tokens in the last day; settled outcomes at what they consumed, open reservations at their allowance" },
                "cost_last_day": { "type": "integer", "description": "provider cost in the last day, in micros of the accounting unit" },
            },
        },
        "PatchToken": {
            "type": "object",
            "required": ["limits"],
            "additionalProperties": false,
            "properties": {
                "limits": { "anyOf": [{ "$ref": "#/components/schemas/TokenLimits" }, { "type": "null" }], "description": "the token's ceilings; `null` removes them" },
            },
        },
        "PersonView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "name": { "type": ["string", "null"] },
            },
        },
        "NewToken": {
            "type": "object",
            "required": ["name", "kind", "expires_in_days"],
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "kind": { "type": "string", "enum": ["read-only", "full-access", "custom"] },
                "permissions": { "type": "array", "items": { "$ref": "#/components/schemas/PermissionBody" }, "description": "`custom` only" },
                "expires_in_days": { "type": "integer", "minimum": 1, "maximum": 366 },
                "limits": { "$ref": "#/components/schemas/TokenLimits", "description": "budget ceilings (D-113); absent means unconstrained" },
            },
        },
        "EventView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "at": { "type": "string", "format": "date-time" },
                "actor": { "$ref": "#/components/schemas/ActorView" },
                "action": { "type": "string" },
                "subject": { "type": "string" },
                "detail": { "type": "object" },
            },
        },
        "ActorView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "name": { "type": ["string", "null"] },
                "kind": { "type": "string", "enum": ["person", "token"] },
            },
        },
        "EventStream": {
            "type": "string",
            "description": "a `text/event-stream` of `hello` once, then `change` events `{\"k\",\"c\"?,\"e\"?}` scoped to the stream's environment, `resync` on a missed hint, and keep-alive comments",
        },
        "RichTextDocument": {
            "type": "object",
            "description": "a `colophon/rich-text` v1 document (docs/formats/rich-text-v1.md)",
        },
        "RichTextVerdict": {
            "type": "object",
            "properties": {
                "value": { "type": "object", "description": "the canonical document" },
                "nodes": { "type": "integer" },
                "depth": { "type": "integer" },
                "bytes": { "type": "integer" },
            },
        },
    })
}

/// The audience (D-137): signup and sign-in bodies, the session they answer, and the
/// staff-managed roles, site users and sessions.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn audience_parts() -> Value {
    json!({
        "AudienceSignUp": {
            "type": "object",
            "required": ["email", "password"],
            "properties": {
                "email": { "type": "string", "description": "the account's email — trimmed, lowered" },
                "password": { "type": "string", "description": "at least 8 characters" },
                "display_name": { "type": "string", "description": "a name to show; optional" },
                "role": { "type": "string", "description": "an audience role open to signup; left out, the project's single open role is taken" },
            },
            "additionalProperties": false,
        },
        "AudienceSignIn": {
            "type": "object",
            "required": ["email", "password"],
            "properties": {
                "email": { "type": "string" },
                "password": { "type": "string" },
            },
            "additionalProperties": false,
        },
        "AudienceSession": {
            "type": "object",
            "required": ["user", "session", "csrf"],
            "properties": {
                "user": { "$ref": "#/components/schemas/SiteUser" },
                "session": { "type": "string", "description": "the session token — `Authorization: Bearer` it, or let the `Set-Cookie` pair carry it" },
                "csrf": { "type": "string", "description": "the CSRF token — cookie-carried writes echo it in `X-CSRF-Token`" },
            },
        },
        "SiteUser": {
            "type": "object",
            "required": ["id", "email", "display_name", "role", "created_at", "disabled"],
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "email": { "type": "string" },
                "display_name": { "type": ["string", "null"] },
                "role": { "type": "string", "description": "the audience role it holds — the label `role:<name>` visibility grants to" },
                "created_at": { "type": "string", "format": "date-time" },
                "last_sign_in_at": { "type": "string", "format": "date-time", "description": "present once it has signed in" },
                "disabled": { "type": "boolean", "description": "staff disabled it: its sessions are dead and sign-in says so" },
            },
        },
        "AudienceRole": {
            "type": "object",
            "required": ["name", "self_assign", "created_at"],
            "properties": {
                "name": { "type": "string", "description": "what `role:<name>` visibility and a site user's `role` hold" },
                "self_assign": { "type": "boolean", "description": "whether public signup may choose it" },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "NewAudienceRole": {
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string", "description": "1–63 of a–z, 0–9 and '-', starting with a letter" },
                "self_assign": { "type": "boolean", "description": "whether signup may choose it; false unless said" },
            },
            "additionalProperties": false,
        },
        "AudienceRoleChange": {
            "type": "object",
            "required": ["self_assign"],
            "properties": {
                "self_assign": { "type": "boolean" },
            },
            "additionalProperties": false,
        },
        "NewSiteUser": {
            "type": "object",
            "required": ["email", "password", "role"],
            "properties": {
                "email": { "type": "string" },
                "password": { "type": "string", "description": "at least 8 characters" },
                "display_name": { "type": "string" },
                "role": { "type": "string", "description": "any audience role the project has, open or not" },
            },
            "additionalProperties": false,
        },
        "SiteUserChange": {
            "type": "object",
            "description": "a role and/or a disable — at least one",
            "properties": {
                "role": { "type": "string" },
                "disabled": { "type": "boolean", "description": "true disables and revokes its sessions, false re-enables" },
            },
            "additionalProperties": false,
        },
        "SiteSession": {
            "type": "object",
            "required": ["id", "created_at", "last_seen_at", "expires_at", "revoked"],
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "created_at": { "type": "string", "format": "date-time" },
                "last_seen_at": { "type": "string", "format": "date-time" },
                "expires_at": { "type": "string", "format": "date-time", "description": "its absolute end, use or not" },
                "revoked": { "type": "boolean" },
            },
        },
    })
}

/// Content types, shared components, migration plans and their items.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn schema_parts() -> Value {
    json!({
        "NewType": {
            "type": "object",
            "required": ["slug", "definition"],
            "additionalProperties": false,
            "properties": {
                "slug": { "type": "string" },
                "definition": { "$ref": "#/components/schemas/TypeDefinition" },
            },
        },
        "TypeDefinition": {
            "type": "object",
            "description": "a content type definition: `kind` (`collection` or `single`), `title` field name, `fields` (each with its `type` and options), `locales` policy. Rust revalidates every mutation against it; its canonical form hashes to `definition_hash`.",
        },
        "TypeVersionView": {
            "type": "object",
            "properties": {
                "slug": { "type": "string" },
                "version": { "type": "integer" },
                "definition": { "$ref": "#/components/schemas/TypeDefinition" },
                "definition_hash": { "type": "string", "description": "`sha256:<hex>` of the canonical definition" },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "RemovedView": {
            "type": "object",
            "properties": {
                "slug": { "type": "string" },
                "removed": { "type": "boolean" },
            },
        },
        "NewComponent": {
            "type": "object",
            "required": ["slug", "definition"],
            "additionalProperties": false,
            "properties": {
                "slug": { "type": "string" },
                "definition": { "$ref": "#/components/schemas/ComponentDefinition" },
            },
        },
        "ComponentDefinition": {
            "type": "object",
            "description": "a shared component's definition: its `fields`, reused as `{\"type\": \"component\", \"component\": <slug>}` in content types",
        },
        "ComponentView": {
            "type": "object",
            "properties": {
                "slug": { "type": "string" },
                "version": { "type": "integer" },
                "definition": { "$ref": "#/components/schemas/ComponentDefinition" },
                "definition_hash": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
                "content_types": { "type": "array", "items": { "$ref": "#/components/schemas/TypeVersion" }, "description": "the types a change versioned along with it" },
            },
        },
        "TypeVersion": {
            "type": "object",
            "properties": {
                "slug": { "type": "string" },
                "version": { "type": "integer" },
            },
        },
        "NewPlan": {
            "type": "object",
            "required": ["collection", "definition"],
            "additionalProperties": false,
            "properties": {
                "collection": { "type": "string" },
                "definition": { "$ref": "#/components/schemas/TypeDefinition" },
                "operations": { "type": "object", "description": "declared `fill`/`move` operations for values the target can't derive" },
            },
        },
        "NewResolution": {
            "type": "object",
            "required": ["resolution"],
            "additionalProperties": false,
            "properties": {
                "resolution": { "type": "object", "description": "the entry's content under the target definition, as a save would store it" },
            },
        },
        "ItemsCount": {
            "type": "object",
            "properties": {
                "expected": { "type": "integer" },
                "done": { "type": "integer" },
                "blocked": { "type": "integer" },
            },
        },
        "MigrationSummaryView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "base_version": { "type": "integer" },
                "target_version": { "type": ["integer", "null"] },
                "state": { "type": "string", "enum": ["proposed", "approved", "running", "done", "blocked", "superseded", "cancelled"] },
                "hash": { "type": "string", "description": "`sha256:<hex>` — the plan's `ETag`" },
                "items": { "$ref": "#/components/schemas/ItemsCount" },
                "created_by": { "type": "string", "format": "uuid" },
                "approved_by": { "type": ["string", "null"], "format": "uuid" },
                "approved_at": { "type": ["string", "null"], "format": "date-time" },
                "started_at": { "type": ["string", "null"], "format": "date-time" },
                "finished_at": { "type": ["string", "null"], "format": "date-time" },
                "error": { "type": ["string", "null"] },
                "created_at": { "type": "string", "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
            },
        },
        "PlanView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "base_version": { "type": "integer" },
                "target_version": { "type": ["integer", "null"] },
                "target_definition": { "$ref": "#/components/schemas/TypeDefinition" },
                "target_definition_hash": { "type": "string" },
                "operations": { "type": ["object", "null"] },
                "state": { "type": "string", "enum": ["proposed", "approved", "running", "done", "blocked", "superseded", "cancelled"] },
                "hash": { "type": "string" },
                "items": { "$ref": "#/components/schemas/ItemsCount" },
                "republish_required": { "type": "boolean" },
                "created_by": { "type": "string", "format": "uuid" },
                "approved_by": { "type": ["string", "null"], "format": "uuid" },
                "approved_at": { "type": ["string", "null"], "format": "date-time" },
                "started_at": { "type": ["string", "null"], "format": "date-time" },
                "finished_at": { "type": ["string", "null"], "format": "date-time" },
                "error": { "type": ["string", "null"] },
                "created_at": { "type": "string", "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
            },
        },
        "PlanRunView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["running"] },
                "hash": { "type": "string" },
                "started_at": { "type": ["string", "null"], "format": "date-time" },
            },
        },
        "MigrationItemView": {
            "type": "object",
            "properties": {
                "entry_id": { "type": "string", "format": "uuid" },
                "expected_revision_id": { "type": "string", "format": "uuid" },
                "title": { "type": ["string", "null"] },
                "outcome": { "type": "string", "enum": ["ok", "migrates", "needs_input", "blocked", "resolved", "applied", "skipped"] },
                "problem": { "type": ["object", "null"], "description": "the refusal the stored revision would get" },
                "resolution": { "type": ["object", "null"] },
                "new_revision_id": { "type": ["string", "null"], "format": "uuid" },
                "applied_at": { "type": ["string", "null"], "format": "date-time" },
                "republish_required": { "type": "boolean" },
            },
        },
        "ResolvedView": {
            "type": "object",
            "properties": {
                "hash": { "type": "string" },
                "item": { "$ref": "#/components/schemas/MigrationItemView" },
            },
        },
        "BulkItemInput": {
            "type": "object",
            "required": ["key", "entry", "expected_revision", "op"],
            "additionalProperties": false,
            "properties": {
                "key": { "type": "string", "description": "the client's stable name for the item within the job (1–256 characters)" },
                "entry": { "type": "string", "format": "uuid" },
                "expected_revision": { "type": "string", "format": "uuid", "description": "the base revision the write is bound to" },
                "op": { "type": "object", "description": "`{\"op\": \"update\", \"content\": {…}}`, `{\"op\": \"set\", \"path\", \"value\"}`, `{\"op\": \"move\", \"from\", \"to\"}` or `{\"op\": \"delete\"}`" },
            },
        },
        "NewBulkJob": {
            "type": "object",
            "required": ["collection", "items"],
            "additionalProperties": false,
            "properties": {
                "collection": { "type": "string" },
                "items": { "type": "array", "items": { "$ref": "#/components/schemas/BulkItemInput" }, "minItems": 1, "maxItems": 100 },
                "seal": { "type": "boolean", "description": "close the manifest at once and start the run" },
            },
        },
        "AppendBulkItems": {
            "type": "object",
            "required": ["items"],
            "additionalProperties": false,
            "properties": {
                "items": { "type": "array", "items": { "$ref": "#/components/schemas/BulkItemInput" }, "minItems": 1, "maxItems": 100 },
                "seal": { "type": "boolean" },
            },
        },
        "BulkJobView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "kind": { "type": "string", "enum": ["entries"] },
                "operation": { "type": "object" },
                "state": { "type": "string", "enum": ["accepting", "running", "done", "blocked", "cancelled"] },
                "hash": { "type": "string", "description": "`sha256:<hex>` — the job's `ETag`" },
                "items": { "$ref": "#/components/schemas/ItemsCount" },
                "created_by": { "type": "string", "format": "uuid" },
                "principal": { "type": "string", "enum": ["member", "token"] },
                "started_at": { "type": ["string", "null"], "format": "date-time" },
                "finished_at": { "type": ["string", "null"], "format": "date-time" },
                "error": { "type": ["string", "null"] },
                "created_at": { "type": "string", "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
            },
        },
        "BulkItemView": {
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "entry_id": { "type": "string", "format": "uuid" },
                "expected_revision_id": { "type": "string", "format": "uuid" },
                "input": { "type": "object" },
                "outcome": { "type": "string", "enum": ["pending", "applied", "conflict", "refused", "skipped"] },
                "problem": { "type": ["object", "null"], "description": "why the item didn't apply, as a refusal reports it" },
                "new_revision_id": { "type": ["string", "null"], "format": "uuid" },
                "applied_at": { "type": ["string", "null"], "format": "date-time" },
            },
        },
        "BulkAppendView": {
            "type": "object",
            "properties": {
                "job": { "$ref": "#/components/schemas/BulkJobView" },
                "appended": { "type": "integer" },
                "duplicates": { "type": "array", "items": { "type": "string" } },
                "sealed": { "type": "boolean" },
            },
        },
    })
}

/// Entry documents, revisions, queries, import/export.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn entry_parts() -> Value {
    json!({
        "EntryDocument": {
            "type": "object",
            "description": "the entry's content document, keyed by field name and validated against the collection's current definition; left-out fields get the type's defaults",
        },
        "QueryDocument": {
            "type": "object",
            "description": "the bounded query document (DX.1, D-103): `filters` (field, operator, value — only indexes the type declares), `sort`, `fields` (a selection that can only narrow), `limit`, `depth` (reference population levels). The canonical form hashes to `query_hash`.",
        },
        "ImportBody": {
            "description": "one JSON object per line, one JSON array of the same objects, or a Strapi API response's `{\"data\": […], \"meta\": {…}}`. Each item is a native content document, a Strapi entity (`{\"type\": \"api::…\", \"data\": {…}}`, `{\"version\":…,\"data\":{…}}`, `{id, attributes}` or a flat `documentId`/`id` document) or a Sanity document (`_id`, `_type`). At most 512 items.",
        },
        "ImportedView": {
            "type": "object",
            "properties": {
                "imported": { "type": "integer", "description": "items that created or updated an entry (`skipped` doesn't count)" },
                "outcomes": { "type": "array", "items": { "$ref": "#/components/schemas/ImportOutcome" } },
            },
        },
        "ImportPlanView": {
            "type": "object",
            "description": "what `?dry_run=true` answers: the plan, nothing written. A prediction, not a reservation — applying re-checks every item",
            "properties": {
                "dry_run": { "type": "boolean", "enum": [true] },
                "outcomes": { "type": "array", "items": { "$ref": "#/components/schemas/ImportOutcome" } },
            },
        },
        "ImportOutcome": {
            "type": "object",
            "description": "one item's place in the report",
            "properties": {
                "item": { "type": "string", "description": "its pointer: `/line/3` for JSONL, `/2` for a JSON array, `/data/0` for a wrapped list" },
                "status": { "type": "string", "enum": ["created", "updated", "skipped", "would_create", "would_update", "invalid"], "description": "`would_*` and `invalid` only in a plan" },
                "entry": { "type": "string", "format": "uuid", "description": "the entry the item writes or named" },
                "source": { "type": "string", "description": "the source's own id: a Sanity `_id`, a Strapi `id`/`documentId`" },
                "unmapped": { "type": "array", "items": { "type": "string" }, "description": "source keys set aside rather than imported" },
                "refusal": { "$ref": "#/components/schemas/ItemRefusal" },
                "reason": { "type": "string", "description": "why a `skipped` item was skipped" },
            },
            "required": ["item", "status"],
        },
        "ItemRefusal": {
            "type": "object",
            "description": "the code, pointer and message an `invalid` item fails with when applied",
            "properties": {
                "code": { "type": "string" },
                "pointer": { "type": "string" },
                "message": { "type": "string" },
            },
            "required": ["code", "pointer", "message"],
        },
        "JsonLines": {
            "type": "string",
            "description": "one JSON object per line, each an entry's content document",
        },
        "RevisionView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "base_revision_id": { "type": ["string", "null"], "format": "uuid" },
                "author_id": { "type": "string", "format": "uuid" },
                "created_at": { "type": "string", "format": "date-time" },
                "content_hash": { "type": "string", "description": "`sha256:<hex>`" },
                "title": { "type": "string" },
            },
        },
        "DraftView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
                "created_by": { "type": "string", "format": "uuid" },
                "updated_at": { "type": "string", "format": "date-time" },
                "revision": { "$ref": "#/components/schemas/RevisionView" },
                "content": { "$ref": "#/components/schemas/EntryDocument" },
                "hidden_fields": { "type": "array", "items": { "type": "string" }, "description": "fields the role may not read; present only when some were hidden" },
                "locked_fields": { "type": "array", "items": { "type": "string" }, "description": "fields the role may not write" },
                "published": { "$ref": "#/components/schemas/LiveView", "description": "the live revision and its generation; present when the read looked it up" },
                "referenced_by": { "type": "array", "items": { "$ref": "#/components/schemas/ReferrerView" }, "description": "entries naming this one, in collections the role reads" },
                "locale": { "type": ["string", "null"] },
                "localizations": { "type": "array", "items": { "$ref": "#/components/schemas/LocalizationView" } },
            },
        },
        "LocalizationView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "locale": { "type": ["string", "null"] },
            },
        },
        "ReferrerView": {
            "type": "object",
            "properties": {
                "entry_id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "field": { "type": "string" },
                "in_draft": { "type": "boolean" },
                "published": { "type": "boolean" },
            },
        },
        "EntrySummaryView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "created_by": { "type": "string", "format": "uuid" },
                "updated_at": { "type": "string", "format": "date-time" },
                "revision": { "$ref": "#/components/schemas/RevisionView" },
                "published": { "type": ["object", "null"], "$comment": "LiveView", "properties": {
                    "revision_id": { "type": "string", "format": "uuid" },
                    "generation": { "type": "integer" },
                } },
                "locale": { "type": ["string", "null"] },
            },
        },
        "LiveView": {
            "type": "object",
            "properties": {
                "revision_id": { "type": "string", "format": "uuid" },
                "generation": { "type": "integer" },
            },
        },
        "RevisionContentView": {
            "type": "object",
            "properties": {
                "revision": { "$ref": "#/components/schemas/RevisionView" },
                "content": { "$ref": "#/components/schemas/EntryDocument" },
            },
        },
        "DeletedView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "revision_id": { "type": "string", "format": "uuid", "description": "the tombstone's own revision" },
                "deleted_at": { "type": "string", "format": "date-time" },
                "deleted_by": { "type": "string", "format": "uuid" },
            },
        },
        "QueriedView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
                "created_by": { "type": "string", "format": "uuid" },
                "updated_at": { "type": "string", "format": "date-time" },
                "revision": { "$ref": "#/components/schemas/RevisionView" },
                "content": { "type": "object", "description": "narrowed to the role's fields, then to `fields`, then `depth`-populated" },
                "hidden_fields": { "type": "array", "items": { "type": "string" } },
                "published": { "type": ["object", "null"], "properties": {
                    "revision_id": { "type": "string", "format": "uuid" },
                    "generation": { "type": "integer" },
                } },
                "locale": { "type": ["string", "null"] },
            },
        },
        "QueryPage": {
            "type": "object",
            "required": ["data", "next_cursor", "applied"],
            "properties": {
                "data": { "type": "array", "items": { "$ref": "#/components/schemas/QueriedView" } },
                "next_cursor": { "type": ["string", "null"] },
                "applied": { "$ref": "#/components/schemas/AppliedQuery" },
            },
        },
        "AppliedQuery": {
            "type": "object",
            "properties": {
                "query": { "$ref": "#/components/schemas/QueryDocument", "description": "the canonical document" },
                "query_hash": { "type": "string", "description": "`sha256:<hex>` of it" },
            },
        },
        "GraphQLRequest": {
            "type": "object",
            "required": ["query"],
            "description": "GraphQL over the query language (DX.10, D-115): a document at most 8 KiB; the schema is generated per collection, so introspection names it. `entries(where, orderBy, first, after)` runs the canonical document and `entry(id)` one draft; mutations stay REST.",
            "properties": {
                "query": { "type": "string", "description": "the GraphQL document; `query` is required, `mutation` and `subscription` are refused" },
                "operationName": { "type": "string", "description": "which operation runs, when the document names several" },
                "variables": { "type": "object", "description": "variable values, an object" },
            },
            "additionalProperties": false,
        },
        "GraphQLResponse": {
            "type": "object",
            "description": "GraphQL's own `{\"data\": …, \"errors\": […]}` — a refusal surfaces as an error on the field that ran, `extensions.code`/`extensions.pointer` carrying the query language's `code` and JSON Pointer (D-115)",
            "properties": {
                "data": { "type": "object", "description": "`entries` (a `{T}Connection`: `nodes`, `pageInfo {endCursor, hasNextPage}`, `applied {query, queryHash}`) and `entry` results" },
                "errors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "message": { "type": "string" },
                            "locations": { "type": "array" },
                            "path": { "type": "array" },
                            "extensions": { "type": "object", "description": "`code` and `pointer`, the refusal's own" },
                        },
                    },
                },
            },
        },
    })
}

/// Comments and editorial tasks.
fn collaboration_parts() -> Value {
    json!({
        "NewComment": {
            "type": "object",
            "required": ["revision", "body"],
            "properties": {
                "revision": { "type": "string", "format": "uuid", "description": "the immutable revision the comment anchors" },
                "field": { "type": "string", "description": "an optional field path" },
                "body": { "type": "string", "minLength": 1, "maxLength": 8000 },
            },
        },
        "CommentView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "entry_id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "revision_id": { "type": "string", "format": "uuid" },
                "field": { "type": ["string", "null"] },
                "body": { "type": "string", "description": "verbatim; the client escapes it" },
                "author_id": { "type": "string", "format": "uuid" },
                "resolved_by": { "type": ["string", "null"], "format": "uuid" },
                "resolved_at": { "type": ["string", "null"], "format": "date-time" },
                "created_at": { "type": "string", "format": "date-time" },
                "etag": { "type": "string", "description": "the `ETag` `If-Match` binds, as the header gives it" },
            },
        },
        "NewTask": {
            "type": "object",
            "required": ["entry", "title"],
            "properties": {
                "entry": { "type": "string", "format": "uuid" },
                "title": { "type": "string", "minLength": 1, "maxLength": 200 },
                "revision": { "type": "string", "format": "uuid" },
                "changeset": { "type": "string", "format": "uuid" },
                "assignee": { "type": "string", "format": "uuid" },
            },
        },
        "TaskUpdate": {
            "type": "object",
            "properties": {
                "title": { "type": "string", "minLength": 1, "maxLength": 200 },
                "assignee": { "type": ["string", "null"], "format": "uuid", "description": "absent leaves it; `null` unassigns" },
                "status": { "type": "string", "enum": ["open", "done", "cancelled"] },
            },
        },
        "TaskView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "entry_id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "revision_id": { "type": ["string", "null"], "format": "uuid" },
                "changeset_id": { "type": ["string", "null"], "format": "uuid" },
                "title": { "type": "string" },
                "assignee_id": { "type": ["string", "null"], "format": "uuid" },
                "status": { "type": "string", "enum": ["open", "done", "cancelled"] },
                "created_by": { "type": "string", "format": "uuid" },
                "done_by": { "type": ["string", "null"], "format": "uuid" },
                "done_at": { "type": ["string", "null"], "format": "date-time" },
                "created_at": { "type": "string", "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
                "etag": { "type": "string" },
            },
        },
    })
}

/// Changesets, reviews, stages, schedules, releases, receipts, destinations, jobs.
#[expect(
    clippy::too_many_lines,
    reason = "a JSON Schema literal reads top to bottom; splitting scatters one component map"
)]
fn publishing_parts() -> Value {
    json!({
        "NewChangeset": {
            "type": "object",
            "required": ["title"],
            "additionalProperties": false,
            "properties": {
                "title": { "type": "string" },
                "entries": { "type": "array", "items": { "type": "string", "format": "uuid" }, "maxItems": 100 },
                "unpublish": { "type": "array", "items": { "type": "string", "format": "uuid" } },
            },
        },
        "NewRelease": {
            "type": "object",
            "required": ["base_generation"],
            "additionalProperties": false,
            "properties": { "base_generation": { "type": "integer", "minimum": 0 } },
        },
        "NewReview": {
            "type": "object",
            "required": ["decision"],
            "additionalProperties": false,
            "properties": {
                "decision": { "type": "string", "enum": ["approve", "request_changes"] },
                "comment": { "type": "string", "maxLength": 2000 },
                "stage": { "type": "string", "description": "the stage's key where the environment configures a list" },
            },
        },
        "NewStages": {
            "type": "object",
            "required": ["stages"],
            "additionalProperties": false,
            "properties": {
                "stages": { "type": "array", "maxItems": 20, "items": {
                    "type": "object",
                    "required": ["key", "name"],
                    "properties": {
                        "key": { "type": "string" },
                        "name": { "type": "string" },
                        "different_reviewer": { "type": "boolean" },
                    },
                } },
            },
        },
        "NewSchedule": {
            "type": "object",
            "required": ["local_time", "time_zone"],
            "additionalProperties": false,
            "properties": {
                "local_time": { "type": "string", "description": "wall-clock time, no offset" },
                "time_zone": { "type": "string", "description": "an IANA zone, such as `Europe/Berlin`" },
                "choice": { "type": "string", "enum": ["earlier", "later"], "description": "which of the two instants an ambiguous or skipped local time means" },
            },
        },
        "ChangesetItemView": {
            "type": "object",
            "properties": {
                "kind": { "type": "string" },
                "entry_id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "revision_id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "content_hash": { "type": "string" },
                "schema_version": { "type": "integer" },
                "current_revision_id": { "type": ["string", "null"], "format": "uuid" },
                "edited": { "type": "boolean", "description": "the draft moved since it was pinned" },
                "published_revision_id": { "type": ["string", "null"], "format": "uuid" },
                "created_by": { "type": ["string", "null"], "format": "uuid" },
                "entry_deleted": { "type": "boolean" },
            },
        },
        "ReviewView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "reviewer_id": { "type": "string", "format": "uuid" },
                "reviewer_name": { "type": ["string", "null"] },
                "decision": { "type": "string", "enum": ["approve", "request_changes"] },
                "stage": { "type": ["string", "null"] },
                "changeset_hash": { "type": "string", "description": "the hash this review bound" },
                "current": { "type": "boolean", "description": "whether the hash it bound is still the changeset's" },
                "comment": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "StageView": {
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "name": { "type": "string" },
                "different_reviewer": { "type": "boolean" },
            },
        },
        "StageStateView": {
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "name": { "type": "string" },
                "different_reviewer": { "type": "boolean" },
                "state": { "type": "string", "enum": ["pending", "changes_requested", "approved", "approval_expired"] },
                "by": { "type": ["string", "null"], "format": "uuid" },
                "by_name": { "type": ["string", "null"] },
                "at": { "type": ["string", "null"], "format": "date-time" },
            },
        },
        "StageListView": {
            "type": "object",
            "properties": {
                "stages": { "type": "array", "items": { "$ref": "#/components/schemas/StageView" } },
            },
        },
        "StatusView": {
            "type": "object",
            "properties": {
                "state": { "type": "string" },
                "generation": { "type": ["integer", "null"] },
                "by": { "type": ["string", "null"] },
                "self_review": { "type": "boolean" },
                "edited_items": { "type": "integer" },
            },
        },
        "ChangesetView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "hash": { "type": "string", "description": "`sha256:<hex>` — its `ETag`, and what reviews bind" },
                "status": { "$ref": "#/components/schemas/StatusView" },
                "created_by": { "type": "string", "format": "uuid" },
                "created_at": { "type": "string", "format": "date-time" },
                "submitted_by": { "type": ["string", "null"], "format": "uuid" },
                "submitted_at": { "type": ["string", "null"], "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
                "rollback_generation": { "type": ["integer", "null"] },
                "items": { "type": "array", "items": { "$ref": "#/components/schemas/ChangesetItemView" } },
                "reviews": { "type": "array", "items": { "$ref": "#/components/schemas/ReviewView" } },
                "stages": { "type": "array", "items": { "$ref": "#/components/schemas/StageStateView" } },
            },
        },
        "ChangesetSummaryView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "hash": { "type": "string" },
                "status": { "$ref": "#/components/schemas/StatusView" },
                "items": { "type": "integer" },
                "unpublish": { "type": "integer" },
                "created_by": { "type": "string", "format": "uuid" },
                "submitted_by": { "type": ["string", "null"], "format": "uuid" },
                "submitted_at": { "type": ["string", "null"], "format": "date-time" },
                "updated_at": { "type": "string", "format": "date-time" },
                "rollback_generation": { "type": ["integer", "null"] },
            },
        },
        "PublishedView": {
            "type": "object",
            "properties": {
                "generation": { "type": "integer" },
                "epoch": { "type": "integer" },
                "changeset_id": { "type": "string", "format": "uuid" },
                "changeset_hash": { "type": "string" },
                "entries_changed": { "type": "integer" },
                "entries_unpublished": { "type": "integer" },
            },
        },
        "ReleaseView": {
            "type": "object",
            "properties": {
                "generation": { "type": "integer" },
                "epoch": { "type": "integer" },
                "changeset_id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "changeset_hash": { "type": "string" },
                "published_by": { "type": "string", "format": "uuid" },
                "published_by_name": { "type": ["string", "null"] },
                "published_at": { "type": "string", "format": "date-time" },
                "entries": { "type": "integer" },
                "unpublished": { "type": "integer" },
                "purge": { "type": "string", "description": "the purge job's state for this release" },
                "label": { "type": "string" },
                "locations": { "type": "integer" },
            },
        },
        "ReceiptView": {
            "type": "object",
            "description": "a release plus its deliveries and observations — the publish receipt",
            "allOf": [{ "$ref": "#/components/schemas/ReleaseView" }],
            "properties": {
                "deliveries": { "type": "array", "items": { "$ref": "#/components/schemas/DeliveryView" } },
                "observations": { "type": "array", "items": { "$ref": "#/components/schemas/SeenView" } },
            },
        },
        "DeliveryView": {
            "type": "object",
            "properties": {
                "destination_id": { "type": "string", "format": "uuid" },
                "name": { "type": "string" },
                "kind": { "type": "string" },
                "location": { "type": ["string", "null"] },
                "state": { "type": "string" },
                "covered_by": { "type": ["integer", "null"] },
                "status": { "type": ["string", "null"] },
                "attempts": { "type": ["integer", "null"] },
                "last_error": { "type": ["string", "null"] },
                "finished_at": { "type": ["string", "null"], "format": "date-time" },
            },
        },
        "SeenView": {
            "type": "object",
            "properties": {
                "destination_id": { "type": "string", "format": "uuid" },
                "location": { "type": "string" },
                "observed_generation": { "type": ["integer", "null"] },
                "http_status": { "type": ["integer", "null"] },
                "body_hash": { "type": ["string", "null"] },
                "error": { "type": ["string", "null"] },
                "observed_at": { "type": "string", "format": "date-time" },
            },
        },
        "ScheduleView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "changeset_id": { "type": "string", "format": "uuid" },
                "title": { "type": "string" },
                "changeset_hash": { "type": "string" },
                "local_time": { "type": "string" },
                "time_zone": { "type": "string" },
                "choice": { "type": "string" },
                "run_at": { "type": "string", "format": "date-time" },
                "state": { "type": "string", "enum": ["pending", "published", "blocked", "cancelled"] },
                "reason": { "type": "string" },
                "generation": { "type": "integer" },
                "started_at": { "type": "string", "format": "date-time" },
                "finished_at": { "type": "string", "format": "date-time" },
                "delayed": { "type": "boolean", "description": "it ran late, truthfully — never reported on time" },
                "scheduled_by": { "type": "string", "format": "uuid" },
                "scheduled_by_name": { "type": "string" },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "NewDestination": {
            "type": "object",
            "required": ["name", "url"],
            "additionalProperties": false,
            "properties": {
                "kind": { "type": "string", "enum": ["webhook"], "default": "webhook" },
                "name": { "type": "string" },
                "url": { "type": "string", "format": "uri", "description": "https to a public address" },
                "location": { "type": "string", "description": "a probe location label" },
            },
        },
        "DestinationEnabled": {
            "type": "object",
            "required": ["enabled"],
            "additionalProperties": false,
            "properties": { "enabled": { "type": "boolean" } },
        },
        "DestinationView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "kind": { "type": "string" },
                "name": { "type": "string" },
                "url": { "type": "string" },
                "enabled": { "type": "boolean" },
                "location": { "type": ["string", "null"] },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "JobView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "destination_id": { "type": "string", "format": "uuid" },
                "destination_name": { "type": "string" },
                "generation": { "type": "integer" },
                "status": { "type": "string" },
                "attempts": { "type": "integer" },
                "last_http_status": { "type": ["integer", "null"] },
                "last_error": { "type": ["string", "null"] },
                "created_at": { "type": "string", "format": "date-time" },
                "finished_at": { "type": ["string", "null"], "format": "date-time" },
            },
        },
    })
}

/// Assets and the public delivery surface.
fn delivery_parts() -> Value {
    json!({
        "AssetView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["quarantined", "live", "rejected", "deleted"] },
                "file_name": { "type": "string" },
                "declared_type": { "type": "string" },
                "detected_type": { "type": ["string", "null"] },
                "sha256": { "type": ["string", "null"], "description": "the current version's digest; `/media/v1/…/{asset}/{sha256}` pins it (D-136)" },
                "size_bytes": { "type": "integer" },
                "width": { "type": ["integer", "null"] },
                "height": { "type": ["integer", "null"] },
                "rejected_reason": { "type": ["string", "null"] },
                "derivatives": { "type": "array", "items": { "type": "object" }, "description": "the renditions the worker wrote" },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "AssetVersionView": {
            "type": "object",
            "properties": {
                "seq": { "type": "integer" },
                "declared_type": { "type": "string" },
                "size_bytes": { "type": "integer" },
                "detected_type": { "type": "string" },
                "sha256": { "type": "string", "description": "the content digest the pinned URL names" },
                "width": { "type": "integer" },
                "height": { "type": "integer" },
                "derivatives": { "type": "array", "items": { "type": "string" } },
                "created_at": { "type": "string", "format": "date-time" },
            },
        },
        "AssetDetailView": {
            "type": "object",
            "properties": {
                "data": { "$ref": "#/components/schemas/AssetView" },
                "versions": { "type": "array", "items": { "$ref": "#/components/schemas/AssetVersionView" } },
            },
        },
        "AssetUploadView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "state": { "type": "string", "enum": ["quarantined"] },
                "file_name": { "type": "string" },
                "declared_type": { "type": "string" },
                "size_bytes": { "type": "integer" },
            },
        },
        "UploadBody": {
            "type": "string",
            "format": "binary",
            "description": "the file's bytes; its name in `X-Colophon-File-Name`, its claimed type in `Content-Type`",
        },
        "PublishedEntryView": {
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "revision_id": { "type": "string", "format": "uuid" },
                "collection": { "type": "string" },
                "title": { "type": "string" },
                "slug": { "type": "string" },
                "content": { "$ref": "#/components/schemas/EntryDocument" },
                "content_hash": { "type": "string" },
                "published_in": { "type": "integer", "description": "the generation this revision went live in" },
            },
        },
        "DeliveryQueryPage": {
            "type": "object",
            "required": ["data", "next_cursor", "applied"],
            "properties": {
                "data": { "type": "array", "items": { "$ref": "#/components/schemas/PublishedEntryView" } },
                "next_cursor": { "type": ["string", "null"] },
                "applied": { "$ref": "#/components/schemas/AppliedQuery" },
            },
        },
    })
}

fn mcp_parts() -> Value {
    json!({
        "McpMessage": {
            "type": "object",
            "description": "one JSON-RPC 2.0 message under MCP protocol 2025-11-25: a request, a notification or a response — never a batch",
            "required": ["jsonrpc"],
            "properties": {
                "jsonrpc": { "type": "string", "enum": ["2.0"] },
                "id": { "description": "a string or number on requests and their responses; absent on notifications" },
                "method": {
                    "type": "string",
                    "description": "`initialize`, `ping`, `tools/list`, `tools/call`, or a `notifications/*` notification",
                },
                "params": { "type": "object" },
                "result": { "description": "the answer, on a client's own response" },
                "error": { "type": "object", "description": "`{code, message, data?}`, on a client's own refusal" },
            },
        },
        "McpResponse": {
            "type": "object",
            "description": "one JSON-RPC 2.0 response message: `result` on success or `error` on failure, never both",
            "required": ["jsonrpc", "id"],
            "properties": {
                "jsonrpc": { "type": "string", "enum": ["2.0"] },
                "id": { "description": "the request's id, echoed" },
                "result": {
                    "description": "the handshake on `initialize`, `{}` on `ping`, `{tools}` on `tools/list`, and `{content, structuredContent, isError}` on `tools/call` — `isError` carries the API's own refusal",
                },
                "error": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "description": "JSON-RPC's own, plus -32001 API-unavailable, -32002 not-initialized, -32800 cancelled" },
                        "message": { "type": "string" },
                        "data": { "description": "the API's error code, status and request id when it refused" },
                    },
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `$ref` the document emits must name a component it declares.
    #[test]
    fn every_ref_resolves() {
        let document = document(&BuildInfo::unstamped("test"));
        let declared: std::collections::HashSet<String> = document["components"]["schemas"]
            .as_object()
            .map(|schemas| schemas.keys().cloned().collect())
            .unwrap_or_default();
        let mut missing = Vec::new();
        collect_refs(&document, &mut |name| {
            if !declared.contains(name) {
                missing.push(name.to_owned());
            }
        });
        assert!(missing.is_empty(), "unresolved refs: {missing:?}");
    }

    fn collect_refs<'a>(value: &'a Value, found: &mut impl FnMut(&'a str)) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if key == "$ref"
                        && let Some(name) = value
                            .as_str()
                            .and_then(|r| r.strip_prefix("#/components/schemas/"))
                    {
                        found(name);
                    }
                    collect_refs(value, found);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_refs(item, found);
                }
            }
            _ => {}
        }
    }

    /// Every operation id is unique and non-empty; every operation has a summary.
    #[test]
    fn operation_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for route in TABLE {
            let path = route.path();
            let mut verbs = std::collections::HashSet::new();
            for op in route.ops {
                assert!(!op.doc.id.is_empty(), "{path} has an unnamed op");
                assert!(ids.insert(op.doc.id), "duplicate operationId {}", op.doc.id);
                assert!(!op.doc.summary.is_empty(), "{path} has an unsummarized op");
                assert!(!op.doc.tag.is_empty(), "{path} has an untagged op");
                assert!(
                    verbs.insert(op.verb),
                    "{path} declares {} twice",
                    op.verb.name()
                );
            }
        }
    }

    /// Path params the document emits are exactly the placeholders the path declares.
    #[test]
    fn path_params_are_derived_from_the_path() {
        for route in TABLE {
            let derived = path_params(&route.path());
            // No duplicate placeholder names on one path.
            let names: std::collections::HashSet<_> = derived.iter().map(|(n, _)| n).collect();
            assert_eq!(
                names.len(),
                derived.len(),
                "{} has a repeated placeholder",
                route.path()
            );
        }
    }

    /// The document is a well-formed `OpenAPI` 3.1 value and covers every table route.
    #[test]
    fn the_document_covers_the_table() {
        let document = document(&BuildInfo::unstamped("test"));
        assert_eq!(document["openapi"], "3.1.0");
        let paths = document["paths"].as_object().expect("paths");
        for route in TABLE {
            let path = route.path();
            let item = paths
                .get(&path)
                .unwrap_or_else(|| panic!("the document misses {path}"));
            for op in route.ops {
                assert!(
                    item.get(op.verb.name()).is_some(),
                    "the document misses {} {}",
                    op.verb.name().to_uppercase(),
                    path
                );
            }
        }
        assert!(
            paths.contains_key(SELF_PATH),
            "the document describes itself"
        );
    }

    /// Every served verb has a documented operation and vice versa — the half of the drift
    /// check that lives with the table. The other half (the built router answers exactly
    /// these methods) is probed over HTTP by [`the_router_matches_the_table`].
    #[test]
    fn every_verb_is_declared_once() {
        for route in TABLE {
            let mut seen = std::collections::HashSet::new();
            for op in route.ops {
                assert!(seen.insert(op.verb.name()));
            }
        }
    }

    /// Fill a path's placeholders with stand-ins so the probe can request it.
    fn concrete(path: &str) -> String {
        let mut concrete = path.to_owned();
        for (name, kind) in path_params(path) {
            let value = match kind {
                Kind::Uuid => "00000000-0000-0000-0000-000000000000",
                Kind::Int | Kind::Bool | Kind::Cursor | Kind::Str => "x",
            };
            concrete = concrete.replace(&format!("{{{name}}}"), value);
        }
        concrete
    }

    const fn method(verb: Verb) -> axum::http::Method {
        match verb {
            Verb::Get => axum::http::Method::GET,
            Verb::Post => axum::http::Method::POST,
            Verb::Put => axum::http::Method::PUT,
            Verb::Patch => axum::http::Method::PATCH,
            Verb::Delete => axum::http::Method::DELETE,
        }
    }

    /// The drift test D-110 promises: every (path, method) the table — and therefore the
    /// document — declares is answered by the built router, and every verb the document does
    /// not declare is refused with 405. A route added to the router outside the table is the
    /// one drift this cannot see; `routes()` is one loop over [`TABLE`] precisely so that
    /// cannot happen quietly.
    #[tokio::test]
    async fn the_router_matches_the_table() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt as _;

        let state = crate::AppState::without_services(crate::Readiness::new("all", true));
        let app = crate::routes(state);
        let mut operations = 0;
        for route in TABLE {
            let uri = concrete(&route.path());
            let declared: Vec<Verb> = route.ops.iter().map(|op| op.verb).collect();
            for verb in Verb::ALL {
                let reply = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method(verb))
                            .uri(&uri)
                            .body(Body::empty())
                            .expect("a request"),
                    )
                    .await
                    .expect("the router answers");
                if declared.contains(&verb) {
                    operations += 1;
                    assert_ne!(
                        reply.status(),
                        StatusCode::METHOD_NOT_ALLOWED,
                        "{uri} must serve {}: the document declares it",
                        verb.name()
                    );
                    assert_ne!(
                        reply.status(),
                        StatusCode::NOT_FOUND,
                        "{uri} must serve {}: the document declares it",
                        verb.name()
                    );
                } else {
                    assert_eq!(
                        reply.status(),
                        StatusCode::METHOD_NOT_ALLOWED,
                        "{uri} must not serve {}: the document does not declare it",
                        verb.name()
                    );
                    assert!(
                        reply.headers().contains_key(header::ALLOW),
                        "a 405 names what it does allow"
                    );
                }
            }
        }
        assert!(operations > 100, "the probe should cover every operation");
    }

    /// `GET /openapi.json` answers the document, `ETag`'d and cacheable, to anyone.
    #[tokio::test]
    async fn the_document_is_served() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt as _;
        use tower::ServiceExt as _;

        let state = crate::AppState::without_services(crate::Readiness::new("all", true));
        let app = crate::routes(state);
        let reply = app
            .clone()
            .oneshot(
                Request::get(SELF_PATH)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        assert_eq!(reply.status(), StatusCode::OK);
        assert_eq!(reply.headers()[header::CONTENT_TYPE], "application/json");
        let etag = reply.headers()[header::ETAG]
            .to_str()
            .expect("an ASCII ETag")
            .to_owned();
        let body = reply
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        let document: Value = serde_json::from_slice(&body).expect("the document parses");
        assert_eq!(document["openapi"], "3.1.0");
        assert!(
            document["paths"].as_object().expect("paths").len() > TABLE.len(),
            "the served document covers the table and itself"
        );

        // Conditional: the same ETag gets a 304.
        let again = app
            .oneshot(
                Request::get(SELF_PATH)
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    }

    /// The committed `docs/api/openapi.json` is this document byte for byte, so what the
    /// server serves and what the repository publishes can never differ (D-110). Regenerate
    /// it with `COLOPHON_UPDATE_OPENAPI=1 cargo test -p colophon-api --lib openapi`.
    #[test]
    fn the_committed_document_is_current() {
        let document = document(&BuildInfo::unstamped(env!("CARGO_PKG_VERSION")));
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&document).expect("the document serializes")
        );
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/api/openapi.json");
        if std::env::var_os("COLOPHON_UPDATE_OPENAPI").is_some() {
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("docs/api");
            std::fs::write(&path, &rendered).expect("the artifact writes");
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "docs/api/openapi.json is unreadable: {error}; regenerate it with \
                 COLOPHON_UPDATE_OPENAPI=1 cargo test -p colophon-api --lib openapi"
            )
        });
        assert_eq!(
            committed, rendered,
            "docs/api/openapi.json is stale; regenerate it with \
             COLOPHON_UPDATE_OPENAPI=1 cargo test -p colophon-api --lib openapi"
        );
    }
}
