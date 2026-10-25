//! Colophon's sign-in (docs/modules/03-auth.md): OpenID Connect against any compliant provider
//! (Keycloak is the reference), and the random tokens behind sessions and CSRF protection.
//! Roles, invitations and machine credentials arrive later in milestone P1.

pub mod oidc;
pub mod tokens;

pub use oidc::{ClientSecret, OidcClient, OidcConfig, OidcError, VerifiedIdentity};
