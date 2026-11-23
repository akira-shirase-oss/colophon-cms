//! Self-serve accounts (D-235): who may sign up, who may create projects, and the per-account
//! bounds an open installation runs with. The server reads them from its environment
//! (`colophon-server`'s config) and hands them to the handlers through [`crate::state::Services`].
//!
//! - `COLOPHON_REGISTRATION`: [`Registration`]. `open` lets anyone sign up and create projects
//!   within the cap and quotas; `invite` (the default) admits a sign-up only with a project
//!   invitation (or the one first-run account before setup), and only administrators create
//!   projects; `closed` admits no sign-up at all.
//! - `COLOPHON_PROJECTS_PER_ACCOUNT` (default 3): projects one account may own.
//! - `COLOPHON_ACCOUNT_MAX_ENTRIES` / `_ASSETS` / `_STORAGE_BYTES` ([`Quotas`]; `0` = no bound).
//! - `COLOPHON_SIGNUPS_PER_ADDRESS_HOUR` (default 10, an IPv6 address by its /64).
//! - `COLOPHON_SIGNED_IN_HINT_DOMAIN`: where the not-secret `colophon_signed_in=1` hint goes.
//!
//! Administrators (installation operators) are bound by neither the cap nor the quotas.

/// Who may sign up (`COLOPHON_REGISTRATION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Registration {
    /// Anyone signs up and creates projects, within the cap and the quotas.
    Open,
    /// A sign-up needs a valid, unclaimed project invitation's token (claimed for that one
    /// account), except the installation's one first-run account before setup; administrators
    /// alone create projects.
    #[default]
    Invite,
    /// No sign-up at all, a first OIDC sign-in included.
    Closed,
}

impl Registration {
    /// The setting's value as written in the environment.
    ///
    /// # Errors
    ///
    /// Anything but `open`, `invite` or `closed`.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "open" => Ok(Self::Open),
            "invite" => Ok(Self::Invite),
            "closed" => Ok(Self::Closed),
            other => Err(format!(
                "COLOPHON_REGISTRATION: {other:?} is not one of open, invite, closed"
            )),
        }
    }

    /// The setting's value as the environment writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Invite => "invite",
            Self::Closed => "closed",
        }
    }

    /// Whether an account that isn't an administrator may create projects.
    pub fn self_serve_projects(self) -> bool {
        self == Self::Open
    }
}

/// Per-account bounds on what an account's projects hold (`0` = no bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quotas {
    /// Entries across the account's projects (localizations included).
    pub entries: u64,
    /// Assets across the account's projects.
    pub assets: u64,
    /// Stored media bytes, uploads still being verified included.
    pub storage_bytes: u64,
}

impl Quotas {
    /// The defaults: 10,000 entries, 2,000 assets, 1 GiB.
    pub const DEFAULT: Self = Self {
        entries: 10_000,
        assets: 2_000,
        storage_bytes: 1 << 30,
    };
}

impl Default for Quotas {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The self-serve settings the handlers read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfServe {
    /// Who may sign up.
    pub registration: Registration,
    /// Projects one account may own (administrators: no bound).
    pub projects_per_account: u32,
    /// What an account's projects may hold (administrators: no bound).
    pub quotas: Quotas,
    /// Sign-ups one address (an IPv6 /64) may make in an hour.
    pub signups_per_address_hour: u32,
    /// The domain of the `colophon_signed_in` hint cookie; `None` sets no hint.
    pub hint_domain: Option<String>,
    /// Cloudflare Turnstile's site key, for the Studio's sign-up form; `None` renders no widget
    /// (and the server checks no token).
    pub turnstile_site_key: Option<String>,
}

impl Default for SelfServe {
    fn default() -> Self {
        Self {
            registration: Registration::default(),
            projects_per_account: 3,
            quotas: Quotas::DEFAULT,
            signups_per_address_hour: 10,
            hint_domain: None,
            turnstile_site_key: None,
        }
    }
}

/// Whether `domain` may carry a cookie for `host`: equal to it, or a parent of it on a label
/// boundary (`404human.dev` for `colophonapp.404human.dev`), and itself at least two labels
/// (never a public suffix like `dev`). Both compared lowercase; no port, no leading dot.
pub fn hint_domain_covers(domain: &str, host: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    if domain.is_empty() || !domain.contains('.') || domain.contains(':') || host.contains(':') {
        return false;
    }
    host == domain || host.ends_with(&format!(".{domain}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_parses_its_three_values_and_nothing_else() {
        assert_eq!(Registration::parse("open"), Ok(Registration::Open));
        assert_eq!(Registration::parse("invite"), Ok(Registration::Invite));
        assert_eq!(Registration::parse("closed"), Ok(Registration::Closed));
        for bad in ["Open", "", "public", "invite-only", " open"] {
            assert!(Registration::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(Registration::default(), Registration::Invite);
        assert!(Registration::Open.self_serve_projects());
        assert!(!Registration::Invite.self_serve_projects());
        assert!(!Registration::Closed.self_serve_projects());
    }

    #[test]
    fn the_hint_domain_covers_its_host_and_its_subdomains_only() {
        assert!(hint_domain_covers(
            "404human.dev",
            "colophonapp.404human.dev"
        ));
        assert!(hint_domain_covers(
            ".404human.dev",
            "colophonapp.404human.dev"
        ));
        assert!(hint_domain_covers(
            "colophonapp.404human.dev",
            "colophonapp.404human.dev"
        ));
        assert!(hint_domain_covers(
            "404Human.dev",
            "ColophonApp.404human.dev"
        ));
        for (domain, host) in [
            ("human.dev", "colophonapp.404human.dev"),
            ("dev", "colophonapp.404human.dev"),
            ("example.com", "colophonapp.404human.dev"),
            ("app.404human.dev", "colophonapp.404human.dev"),
            ("404human.dev:443", "colophonapp.404human.dev"),
            ("", "colophonapp.404human.dev"),
        ] {
            assert!(!hint_domain_covers(domain, host), "{domain} for {host}");
        }
    }
}
