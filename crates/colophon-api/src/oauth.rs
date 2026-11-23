//! OAuth 2.1 for the MCP endpoint (D-246; the MCP authorization spec of 2025-11-25): an agent
//! connects by `/mcp` alone. It finds this server from `/mcp`'s 401 (`WWW-Authenticate` names the
//! protected-resource metadata, RFC 9728), reads the authorization server's metadata (RFC 8414),
//! registers itself (RFC 7591: public clients, exact redirect URIs), sends its person to
//! `/oauth/authorize` (PKCE S256 required, RFC 7636; `resource` the `/mcp` URL, RFC 8707), and
//! once they approve it in the Studio for one of their projects, trades the single-use code for an
//! access token (an hour) and a refresh token (thirty days, rotated, a replay revokes the grant).
//!
//! An access token acts as its person, with their role, in that one project, and only through
//! `/mcp` (session.rs): anywhere else it is refused. Everything a tool does is still decided by
//! the API under that person's permissions. These routes are a protocol surface beside the
//! documented API (openapi.rs), mounted by `routes()`.

use axum::{
    Json, Router,
    extract::{Path, Query, RawQuery, State, rejection::FormRejection, rejection::JsonRejection},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use colophon_auth::tokens;
use colophon_db::{oauth as store, projects};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{Credential, SignedIn, services},
    shape::Data,
};

/// What an OAuth access token's secret starts with: how `Authorization: Bearer` tells it from a
/// person's or a project's token, and how a secret scanner recognises one that leaked.
pub(crate) const ACCESS_PREFIX: &str = "colophon_oauth_";
/// A refresh token's prefix.
pub(crate) const REFRESH_PREFIX: &str = "colophon_oauthr_";
/// An access token's life, in seconds (the database's `interval '1 hour'`).
const ACCESS_SECONDS: u64 = 3600;

/// The OAuth routes, for `routes()` to mount beside the API's table.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(authorization_server))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize))
        .route("/oauth/requests/{id}", get(request).post(answer))
        .route("/oauth/token", post(token))
        .route("/oauth/revoke", post(revoke))
        .route("/api/v1/me/connected-apps", get(connected))
        .route("/api/v1/me/connected-apps/{id}", delete(disconnect))
}

/// This server's public origin (the first trusted origin): the issuer, and the base of every URL
/// the metadata names.
fn origin(state: &AppState) -> Result<String, ApiError> {
    let services = services(state)?;
    services
        .trusted_origins
        .first()
        .cloned()
        .ok_or_else(|| ApiError::internal(std::io::Error::other("no public origin configured")))
}

/// The protected resource: `/mcp`, at this origin.
pub(crate) fn resource(origin: &str) -> String {
    format!("{origin}/mcp")
}

/// `WWW-Authenticate` for a request to `/mcp` without a valid credential (RFC 9728 §5.1).
pub(crate) fn challenge(state: &AppState) -> Option<HeaderValue> {
    let origin = origin(state).ok()?;
    HeaderValue::from_str(&format!(
        "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource\""
    ))
    .ok()
}

/// `GET /.well-known/oauth-protected-resource` (RFC 9728).
async fn protected_resource(State(state): State<AppState>) -> Result<Response, ApiError> {
    let origin = origin(&state)?;
    Ok(Json(json!({
        "resource": resource(&origin),
        "authorization_servers": [origin],
        "bearer_methods_supported": ["header"],
        "resource_name": "Colophon",
    }))
    .into_response())
}

/// `GET /.well-known/oauth-authorization-server` (RFC 8414).
async fn authorization_server(State(state): State<AppState>) -> Result<Response, ApiError> {
    let origin = origin(&state)?;
    Ok(Json(json!({
        "issuer": origin,
        "authorization_endpoint": format!("{origin}/oauth/authorize"),
        "token_endpoint": format!("{origin}/oauth/token"),
        "registration_endpoint": format!("{origin}/oauth/register"),
        "revocation_endpoint": format!("{origin}/oauth/revoke"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "revocation_endpoint_auth_methods_supported": ["none"],
        "authorization_response_iss_parameter_supported": true,
        "scopes_supported": [],
    }))
    .into_response())
}

/// An error in RFC 6749 §5.2's shape (`error`, `error_description`), never cached.
fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let mut response = (
        status,
        Json(json!({ "error": error, "error_description": description })),
    )
        .into_response();
    no_store(&mut response);
    response
}

fn no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

/// Whether `uri` may be a redirect URI: exact and absolute, no fragment or credentials; `https`
/// anywhere, `http` only to a loopback host (RFC 8252 §7.3), or a native app's private-use scheme
/// (§7.1: `cursor://…`, `vscode://…`, `com.example.app:/…`): PKCE is what keeps a code another app
/// intercepts there useless. Never a scheme a browser runs or reads locally.
pub(crate) fn redirect_allowed(uri: &str) -> bool {
    if uri.len() > 2000 {
        return false;
    }
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some_and(|host| !host.is_empty()),
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        scheme => !matches!(
            scheme,
            "javascript" | "data" | "file" | "vbscript" | "blob" | "about" | "filesystem" | "ftp" | "ws" | "wss"
        ),
    }
}

/// RFC 7591's registration request: only what this server acts on is read, the rest ignored.
#[derive(Deserialize)]
struct Registration {
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    redirect_uris: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    grant_types: Option<Vec<String>>,
    #[serde(default)]
    response_types: Option<Vec<String>>,
}

/// `POST /oauth/register`: a public client registers itself (RFC 7591). Rate-limited per address
/// with the password work (admission.rs); the table is bounded (`store::MAX_CLIENTS`).
async fn register(
    State(state): State<AppState>,
    body: Result<Json<Registration>, JsonRejection>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let Ok(Json(body)) = body else {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "the body is not a JSON registration"));
    };
    if body.redirect_uris.is_empty() || body.redirect_uris.len() > 5 {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_redirect_uri", "give one to five redirect_uris"));
    }
    if let Some(bad) = body.redirect_uris.iter().find(|uri| !redirect_allowed(uri)) {
        return Ok(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            &format!("not an allowed redirect URI (https, http on a loopback host, or a native app's scheme): {bad}"),
        ));
    }
    if body.token_endpoint_auth_method.as_deref().is_some_and(|method| method != "none") {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "only public clients (token_endpoint_auth_method \"none\") are supported"));
    }
    if body
        .grant_types
        .as_ref()
        .is_some_and(|types| types.iter().any(|t| t != "authorization_code" && t != "refresh_token"))
    {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "grant_types may be authorization_code and refresh_token"));
    }
    if body.response_types.as_ref().is_some_and(|types| types.iter().any(|t| t != "code")) {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "response_types may be code"));
    }
    // The name is the client's own claim, shown as unverified: plain text, one line, bounded.
    let name: String = body
        .client_name
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(100)
        .collect::<String>()
        .trim()
        .to_owned();
    let name = if name.is_empty() { "Unnamed app".to_owned() } else { name };
    let id = Uuid::now_v7();
    if !store::register_client(&services.pool, id, &name, &body.redirect_uris)
        .await
        .map_err(ApiError::from_db)?
    {
        return Err(ApiError::Unavailable("this server holds as many OAuth clients as it can; try again later"));
    }
    tracing::info!(client = %id, "OAuth client registered");
    let mut response = (
        StatusCode::CREATED,
        Json(json!({
            "client_id": id,
            "client_name": name,
            "redirect_uris": body.redirect_uris,
            "token_endpoint_auth_method": "none",
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "client_id_issued_at": time::OffsetDateTime::now_utc().unix_timestamp(),
        })),
    )
        .into_response();
    no_store(&mut response);
    Ok(response)
}

/// `/oauth/authorize`'s query (RFC 6749 §4.1.1, RFC 7636, RFC 8707).
#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    resource: Option<String>,
}

/// A page that says what is wrong with an authorization request whose client or redirect URI
/// can't be trusted: never a redirect to an address nobody registered (RFC 6749 §4.1.2.1).
fn refused_page(why: &str) -> Response {
    let why = why.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut response = (
        StatusCode::BAD_REQUEST,
        Html(format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Can't connect this app</title>\
             <meta name=\"viewport\" content=\"width=device-width\"></head><body style=\"font: 16px system-ui, sans-serif; max-width: 36rem; margin: 3rem auto; padding: 0 1rem\">\
             <h1 style=\"font-size: 1.4rem\">Can't connect this app</h1><p>{why}</p>\
             <p>Nothing was shared. Go back to the app and try connecting again.</p></body></html>"
        )),
    )
        .into_response();
    no_store(&mut response);
    response
}

/// `redirect_uri` with `pairs` added to its query.
fn with_query(redirect_uri: &str, pairs: &[(&str, &str)]) -> Result<String, ApiError> {
    let mut url = url::Url::parse(redirect_uri).map_err(ApiError::internal)?;
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
    }
    Ok(url.into())
}

/// `GET /oauth/authorize`: checks the client and its redirect URI first (a failure there is a
/// page, never a redirect), then the rest (a failure is the error sent back to the client), then
/// hands a signed-in person to the Studio's consent page, or someone signed out to sign-in first.
async fn authorize(
    State(state): State<AppState>,
    caller: Result<SignedIn, ApiError>,
    RawQuery(raw): RawQuery,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let origin = origin(&state)?;
    let Some(client_id) = query.client_id.as_deref().and_then(|id| Uuid::parse_str(id).ok()) else {
        return Ok(refused_page("The request names no app this server knows (client_id)."));
    };
    let Some(client) = store::client(&services.pool, client_id).await.map_err(ApiError::from_db)? else {
        return Ok(refused_page("The request names no app this server knows (client_id)."));
    };
    let redirect_uri = match query.redirect_uri.as_deref() {
        Some(uri) if client.redirect_uris.iter().any(|known| known == uri) => uri.to_owned(),
        None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
        _ => return Ok(refused_page("The app asked to be answered at an address it didn't register (redirect_uri).")),
    };
    // From here the client is known and the address is its own: errors go back to it.
    let state_param = query.state.clone();
    let fail = |error: &str, description: &str| -> Result<Response, ApiError> {
        let mut pairs = vec![("error", error), ("error_description", description), ("iss", origin.as_str())];
        if let Some(state) = state_param.as_deref() {
            pairs.push(("state", state));
        }
        Ok(Redirect::to(&with_query(&redirect_uri, &pairs)?).into_response())
    };
    if query.response_type.as_deref() != Some("code") {
        return fail("unsupported_response_type", "response_type must be code");
    }
    let challenge = match (query.code_challenge.as_deref(), query.code_challenge_method.as_deref()) {
        (Some(challenge), Some("S256"))
            if challenge.len() == 43
                && challenge.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') =>
        {
            challenge.to_owned()
        }
        _ => return fail("invalid_request", "PKCE is required: code_challenge with code_challenge_method S256"),
    };
    if query.resource.as_deref().is_some_and(|asked| asked.trim_end_matches('/') != resource(&origin)) {
        return fail("invalid_target", "resource must be this server's /mcp");
    }
    if query.state.as_deref().is_some_and(|state| state.len() > 500) {
        return fail("invalid_request", "state is too long");
    }
    match caller {
        Ok(caller) if caller.credential == Credential::Session => {
            let id = store::start_request(
                &services.pool,
                client.id,
                &redirect_uri,
                query.state.as_deref(),
                &challenge,
                caller.identity,
            )
            .await
            .map_err(ApiError::from_db)?;
            Ok(Redirect::to(&format!("/studio/connect/{id}")).into_response())
        }
        // Signed out (or not by a browser session): sign in, then this same request again. The
        // return address is this server's own path, built here, never a caller's.
        _ => {
            let back = format!("/oauth/authorize?{}", raw.unwrap_or_default());
            let target = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("return_to", &back)
                .finish();
            Ok(Redirect::to(&format!("/studio/signin?{target}")).into_response())
        }
    }
}

/// The person answering must be signed in to the Studio itself.
fn browser(caller: &SignedIn) -> Result<(), ApiError> {
    if caller.credential == Credential::Session {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "browser_session_required",
            "connect an app from a browser where you are signed in",
        ))
    }
}

#[derive(Serialize)]
struct RequestView {
    client_id: Uuid,
    /// The app's own name for itself: nobody verified it.
    client_name: String,
    /// Where the answer goes, and its host on its own, so a name can't hide where it really goes.
    redirect_uri: String,
    redirect_host: String,
}

/// `GET /oauth/requests/{id}`: what the consent page shows.
async fn request(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(id): Path<Uuid>,
) -> Result<Json<Data<RequestView>>, ApiError> {
    browser(&caller)?;
    let services = services(&state)?;
    let pending = store::pending(&services.pool, id, caller.identity)
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    let host = url::Url::parse(&pending.redirect_uri)
        .ok()
        .map(|url| url.host_str().map_or_else(|| url.scheme().to_owned(), str::to_owned))
        .unwrap_or_default();
    Ok(Json(Data {
        data: RequestView {
            client_id: pending.client.id,
            client_name: pending.client.name,
            redirect_uri: pending.redirect_uri,
            redirect_host: host,
        },
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    /// `approve` or `deny`.
    decision: String,
    /// The project to approve it for; one the person belongs to.
    project: Option<Uuid>,
}

#[derive(Serialize)]
struct Answered {
    /// Where the browser goes now: the app's redirect URI, with the code or the refusal.
    redirect: String,
}

/// `POST /oauth/requests/{id}`: the person's answer. The session extractor holds it to the
/// Studio's origin and CSRF token (a cross-site form can't approve). Approving needs a project
/// the person belongs to; the code it makes is single use, ten minutes, bound to the client, its
/// redirect URI and PKCE challenge, and that person and project.
async fn answer(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(id): Path<Uuid>,
    body: Result<Json<Answer>, JsonRejection>,
) -> Result<Json<Data<Answered>>, ApiError> {
    browser(&caller)?;
    let services = services(&state)?;
    let origin = origin(&state)?;
    let Json(body) = body?;
    let approve = match body.decision.as_str() {
        "approve" => true,
        "deny" => false,
        _ => {
            return Err(ApiError::InvalidField {
                reason: "invalid_decision",
                pointer: "/decision",
                message: "decision is approve or deny".to_owned(),
            });
        }
    };
    let code = if approve {
        let project = body.project.ok_or_else(|| ApiError::InvalidField {
            reason: "project_required",
            pointer: "/project",
            message: "name the project to connect the app to".to_owned(),
        })?;
        projects::get(&services.pool, caller.identity, project)
            .await
            .map_err(ApiError::from_db)?
            .ok_or(ApiError::NotFound)?;
        let code = tokens::random_token().map_err(|_| ApiError::internal(std::io::Error::other("no randomness")))?;
        Some((project, code))
    } else {
        None
    };
    let hash = code.as_ref().map(|(_, code)| tokens::hash(code));
    let answered = store::answer(
        &services.pool,
        id,
        caller.identity,
        code.as_ref().zip(hash.as_ref()).map(|((project, _), hash)| (*project, hash)),
    )
    .await
    .map_err(ApiError::from_db)?
    .ok_or(ApiError::NotFound)?;
    let mut pairs: Vec<(&str, &str)> = vec![("iss", origin.as_str())];
    match &code {
        Some((project, code)) => {
            tracing::info!(identity = %caller.identity, client = %answered.client.id, project = %project, "OAuth client approved");
            pairs.push(("code", code));
        }
        None => pairs.extend([("error", "access_denied"), ("error_description", "the person declined")]),
    }
    if let Some(state) = answered.state.as_deref() {
        pairs.push(("state", state));
    }
    Ok(Json(Data {
        data: Answered {
            redirect: with_query(&answered.redirect_uri, &pairs)?,
        },
    }))
}

/// `/oauth/token`'s form (RFC 6749 §4.1.3, §6; RFC 7636 §4.5).
#[derive(Deserialize)]
struct TokenForm {
    grant_type: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

/// A fresh access and refresh token for `grant`, in RFC 6749 §5.1's answer.
async fn issue(pool: &colophon_db::PgPool, grant: Uuid) -> Result<Response, ApiError> {
    let random = || tokens::random_token().map_err(|_| ApiError::internal(std::io::Error::other("no randomness")));
    let access = format!("{ACCESS_PREFIX}{}", random()?);
    let refresh = format!("{REFRESH_PREFIX}{}", random()?);
    store::issue(pool, grant, &tokens::hash(&access), &tokens::hash(&refresh))
        .await
        .map_err(ApiError::from_db)?;
    let mut response = Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_SECONDS,
        "refresh_token": refresh,
    }))
    .into_response();
    no_store(&mut response);
    Ok(response)
}

/// Whether `verifier` is a PKCE code verifier (RFC 7636 §4.1) whose S256 is `challenge`.
fn verifier_matches(verifier: &str, challenge: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
        && tokens::same_secret(tokens::pkce_challenge(verifier).as_bytes(), challenge.as_bytes())
}

/// `POST /oauth/token`: a code for tokens, or a refresh token for new ones.
async fn token(
    State(state): State<AppState>,
    form: Result<axum::Form<TokenForm>, FormRejection>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let Ok(axum::Form(form)) = form else {
        return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "send the token request as a form"));
    };
    let Some(client) = form.client_id.as_deref().and_then(|id| Uuid::parse_str(id).ok()) else {
        return Ok(oauth_error(StatusCode::UNAUTHORIZED, "invalid_client", "client_id is required"));
    };
    let invalid = |why: &str| Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", why));
    match form.grant_type.as_deref() {
        Some("authorization_code") => {
            let Some(code) = form.code.as_deref() else {
                return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "code is required"));
            };
            let redeemed = match store::redeem_code(&services.pool, &tokens::hash(code))
                .await
                .map_err(ApiError::from_db)?
            {
                store::Presented::Taken(redeemed) => redeemed,
                store::Presented::Replayed => {
                    tracing::warn!(client = %client, "an OAuth code was presented twice: its grant is revoked");
                    return invalid("the code was already used");
                }
                store::Presented::Invalid => return invalid("the code is unknown or expired"),
            };
            // The code is used up whatever follows: a mismatch revokes the grant it would have
            // given, so a stolen code tried against the wrong verifier ends that connection.
            let bound = redeemed.grant.client == client
                && form.redirect_uri.as_deref() == Some(redeemed.redirect_uri.as_str())
                && form
                    .code_verifier
                    .as_deref()
                    .is_some_and(|verifier| verifier_matches(verifier, &redeemed.code_challenge));
            if !bound {
                store::revoke_grant(&services.pool, redeemed.grant.identity, redeemed.grant.id)
                    .await
                    .map_err(ApiError::from_db)?;
                return invalid("the code was issued to another client, redirect URI or code verifier");
            }
            issue(&services.pool, redeemed.grant.id).await
        }
        Some("refresh_token") => {
            let Some(refresh) = form.refresh_token.as_deref() else {
                return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_request", "refresh_token is required"));
            };
            match store::redeem_refresh(&services.pool, &tokens::hash(refresh), client)
                .await
                .map_err(ApiError::from_db)?
            {
                store::Presented::Taken(grant) => issue(&services.pool, grant.id).await,
                store::Presented::Replayed => {
                    tracing::warn!(client = %client, "an OAuth refresh token was used twice: its grant is revoked");
                    invalid("the refresh token was already used; the connection is revoked")
                }
                store::Presented::Invalid => invalid("the refresh token is unknown, expired or revoked"),
            }
        }
        _ => Ok(oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", "grant_type is authorization_code or refresh_token")),
    }
}

#[derive(Deserialize)]
struct RevokeForm {
    token: Option<String>,
}

/// `POST /oauth/revoke` (RFC 7009): ends the token's whole connection. Always 200, so it says
/// nothing about which tokens exist.
async fn revoke(
    State(state): State<AppState>,
    form: Result<axum::Form<RevokeForm>, FormRejection>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    if let Ok(axum::Form(RevokeForm { token: Some(token) })) = form {
        store::revoke_token(&services.pool, &tokens::hash(&token))
            .await
            .map_err(ApiError::from_db)?;
    }
    let mut response = StatusCode::OK.into_response();
    no_store(&mut response);
    Ok(response)
}

#[derive(Serialize)]
struct ConnectedView {
    id: Uuid,
    client_name: String,
    project: Uuid,
    project_name: String,
    created_at: String,
    last_used_at: Option<String>,
}

/// `GET /api/v1/me/connected-apps`: the apps the caller connected and hasn't revoked.
async fn connected(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<Data<Vec<ConnectedView>>>, ApiError> {
    browser(&caller)?;
    let services = services(&state)?;
    let rows = store::connected(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?;
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        views.push(ConnectedView {
            id: row.id,
            client_name: row.client_name,
            project: row.project,
            project_name: row.project_name,
            created_at: crate::entries::rfc3339(row.created_at)?,
            last_used_at: row.last_used_at.map(crate::entries::rfc3339).transpose()?,
        });
    }
    Ok(Json(Data { data: views }))
}

/// `DELETE /api/v1/me/connected-apps/{id}`: revokes one of the caller's connections.
async fn disconnect(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    browser(&caller)?;
    let services = services(&state)?;
    if store::revoke_grant(&services.pool, caller.identity, id)
        .await
        .map_err(ApiError::from_db)?
    {
        tracing::info!(identity = %caller.identity, grant = %id, "OAuth connection revoked");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris_are_https_loopback_http_or_a_native_scheme() {
        for good in [
            "https://claude.ai/api/mcp/auth_callback",
            "http://localhost:6274/callback",
            "http://127.0.0.1:33418/",
            "http://[::1]:8080/cb",
            "cursor://anysphere.cursor-retrieval/oauth/callback",
            "com.example.app:/oauth2redirect",
            "vscode://anthropic.claude/callback",
        ] {
            assert!(redirect_allowed(good), "{good}");
        }
        for bad in [
            "http://evil.example/cb",
            "https://claude.ai/cb#fragment",
            "https://user:pass@claude.ai/cb",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "notaurl",
            "/relative/path",
            "about:blank",
        ] {
            assert!(!redirect_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn a_verifier_matches_only_its_own_s256_challenge() {
        let verifier = "a".repeat(43);
        let challenge = tokens::pkce_challenge(&verifier);
        assert!(verifier_matches(&verifier, &challenge));
        assert!(!verifier_matches(&"b".repeat(43), &challenge));
        assert!(!verifier_matches("short", &tokens::pkce_challenge("short")));
        assert!(!verifier_matches(&verifier, &verifier), "plain isn't S256");
    }
}
