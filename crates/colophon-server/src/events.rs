//! The change-hint pump (D-180) and listener (D-108). The pump sends what writers queued in
//! `change_hints`; the listener is one `PgListener` per API process on the `colophon_events`
//! channel, decoding each committed payload into a [`Hint`] and publishing it on the bus the
//! `{P}/events` streams subscribe to.
//!
//! The listener is deliberately *not* a pooled connection — `LISTEN` holds its connection for
//! the process's life, and a pooled one would starve the pool. Reconnects are handled here
//! rather than trusted to hide gaps: sqlx re-subscribes on its own after a loss, but any
//! notification Postgres sent while the connection was dead is gone, so every reconnect (and
//! every hard error) ends in a `resync` to all streams — clients reload instead of trusting a
//! view that may have skipped a commit. There is no durable replay anywhere in the chain.

use std::{sync::Arc, time::Duration};

use colophon_api::events::EventBus;
use colophon_db::events::{CHANNEL, Hint};
use tokio_util::sync::CancellationToken;

/// How often the pump sends queued hints (D-180): the most a live list's hint waits.
const PUMP_EVERY: Duration = Duration::from_millis(100);

/// Sends queued change hints until `cancel` fires (D-180): one pass every 100 ms, and
/// at once again while a pass comes back full. A failed pass waits for the next tick — the
/// hints stay queued, so a database blip delays them and loses none that committed.
pub async fn pump(pool: colophon_db::PgPool, cancel: CancellationToken) {
    let mut ticks = tokio::time::interval(PUMP_EVERY);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            _ = ticks.tick() => {}
        }
        loop {
            let pass = tokio::select! {
                () = cancel.cancelled() => return,
                pass = tokio::time::timeout(PUMP_DEADLINE, colophon_db::events::pump(&pool)) => pass,
            };
            match pass {
                Ok(Ok(drained))
                    if drained
                        >= u64::try_from(colophon_db::events::PUMP_BATCH).unwrap_or(u64::MAX) => {}
                Ok(Ok(_)) => break,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "sending change hints failed; trying again");
                    break;
                }
                Err(_) => {
                    tracing::warn!("sending change hints timed out; trying again");
                    break;
                }
            }
        }
    }
}

/// The deadline for one pump pass.
const PUMP_DEADLINE: Duration = Duration::from_secs(5);

/// How long to wait before reconnecting after a failure. Bounded, so a database that never
/// comes back can't spin, and short, so a blip's streams resync quickly.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Runs the listener until `cancel` fires. Reconnects forever after failures: a stopped
/// listener silently stops the live lists, and the reconnect itself is what tells streams to
/// resync. Every blocking call is wrapped in the cancellation token — `connect` and `listen`
/// can wait out sqlx's internal retries on a dead database, and a shutdown must not.
pub async fn run(bus: Arc<EventBus>, database_url: String, cancel: CancellationToken) {
    'reconnect: loop {
        let connected = tokio::select! {
            () = cancel.cancelled() => return,
            connected = colophon_db::listener(&database_url) => connected,
        };
        let mut listener = match connected {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(
                    %error,
                    cause = std::error::Error::source(&error).map(ToString::to_string),
                    "the change-hint listener could not connect; retrying"
                );
                tokio::select! {
                    () = cancel.cancelled() => return,
                    () = tokio::time::sleep(RECONNECT_DELAY) => continue 'reconnect,
                }
            }
        };
        let listening = tokio::select! {
            () = cancel.cancelled() => return,
            result = listener.listen(CHANNEL) => result,
        };
        if let Err(error) = listening {
            tracing::warn!(%error, "the change-hint listener could not subscribe; retrying");
            tokio::select! {
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(RECONNECT_DELAY) => continue 'reconnect,
            }
        }
        loop {
            let message = tokio::select! {
                () = cancel.cancelled() => return,
                message = listener.try_recv() => message,
            };
            match message {
                Ok(Some(notification)) => {
                    if let Some(hint) = Hint::decode(notification.payload()) {
                        bus.publish(hint);
                    } else {
                        // A payload we don't read can only under-deliver; it still means
                        // *something* changed, so be loud rather than skip silently.
                        tracing::warn!(
                            payload = notification.payload(),
                            "a change hint did not decode"
                        );
                    }
                }
                // The connection was lost and re-established (sqlx already re-listened):
                // anything notified in the gap is gone, so every stream resyncs.
                Ok(None) => {
                    tracing::warn!("the change-hint listener reconnected; streams resync");
                    bus.resync();
                }
                Err(error) => {
                    tracing::warn!(%error, "the change-hint listener failed; reconnecting");
                    bus.resync();
                    tokio::select! {
                        () = cancel.cancelled() => return,
                        () = tokio::time::sleep(RECONNECT_DELAY) => continue 'reconnect,
                    }
                }
            }
        }
    }
}
