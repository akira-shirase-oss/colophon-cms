//! Configuration from the environment, read once at startup and checked as a whole: every
//! problem is reported together, and the token never appears in any message or `Debug` output.
//!
//! - `COLOPHON_SERVER`: the server's origin, such as `https://cms.example.com`. Plain `http` is
//!   accepted only for a loopback host, so the token never crosses a network in the clear.
//! - `COLOPHON_TOKEN`: a project API token (`colophon_api_…`, D-063). A person's sign-in token is
//!   refused: an agent acts as its own identity, with the permissions its token was given.
//! - `COLOPHON_PROJECT`: the token's project id. A token works only in its own project, and the
//!   API has no route that tells a token which project that is.

use std::fmt;

use reqwest::Url;
use uuid::Uuid;

/// The prefix of a project API token's secret (D-063).
pub const API_TOKEN_PREFIX: &str = "colophon_api_";
/// The length of the random part after the prefix.
const SECRET_LENGTH: usize = 43;

/// A secret that never prints: no `Display`, and `Debug` is redacted.
#[derive(Clone)]
pub struct Secret(Box<str>);

impl Secret {
    /// The secret itself, for the one place that sends it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// Where the server is, and who the agent is there.
#[derive(Debug, Clone)]
pub struct Config {
    /// The server's origin.
    pub server: Url,
    /// The project API token.
    pub token: Secret,
    /// The token's project.
    pub project: Uuid,
}

/// Every problem with the environment, in one error.
#[derive(Debug, thiserror::Error)]
#[error("invalid configuration: {}", problems.join("; "))]
pub struct ConfigError {
    /// One line per problem, never including the token.
    pub problems: Vec<String>,
}

impl Config {
    /// Reads the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var_os(key).map(|value| value.to_string_lossy().into()))
    }

    /// Reads the configuration through `lookup`, so tests never touch the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut problems = Vec::new();
        let mut required = |key: &str| match lookup(key) {
            Some(value) if !value.trim().is_empty() => Some(value),
            _ => {
                problems.push(format!("{key} is required"));
                None
            }
        };
        let (server, token, project) = (
            required("COLOPHON_SERVER"),
            required("COLOPHON_TOKEN"),
            required("COLOPHON_PROJECT"),
        );
        let server = server.and_then(|text| {
            server_url(text.trim())
                .map_err(|problem| problems.push(format!("COLOPHON_SERVER: {problem}")))
                .ok()
        });
        let token = token.and_then(|text| {
            api_token(&text)
                .map_err(|problem| problems.push(format!("COLOPHON_TOKEN: {problem}")))
                .ok()
        });
        let project = project.and_then(|text| {
            Uuid::try_parse(text.trim())
                .map_err(|_| problems.push("COLOPHON_PROJECT: not a project id (a UUID)".into()))
                .ok()
        });
        match (server, token, project) {
            (Some(server), Some(token), Some(project)) if problems.is_empty() => Ok(Self {
                server,
                token,
                project,
            }),
            _ => Err(ConfigError { problems }),
        }
    }
}

/// An http(s) origin; plain http only for loopback.
fn server_url(text: &str) -> Result<Url, String> {
    let url = Url::parse(text).map_err(|error| format!("not a URL ({error})"))?;
    let loopback = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => {
            return Err(
                "plain http is accepted only for localhost: use https, so the token stays private"
                    .into(),
            );
        }
        other => return Err(format!("{other:?} is not http or https")),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not hold credentials".into());
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err("must be an origin only, such as https://cms.example.com".into());
    }
    Ok(url)
}

/// A project API token; the message never repeats it.
fn api_token(text: &str) -> Result<Secret, String> {
    let Some(secret) = text.strip_prefix(API_TOKEN_PREFIX) else {
        return Err(format!(
            "not a project API token (those start with {API_TOKEN_PREFIX}); the MCP server never uses a person's sign-in"
        ));
    };
    let shaped = secret.len() == SECRET_LENGTH
        && secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if shaped {
        Ok(Secret(text.into()))
    } else {
        Err(format!(
            "malformed: {} characters after the prefix where {SECRET_LENGTH} letters, digits, '-' or '_' are expected",
            secret.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const TOKEN: &str = "colophon_api_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";
    const PROJECT: &str = "0190f0c2-7a4e-7cc1-9d1e-2b8a4c6f1a01";

    fn load(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn a_complete_environment_loads() {
        let config = load(&[
            ("COLOPHON_SERVER", "http://localhost:58000"),
            ("COLOPHON_TOKEN", TOKEN),
            ("COLOPHON_PROJECT", PROJECT),
        ])
        .expect("valid");
        assert_eq!(config.server.as_str(), "http://localhost:58000/");
        assert_eq!(config.project.to_string(), PROJECT);
        assert_eq!(config.token.expose(), TOKEN);
    }

    #[test]
    fn every_problem_is_reported_at_once_and_the_token_never_appears() {
        let error = load(&[
            ("COLOPHON_SERVER", "http://cms.example.com"),
            ("COLOPHON_TOKEN", "colophon_api_short"),
        ])
        .expect_err("invalid");
        assert_eq!(error.problems.len(), 3, "{error}");
        let text = format!("{error} {error:?}");
        assert!(text.contains("COLOPHON_PROJECT is required"), "{text}");
        assert!(text.contains("plain http"), "{text}");
        assert!(text.contains("malformed: 5 characters"), "{text}");
        assert!(!text.contains("colophon_api_short"), "{text}");
    }

    #[test]
    fn a_person_s_token_is_refused() {
        let error = load(&[
            ("COLOPHON_SERVER", "https://cms.example.com"),
            (
                "COLOPHON_TOKEN",
                "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG",
            ),
            ("COLOPHON_PROJECT", PROJECT),
        ])
        .expect_err("a session token");
        assert!(error.problems[0].contains("never uses a person's sign-in"));
        assert!(!format!("{error:?}").contains("abcdefghijklmnop"));
    }

    #[test]
    fn the_server_is_an_origin_without_credentials() {
        for (server, problem) in [
            ("https://user:pw@cms.example.com", "credentials"),
            ("https://cms.example.com/api", "origin only"),
            ("ftp://cms.example.com", "not http or https"),
            ("cms.example.com", "not a URL"),
        ] {
            let error = load(&[
                ("COLOPHON_SERVER", server),
                ("COLOPHON_TOKEN", TOKEN),
                ("COLOPHON_PROJECT", PROJECT),
            ])
            .expect_err(server);
            assert!(error.problems[0].contains(problem), "{server}: {error}");
        }
        for loopback in ["http://127.0.0.1:58000", "http://[::1]:58000"] {
            load(&[
                ("COLOPHON_SERVER", loopback),
                ("COLOPHON_TOKEN", TOKEN),
                ("COLOPHON_PROJECT", PROJECT),
            ])
            .expect(loopback);
        }
    }

    #[test]
    fn secrets_are_redacted() {
        let config = load(&[
            ("COLOPHON_SERVER", "https://cms.example.com"),
            ("COLOPHON_TOKEN", TOKEN),
            ("COLOPHON_PROJECT", PROJECT),
        ])
        .expect("valid");
        let shown = format!("{config:?}");
        assert!(shown.contains("Secret([REDACTED])"), "{shown}");
        assert!(!shown.contains(TOKEN), "{shown}");
    }
}
