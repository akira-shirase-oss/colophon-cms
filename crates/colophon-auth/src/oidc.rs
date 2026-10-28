//! An OpenID Connect relying party: the Authorization Code flow with PKCE, `state` and `nonce`
//! (docs/modules/03-auth.md, "Sign-in"). Keycloak is the reference provider; nothing here is
//! specific to it.
//!
//! What it checks, because each one stops a known attack:
//! - the discovery document's `issuer` is exactly the configured one;
//! - the ID token's signature, with a key from the provider's JWKS, using an allowlisted
//!   asymmetric algorithm (never `none`, never HMAC with a public key);
//! - `iss` and `aud` exactly, `exp` and `nbf` with a minute of leeway, and `azp` when present;
//! - the `nonce` it sent, so a token can't be replayed into another sign-in.
//!
//! Every request has a timeout, and every response a size limit.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::{OnceCell, RwLock};
use url::Url;

use crate::tokens;

/// Signature algorithms accepted on ID tokens: asymmetric only.
const ALGORITHMS: [Algorithm; 4] = [
    Algorithm::RS256,
    Algorithm::PS256,
    Algorithm::ES256,
    Algorithm::EdDSA,
];
/// The largest discovery document, JWKS or token response read.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// How often an unknown key id may trigger a JWKS refresh.
const JWKS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// The client secret: redacted in `Debug`, no `Display`.
#[derive(Clone)]
pub struct ClientSecret(String);

impl ClientSecret {
    /// Wraps a secret.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// The secret itself.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ClientSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientSecret([REDACTED])")
    }
}

/// How this relying party is registered with the provider.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// The issuer, exactly as the provider names itself.
    pub issuer: String,
    /// This client's id.
    pub client_id: String,
    /// This client's secret (`client_secret_post`).
    pub client_secret: ClientSecret,
    /// The exact redirect URI registered with the provider.
    pub redirect_uri: String,
}

/// What a verified ID token says about the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    /// The issuer: half of the identity.
    pub issuer: String,
    /// The subject: the other half. Stable per provider; never an email address.
    pub subject: String,
    /// A name to show, if the provider gave one. Never used to identify anyone.
    pub display_name: Option<String>,
}

/// Why a sign-in failed. The messages are for logs; clients get a generic answer.
#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    /// The provider couldn't be reached, or answered with an error status.
    #[error("identity provider request failed: {0}")]
    Provider(String),
    /// The provider's discovery document is unusable.
    #[error("identity provider discovery failed: {0}")]
    Discovery(String),
    /// The provider refused the code (expired, used, or the PKCE verifier didn't match).
    #[error("the identity provider refused the code: {0}")]
    CodeRefused(String),
    /// The ID token failed verification.
    #[error("invalid ID token: {0}")]
    InvalidIdToken(String),
}

#[derive(Debug, Deserialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    /// The `prompt` values the provider understands; `create` means it can show its own sign-up
    /// form (OpenID Connect Prompt Create 1.0).
    #[serde(default)]
    prompt_values_supported: Vec<String>,
    /// Where a browser ends its session at the provider (RP-Initiated Logout 1.0), if it can.
    #[serde(default)]
    end_session_endpoint: Option<String>,
}

/// Which of the provider's pages a sign-in starts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Its sign-in form.
    SignIn,
    /// Its form for making an account, when it has one (`prompt=create`); its sign-in form
    /// otherwise, which may link to one.
    CreateAccount,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    nonce: Option<String>,
    azp: Option<String>,
    aud: serde_json::Value,
    preferred_username: Option<String>,
    name: Option<String>,
}

struct Keys {
    set: JwkSet,
    fetched: Instant,
}

/// A relying party. Discovery happens on first use, so the server starts (and reports ready)
/// even when the provider is briefly unavailable.
pub struct OidcClient {
    config: OidcConfig,
    http: reqwest::Client,
    metadata: OnceCell<Metadata>,
    keys: RwLock<Option<Keys>>,
}

impl OidcClient {
    /// A client for `config`, with request timeouts.
    pub fn new(config: OidcConfig) -> Result<Arc<Self>, OidcError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            // A provider redirecting its API endpoints is a misconfiguration, not something to follow.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| OidcError::Provider(error.to_string()))?;
        Ok(Arc::new(Self {
            config,
            http,
            metadata: OnceCell::new(),
            keys: RwLock::new(None),
        }))
    }

    /// The configuration.
    pub fn config(&self) -> &OidcConfig {
        &self.config
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, OidcError> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| OidcError::Provider(error.to_string()))?;
        if !response.status().is_success() {
            return Err(OidcError::Provider(format!(
                "GET {url} answered {}",
                response.status()
            )));
        }
        read_json(response).await
    }

    async fn metadata(&self) -> Result<&Metadata, OidcError> {
        self.metadata
            .get_or_try_init(|| async {
                let url = format!(
                    "{}/.well-known/openid-configuration",
                    self.config.issuer.trim_end_matches('/')
                );
                let metadata: Metadata = self.get_json(&url).await?;
                if metadata.issuer != self.config.issuer {
                    return Err(OidcError::Discovery(format!(
                        "the document names issuer {:?}, not {:?}",
                        metadata.issuer, self.config.issuer
                    )));
                }
                for endpoint in [
                    &metadata.authorization_endpoint,
                    &metadata.token_endpoint,
                    &metadata.jwks_uri,
                ] {
                    require_secure(endpoint)?;
                }
                Ok(metadata)
            })
            .await
    }

    /// The provider's sign-in URL for this attempt, starting on the page `start` names.
    pub async fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
        start: Start,
    ) -> Result<Url, OidcError> {
        let metadata = self.metadata().await?;
        let mut url = Url::parse(&metadata.authorization_endpoint)
            .map_err(|error| OidcError::Discovery(format!("authorization endpoint: {error}")))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", "openid profile")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", &tokens::pkce_challenge(pkce_verifier))
            .append_pair("code_challenge_method", "S256");
        if start == Start::CreateAccount
            && metadata
                .prompt_values_supported
                .iter()
                .any(|value| value == "create")
        {
            url.query_pairs_mut().append_pair("prompt", "create");
        }
        Ok(url)
    }

    /// Where a browser goes to end its session at the provider as well, then comes back to
    /// `post_logout_redirect` (OpenID Connect RP-Initiated Logout 1.0); `None` when the provider
    /// has no end-session endpoint. It names the client but sends no ID token hint, so the
    /// provider may ask the person to confirm: on a shared computer, that is the safer default.
    pub async fn logout_url(&self, post_logout_redirect: &str) -> Result<Option<Url>, OidcError> {
        let metadata = self.metadata().await?;
        let Some(endpoint) = &metadata.end_session_endpoint else {
            return Ok(None);
        };
        let mut url = Url::parse(endpoint)
            .map_err(|error| OidcError::Discovery(format!("end-session endpoint: {error}")))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("post_logout_redirect_uri", post_logout_redirect);
        Ok(Some(url))
    }

    /// Exchanges an authorization code and verifies the ID token it returns.
    pub async fn exchange(
        &self,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<VerifiedIdentity, OidcError> {
        let metadata = self.metadata().await?;
        let response = self
            .http
            .post(&metadata.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code_verifier", pkce_verifier),
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.expose()),
            ])
            .send()
            .await
            .map_err(|error| OidcError::Provider(error.to_string()))?;
        let status = response.status();
        let body: TokenResponse = read_json(response).await?;
        if !status.is_success() {
            return Err(OidcError::CodeRefused(
                body.error.unwrap_or_else(|| status.to_string()),
            ));
        }
        let id_token = body
            .id_token
            .ok_or_else(|| OidcError::InvalidIdToken("no id_token in the response".to_owned()))?;
        self.verify(&id_token, nonce).await
    }

    /// Verifies an ID token's signature and claims.
    pub async fn verify(&self, id_token: &str, nonce: &str) -> Result<VerifiedIdentity, OidcError> {
        let invalid = |reason: &str| OidcError::InvalidIdToken(reason.to_owned());
        let header =
            jsonwebtoken::decode_header(id_token).map_err(|_| invalid("unreadable header"))?;
        if !ALGORITHMS.contains(&header.alg) {
            return Err(invalid("algorithm not allowed"));
        }
        let kid = header.kid.ok_or_else(|| invalid("no key id"))?;
        let key = self.key(&kid).await?;
        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.config.issuer.as_str()]);
        validation.set_audience(&[self.config.client_id.as_str()]);
        validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 60;
        let claims = jsonwebtoken::decode::<Claims>(id_token, &key, &validation)
            .map_err(|error| OidcError::InvalidIdToken(error.to_string()))?
            .claims;
        let token_nonce = claims.nonce.ok_or_else(|| invalid("no nonce"))?;
        if !tokens::same_secret(token_nonce.as_bytes(), nonce.as_bytes()) {
            return Err(invalid("nonce mismatch"));
        }
        let audiences = match &claims.aud {
            serde_json::Value::Array(list) => list.len(),
            _ => 1,
        };
        if (audiences > 1 || claims.azp.is_some())
            && claims.azp.as_deref() != Some(self.config.client_id.as_str())
        {
            return Err(invalid("authorized party mismatch"));
        }
        if claims.sub.is_empty() || claims.sub.len() > 255 {
            return Err(invalid("subject out of range"));
        }
        let display_name = claims
            .name
            .or(claims.preferred_username)
            .map(|name| name.chars().take(200).collect());
        Ok(VerifiedIdentity {
            issuer: self.config.issuer.clone(),
            subject: claims.sub,
            display_name,
        })
    }

    /// The signing key with this id, refreshing the JWKS at most once a minute when it's unknown
    /// (providers rotate keys).
    async fn key(&self, kid: &str) -> Result<DecodingKey, OidcError> {
        {
            let keys = self.keys.read().await;
            if let Some(found) = keys.as_ref().and_then(|keys| keys.set.find(kid)) {
                return DecodingKey::from_jwk(found)
                    .map_err(|error| OidcError::InvalidIdToken(error.to_string()));
            }
        }
        let mut keys = self.keys.write().await;
        let stale = keys
            .as_ref()
            .is_none_or(|keys| keys.fetched.elapsed() >= JWKS_REFRESH_INTERVAL);
        if stale {
            let jwks_uri = self.metadata().await?.jwks_uri.clone();
            let set: JwkSet = self.get_json(&jwks_uri).await?;
            *keys = Some(Keys {
                set,
                fetched: Instant::now(),
            });
        }
        let found = keys
            .as_ref()
            .and_then(|keys| keys.set.find(kid))
            .ok_or_else(|| OidcError::InvalidIdToken("unknown signing key".to_owned()))?;
        DecodingKey::from_jwk(found).map_err(|error| OidcError::InvalidIdToken(error.to_string()))
    }

    /// Preloads keys, for tests that verify tokens without a provider.
    #[doc(hidden)]
    pub async fn set_keys(&self, set: JwkSet) {
        *self.keys.write().await = Some(Keys {
            set,
            fetched: Instant::now(),
        });
    }
}

/// HTTPS only, except for providers on this machine (the local stack).
fn require_secure(endpoint: &str) -> Result<(), OidcError> {
    let url = Url::parse(endpoint)
        .map_err(|error| OidcError::Discovery(format!("{endpoint}: {error}")))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() == "https" || (url.scheme() == "http" && local) {
        Ok(())
    } else {
        Err(OidcError::Discovery(format!("{endpoint} is not HTTPS")))
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, OidcError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(OidcError::Provider("response too large".to_owned()));
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| OidcError::Provider(error.to_string()))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(OidcError::Provider("response too large".to_owned()));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|error| OidcError::Provider(format!("unreadable response: {error}")))
}

#[cfg(test)]
mod tests {
    use super::require_secure;

    #[test]
    fn endpoints_must_be_https_unless_local() {
        assert!(require_secure("https://id.example.com/token").is_ok());
        assert!(require_secure("http://localhost:58080/realms/colophon").is_ok());
        assert!(require_secure("http://127.0.0.1:58080/x").is_ok());
        assert!(require_secure("http://id.example.com/token").is_err());
        assert!(require_secure("ftp://localhost/x").is_err());
    }
}
