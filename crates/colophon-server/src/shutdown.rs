//! Graceful shutdown with a readiness drain and a hard deadline (rust-mastery chapter 12).
//!
//! After the trigger fires:
//!
//! 1. readiness flips to `draining`, so load balancers stop sending new requests;
//! 2. after `drain_delay`, the listener stops accepting and idle connections close;
//! 3. in-flight requests finish;
//! 4. if they haven't finished `grace` after the trigger, [`serve`] returns
//!    [`ShutdownOutcome::DeadlineExceeded`] and the process exits anyway.

use std::{future::Future, io, time::Duration};

use axum::{Router, serve::ListenerExt as _};
use colophon_api::Readiness;
use tokio::{net::TcpListener, sync::oneshot};

/// Shutdown timings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownTiming {
    /// Between "not ready" and "stop accepting".
    pub drain_delay: Duration,
    /// Hard deadline, measured from the trigger.
    pub grace: Duration,
}

/// How [`serve`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// Every in-flight request finished.
    Graceful,
    /// The deadline passed with requests still running.
    DeadlineExceeded,
}

/// Installs the SIGTERM and Ctrl-C handlers now, so a failure is a startup error rather than
/// a panic in a background task, and returns a future that resolves with the signal's name.
pub fn os_signal() -> io::Result<impl Future<Output = &'static str> + Send + 'static> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    Ok(async move {
        let ctrl_c = async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "Ctrl-C handler failed; waiting for SIGTERM only");
                std::future::pending::<()>().await;
            }
        };
        #[cfg(unix)]
        {
            tokio::select! {
                () = ctrl_c => "SIGINT",
                _ = terminate.recv() => "SIGTERM",
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await;
            "Ctrl-C"
        }
    })
}

/// Serves `app` until `trigger` resolves, then runs the drain sequence.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    readiness: Readiness,
    timing: ShutdownTiming,
    trigger: impl Future<Output = ()> + Send + 'static,
) -> io::Result<ShutdownOutcome> {
    let (triggered_tx, triggered_rx) = oneshot::channel::<()>();

    let stop_accepting = async move {
        trigger.await;
        let _receiver_gone_is_fine = triggered_tx.send(());
        readiness.begin_drain();
        tracing::info!(
            drain_delay_ms = timing.drain_delay.as_millis(),
            "shutting down: readiness is now draining"
        );
        tokio::time::sleep(timing.drain_delay).await;
        tracing::info!("drain delay over: no longer accepting connections");
    };

    // Small request/response exchanges: don't let Nagle's algorithm hold back the last packet.
    let listener = listener.tap_io(|tcp| {
        if let Err(error) = tcp.set_nodelay(true) {
            tracing::debug!(%error, "could not set TCP_NODELAY");
        }
    });
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(stop_accepting)
        .into_future();

    let hard_deadline = async move {
        // If the server ends without a trigger, the sender is dropped: never fire.
        if triggered_rx.await.is_err() {
            std::future::pending::<()>().await;
        }
        tokio::time::sleep(timing.grace).await;
    };

    tokio::select! {
        result = server => {
            result?;
            Ok(ShutdownOutcome::Graceful)
        }
        () = hard_deadline => {
            tracing::warn!(grace_ms = timing.grace.as_millis(), "shutdown deadline passed with requests still in flight");
            Ok(ShutdownOutcome::DeadlineExceeded)
        }
    }
}
