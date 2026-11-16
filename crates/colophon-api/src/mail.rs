//! Transactional mail: the one thing the API needs a sender for today is a sign-in link (D-145).
//! The trait is the seam a deployment's own provider sits behind — Resend is the adapter the
//! server ships; an SMTP one lands here without the API changing.
//!
//! Mail is deliberately best-effort at the call site: a magic-link request answers `202` whether
//! or not the address has an account, so a failed send warns in the server's log rather than
//! changing what the response can leak.

use std::future::Future;
use std::pin::Pin;

/// A transactional mail sender, such as the server's Resend adapter.
pub trait Mail: Send + Sync {
    /// Delivers one message — `text` always, `html` alongside it — or says why the provider
    /// could not take it. Implementations keep secrets and message bodies out of their errors.
    fn send<'a>(
        &'a self,
        to: &'a str,
        subject: &'a str,
        text: &'a str,
        html: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), MailError>> + Send + 'a>>;
}

/// A send that did not happen. Carries only the provider's failure — never the key, the link,
/// or the message — so a caller may log it freely.
#[derive(Debug)]
pub struct MailError(pub String);

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MailError {}
