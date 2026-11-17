//! Credentials in a path never reach the log (D-152): a magic link's token and a device
//! code ride in their routes' paths, and the request span records the path for every line
//! logged while handling the request — so the span writes those segments as their template
//! names instead. Tested in-process with a subscriber capturing everything, at the most
//! verbose level, through the same middleware stack the server uses.

use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use axum::{body::Body, http::Request};
use colophon_api::{HttpLimits, Readiness, middleware};
use tower::ServiceExt as _;
use tracing_subscriber::fmt::MakeWriter;

/// Everything the subscriber writes, kept for the assertions.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn a_magic_link_token_in_the_path_never_reaches_the_log() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::TRACE)
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(captured.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let state = colophon_api::AppState::without_services(Readiness::new("all", false));
    let build = state.build;
    let app = middleware::apply(
        colophon_api::routes(state),
        HttpLimits {
            request_timeout: Duration::from_secs(5),
            max_body_bytes: 1024,
        },
        build,
    );
    let token = "Tk7sEcReT-magic-link-token-0123456789abcdef";
    let code = "WXYZ-1234";
    for (method, uri) in [
        (
            "POST",
            format!("/audience/v1/01a1e62e-33d0-777b-bfaa-b6df8a8dab76/magic-link/{token}"),
        ),
        ("GET", format!("/auth/device/{code}")),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri.as_str())
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        drop(response);
    }

    let log = captured.text();
    assert!(
        log.contains("magic-link/{token}"),
        "the span still says which route ran: {log}"
    );
    assert!(!log.contains(token), "the token leaked into the log: {log}");
    assert!(
        !log.contains(code),
        "the device code leaked into the log: {log}"
    );
}
