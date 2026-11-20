//! D-221's evidence: two tickers that say when this process stopped running on time. Both sleep
//! [`TICK`] and log at DEBUG (target `colophon_server::lag`) whenever a tick arrives more than [`LATE`]
//! after it was due: one is a tokio task on the runtime, the other a plain OS thread beside it.
//!
//! How to read them beside the runner's `vmstat` (D-221's key): both late with CPU steal → the runner;
//! both late without it → a process-wide pause (an allocator purge, a page-fault storm); only the tokio
//! ticker late → something blocked the runtime (a synchronous call on a worker thread), which is ours.
//! Nothing runs unless that target is on.

use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

/// How often each ticker wakes.
const TICK: Duration = Duration::from_millis(50);
/// How late a tick must be to be logged.
const LATE: Duration = Duration::from_millis(100);

/// Starts both tickers when `colophon_server::lag` is on at DEBUG; the tokio one stops with `cancel`,
/// the thread with the process.
pub(crate) fn start(cancel: &CancellationToken) {
    if !tracing::enabled!(target: "colophon_server::lag", tracing::Level::DEBUG) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lag-ticker".to_owned())
        .spawn(|| {
            let mut due = Instant::now() + TICK;
            loop {
                std::thread::sleep(TICK);
                let now = Instant::now();
                report("thread", now.saturating_duration_since(due));
                due = now + TICK;
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the lag ticker thread didn't start");
    }
    let cancel = cancel.clone();
    tokio::spawn(async move {
        let mut due = Instant::now() + TICK;
        loop {
            tokio::select! {
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(TICK) => {}
            }
            let now = Instant::now();
            report("tokio", now.saturating_duration_since(due));
            due = now + TICK;
        }
    });
}

fn report(ticker: &'static str, late: Duration) {
    if late >= LATE {
        tracing::debug!(
            target: "colophon_server::lag",
            ticker,
            late_ms = late.as_millis(),
            "ticker ran late"
        );
    }
}
