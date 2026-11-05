//! Transactional mail through Resend (D-145): one `POST` to its emails endpoint with a bearer
//! key, on a ten-second budget so a provider stalling can never hold a request. The key and the
//! link the message carries stay out of errors and logs — the client reports status codes only.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use colophon_api::mail::{Mail, MailError};

use crate::config::Secret;

/// Resend's `POST /emails` adapter: the configured `From`, the bearer key, one send at a time.
pub(crate) struct Resend {
    client: reqwest::Client,
    endpoint: String,
    key: Secret,
    from: String,
}

impl Resend {
    /// A sender with a ten-second deadline on every call.
    pub(crate) fn new(key: Secret, from: String, endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            endpoint,
            key,
            from,
        }
    }
}

impl Mail for Resend {
    fn send<'a>(
        &'a self,
        to: &'a str,
        subject: &'a str,
        text: &'a str,
        html: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), MailError>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .client
                .post(&self.endpoint)
                .bearer_auth(self.key.expose())
                .json(&serde_json::json!({
                    "from": self.from,
                    "to": [to],
                    "subject": subject,
                    "text": text,
                    "html": html,
                }))
                .send()
                .await
                .map_err(|error| {
                    MailError(format!("the mail provider could not be reached: {error}"))
                })?;
            if !response.status().is_success() {
                return Err(MailError(format!(
                    "the mail provider answered {}",
                    response.status()
                )));
            }
            Ok(())
        })
    }
}
