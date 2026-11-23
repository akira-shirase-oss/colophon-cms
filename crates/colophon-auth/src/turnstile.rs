//! Cloudflare Turnstile's server-side check for the sign-up form (D-235). The browser solves a
//! challenge and sends its token with the sign-up; the server asks Cloudflare's `siteverify`
//! whether that token is good, for this site, and not used before.
//!
//! Fail closed: a missing, oversized, unknown, expired or replayed token, a token for another
//! hostname, and a verifier that is slow, down, redirecting or answering anything but the
//! documented JSON all refuse the sign-up. Nothing here holds a database transaction (the caller
//! has none open), and the request has a deadline and a response-size bound.
//!
//! The hostname a token must be for is the installation's configured public host, never the
//! request's `Host` or `Origin`, which a client chooses.

use std::{fmt, net::IpAddr, time::Duration};

use serde::Deserialize;

/// Cloudflare's verification endpoint.
pub const SITEVERIFY: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// The longest token Cloudflare issues is 2048 characters; anything longer is refused unsent.
pub const MAX_TOKEN_BYTES: usize = 2048;

/// The most of `siteverify`'s answer read; its documented answer is a few hundred bytes.
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

/// Whether a token passed, and if not, why (for the log; the client gets one answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Cloudflare vouched for the token, for this hostname.
    Passed,
    /// Refused, with a short reason for the log.
    Failed(&'static str),
}

/// The verifier: the secret, where `siteverify` is, and the hostname tokens must be for.
pub struct Turnstile {
    http: reqwest::Client,
    secret: String,
    verify_url: url::Url,
    hostname: String,
}

impl fmt::Debug for Turnstile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Turnstile")
            .field("verify_url", &self.verify_url.as_str())
            .field("hostname", &self.hostname)
            .field("secret", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Why a verifier couldn't be built.
#[derive(Debug, thiserror::Error)]
pub enum TurnstileError {
    /// The verify URL is not Cloudflare's and not on this machine's loopback.
    #[error("the Turnstile verify URL must be Cloudflare's or a loopback address (for tests): {0}")]
    VerifyUrl(String),
    /// The HTTP client couldn't be built.
    #[error("the Turnstile client: {0}")]
    Client(String),
}

/// What `siteverify` answers (the fields this reads).
#[derive(Debug, Deserialize)]
pub struct SiteVerify {
    /// Whether the token is valid and unused.
    pub success: bool,
    /// The hostname the challenge was solved on.
    #[serde(default)]
    pub hostname: Option<String>,
    /// Why not, when not.
    #[serde(default, rename = "error-codes")]
    pub error_codes: Vec<String>,
}

/// Judges an answer: it passes only when Cloudflare says `success` and the token was solved on
/// `hostname` exactly (compared without case).
pub fn judge(answer: &SiteVerify, hostname: &str) -> Verdict {
    if !answer.success {
        // `timeout-or-duplicate` is an expired or replayed token; the rest are bad input or a
        // bad secret. One reason each would be noise: the log line carries the codes.
        return if answer
            .error_codes
            .iter()
            .any(|code| code == "timeout-or-duplicate")
        {
            Verdict::Failed("expired_or_replayed")
        } else {
            Verdict::Failed("rejected")
        };
    }
    match answer.hostname.as_deref() {
        Some(solved) if solved.eq_ignore_ascii_case(hostname) => Verdict::Passed,
        _ => Verdict::Failed("other_hostname"),
    }
}

/// Whether `url` may be the verify endpoint: Cloudflare's exactly, or an `http(s)` URL on a
/// loopback address (a test's stub), so configuration can never send the secret and the
/// visitors' addresses elsewhere.
pub fn allowed_verify_url(url: &url::Url) -> bool {
    if url.as_str() == SITEVERIFY {
        return true;
    }
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

impl Turnstile {
    /// A verifier for tokens solved on `hostname`, at Cloudflare's endpoint or (tests only) a
    /// loopback stub.
    ///
    /// # Errors
    ///
    /// A verify URL that is neither, or a client that can't be built.
    pub fn new(
        secret: String,
        verify_url: Option<&str>,
        hostname: String,
    ) -> Result<Self, TurnstileError> {
        let raw = verify_url.unwrap_or(SITEVERIFY);
        let verify_url =
            url::Url::parse(raw).map_err(|_| TurnstileError::VerifyUrl(raw.to_owned()))?;
        if !allowed_verify_url(&verify_url) {
            return Err(TurnstileError::VerifyUrl(raw.to_owned()));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| TurnstileError::Client(error.to_string()))?;
        Ok(Self {
            http,
            secret,
            verify_url,
            hostname,
        })
    }

    /// Asks `siteverify` about `token`, solved by the client at `remote_ip` (the address the
    /// server derived, never a header the client wrote). Fails closed.
    pub async fn verify(&self, token: Option<&str>, remote_ip: Option<IpAddr>) -> Verdict {
        let Some(token) = token.filter(|token| !token.is_empty()) else {
            return Verdict::Failed("missing");
        };
        if token.len() > MAX_TOKEN_BYTES {
            return Verdict::Failed("oversized");
        }
        let remote = remote_ip.map(|ip| ip.to_string());
        let mut form = vec![("secret", self.secret.as_str()), ("response", token)];
        if let Some(remote) = remote.as_deref() {
            form.push(("remoteip", remote));
        }
        let answer = match self
            .http
            .post(self.verify_url.clone())
            .form(&form)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => read(response).await,
            Ok(response) => {
                tracing::warn!(status = %response.status(), "Turnstile verifier refused the call");
                return Verdict::Failed("verifier_unavailable");
            }
            Err(error) => {
                tracing::warn!(error = %error, "Turnstile verifier unreachable");
                return Verdict::Failed("verifier_unavailable");
            }
        };
        match answer {
            Some(answer) => {
                let verdict = judge(&answer, &self.hostname);
                if verdict != Verdict::Passed {
                    tracing::info!(codes = ?answer.error_codes, ?verdict, "Turnstile token refused");
                }
                verdict
            }
            None => Verdict::Failed("verifier_unavailable"),
        }
    }
}

/// The answer, bounded; `None` if too large or not the documented JSON.
async fn read(mut response: reqwest::Response) -> Option<SiteVerify> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        tracing::warn!("Turnstile verifier answered too much");
        return None;
    }
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    tracing::warn!("Turnstile verifier answered too much");
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(error = %error, "Turnstile verifier's answer broke off");
                return None;
            }
        }
    }
    serde_json::from_slice(&body)
        .inspect_err(
            |error| tracing::warn!(error = %error, "Turnstile verifier's answer unreadable"),
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(success: bool, hostname: Option<&str>, codes: &[&str]) -> SiteVerify {
        SiteVerify {
            success,
            hostname: hostname.map(str::to_owned),
            error_codes: codes.iter().map(|code| (*code).to_owned()).collect(),
        }
    }

    #[test]
    fn a_token_passes_only_when_cloudflare_vouches_for_this_hostname() {
        let host = "colophonapp.example.dev";
        assert_eq!(judge(&answer(true, Some(host), &[]), host), Verdict::Passed);
        assert_eq!(
            judge(&answer(true, Some("COLOPHONAPP.example.dev"), &[]), host),
            Verdict::Passed
        );
        assert_eq!(
            judge(&answer(true, Some("evil.example.dev"), &[]), host),
            Verdict::Failed("other_hostname")
        );
        assert_eq!(
            judge(&answer(true, None, &[]), host),
            Verdict::Failed("other_hostname"),
            "a success that names no hostname isn't for this one"
        );
        assert_eq!(
            judge(&answer(false, Some(host), &["timeout-or-duplicate"]), host),
            Verdict::Failed("expired_or_replayed")
        );
        assert_eq!(
            judge(
                &answer(false, Some(host), &["invalid-input-response"]),
                host
            ),
            Verdict::Failed("rejected")
        );
    }

    #[test]
    fn the_verify_url_is_cloudflares_or_loopback_only() {
        for good in [
            SITEVERIFY,
            "http://127.0.0.1:9999/siteverify",
            "http://[::1]:9999/x",
            "http://localhost:1/x",
        ] {
            assert!(
                allowed_verify_url(&url::Url::parse(good).expect("url")),
                "{good}"
            );
        }
        for bad in [
            "https://challenges.cloudflare.com/turnstile/v0/siteverify/",
            "https://challenges.cloudflare.com.evil.dev/turnstile/v0/siteverify",
            "https://attacker.dev/siteverify",
            "http://10.0.0.1/siteverify",
            "http://localhost.evil.dev/x",
            "file:///etc/passwd",
        ] {
            assert!(
                !allowed_verify_url(&url::Url::parse(bad).expect("url")),
                "{bad}"
            );
            assert!(
                Turnstile::new("s".to_owned(), Some(bad), "h".to_owned()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_secret_never_prints() {
        let verifier =
            Turnstile::new("top-secret".to_owned(), None, "h".to_owned()).expect("a verifier");
        assert!(!format!("{verifier:?}").contains("top-secret"));
    }
}
