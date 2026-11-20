//! D-224: closes the outage log's windows ([`colophon_api::outages`]) every
//! [`WINDOW`](colophon_api::outages::WINDOW), so an outage in progress is summarised once a window
//! and called over after a quiet one; at shutdown, what the open windows counted is said, without
//! calling anything over.

use colophon_api::outages::{OUTAGES, Outages, WINDOW};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

/// Starts the window task for the process's outages; it stops with `cancel`.
pub(crate) fn start(cancel: &CancellationToken) {
    tokio::spawn(run(&OUTAGES, cancel.clone()));
}

async fn run(outages: &'static Outages, cancel: CancellationToken) {
    let mut ticker = tokio::time::interval_at(Instant::now() + WINDOW, WINDOW);
    // After a stall, close one window late rather than several at once (each would be "quiet").
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                outages.flush();
                return;
            }
            _ = ticker.tick() => outages.close_window(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::LazyLock, time::Duration};

    use colophon_api::outages::{Fault, Outages};

    use super::*;

    static ISOLATED: LazyLock<Outages> = LazyLock::new(Outages::default);

    #[tokio::test(start_paused = true)]
    async fn windows_close_every_ten_seconds_and_a_quiet_one_ends_the_outage() {
        let cancel = CancellationToken::new();
        let task = tokio::spawn(run(&ISOLATED, cancel.clone()));
        assert!(ISOLATED.begins(Fault::NoConnection));
        assert!(!ISOLATED.begins(Fault::NoConnection));
        // The first window closes at 10 s: the outage goes on (one more occurrence counted).
        tokio::time::sleep(WINDOW + Duration::from_millis(1)).await;
        assert!(
            !ISOLATED.begins(Fault::NoConnection),
            "still the same outage"
        );
        // At 20 s the window had that one; at 30 s a quiet window ends it.
        tokio::time::sleep(WINDOW * 2).await;
        assert!(
            ISOLATED.begins(Fault::NoConnection),
            "over after a quiet window, so this begins a new one"
        );
        cancel.cancel();
        task.await.expect("the window task stops on cancel");
    }
}
