//! Beginning a transaction so a cancelled caller can't leave one open on the pool (D-217).
//!
//! sqlx 0.9.0 counts a connection's transaction depth only once the server has answered
//! `BEGIN`: a `pool.begin()` dropped while waiting for that answer (a client that went away,
//! a request deadline) rolls back nothing, since its guard rolls back only at depth one or more.
//! The connection then returns to the pool inside the open transaction and still looks clean.
//! The next borrower's autocommit writes run inside that transaction, invisible to every other
//! connection (a session just written reads back as signed out), and its own `BEGIN` draws
//! "there is already a transaction in progress". Two guards:
//!
//! - [`begin_tx`] and [`begin_tx_snapshot`] run the `BEGIN` in a task of its own, so dropping the
//!   caller can't cut it short: the task finishes, and a transaction nobody is waiting for any
//!   more drops at depth one, which rolls it back before the connection is pooled again. They are
//!   the only way this workspace begins a transaction on a pool (`tests/one_begin.rs` checks).
//! - The begin tripwire (D-226): a connection that is somehow inside a transaction already is
//!   harmful only at its next `BEGIN`, and Postgres says so for free: that `BEGIN` draws the warning
//!   "there is already a transaction in progress". sqlx 0.9.0 hands a notice to nobody, it only
//!   emits it as a tracing event (sqlx-postgres `connection/stream.rs`), so [`begin_tripwire`] is a
//!   tracing layer that marks the begin it happened in, and [`begin_tx`] rolls that transaction back
//!   and fails loudly instead of handing it out ([`begin_trips`]). No extra round trip.
//! - [`release_check`], the pool's `after_release` hook, closes any connection handed back while
//!   still inside a transaction and counts it ([`dirty_releases`]). It costs a round trip per
//!   release (D-226 measured it doubling a read's statements), so once [`arm_begin_tripwire`] has
//!   proved the tripwire works in this process it runs on one release in [`CANARY_EVERY`]; until
//!   then, and whenever that proof fails, it runs on every release.
//!
//! Every other way a transaction can end leaves its connection clean without either guard, and
//! `tests/release_paths.rs` proves each one with the check off: a `Transaction` dropped while open
//! (a handler dropped mid-transaction, a cancelled COMMIT, an aborted transaction, a panic) queues
//! a `ROLLBACK`, which sqlx's own release `ping` flushes before the connection is pooled again
//! (sqlx-core `pool/connection.rs` `return_to_pool`; sqlx-postgres `start_rollback`,
//! `wait_until_ready`).

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sqlx::Transaction;
use sqlx::pool::PoolConnectionMetadata;
use sqlx::postgres::{PgConnection, PgPool, Postgres};

/// Begins a transaction on a connection from `pool`, at Postgres's default isolation.
pub async fn begin_tx(pool: &PgPool) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let pool = pool.clone();
    detached(async move { pool.begin().await }).await
}

/// The longest one statement of a background unit (a sweep, a fold, a pump pass) may run, and
/// the longest it may wait on a lock: both under the server's 5 s cleanup deadline.
pub const BACKGROUND_STATEMENT_TIMEOUT: Duration = Duration::from_secs(4);
/// See [`BACKGROUND_STATEMENT_TIMEOUT`]. Shorter, so a unit queued behind a held row errors as
/// a lock wait, not as a slow statement; not so short that ordinary contention fails a sweep.
pub const BACKGROUND_LOCK_TIMEOUT: Duration = Duration::from_secs(3);

/// Bounds `transaction` as a background unit's: its statements end at
/// [`BACKGROUND_STATEMENT_TIMEOUT`] and its lock waits at [`BACKGROUND_LOCK_TIMEOUT`], server
/// side. A background loop never drops a unit to stop it (a COMMIT cancelled in flight queues
/// a `ROLLBACK` the server answers with "there is no transaction in progress", and leaves
/// whether the work committed unknown); it lets the unit finish, and these bounds are what
/// keep that finite: a stuck unit fails with an error and is rolled back, on its own.
pub async fn bound_background(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT set_config('statement_timeout', $1, true), set_config('lock_timeout', $2, true)",
    )
    .bind(BACKGROUND_STATEMENT_TIMEOUT.as_millis().to_string())
    .bind(BACKGROUND_LOCK_TIMEOUT.as_millis().to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

/// Asked between a multi-statement background unit's statements: has its loop been told to
/// stop? [`BACKGROUND_STATEMENT_TIMEOUT`] bounds one statement, not a unit of many, so a unit
/// that runs several (the table sweep's passes, the budget reconciler, the hint pump) checks
/// this before each after its first and, told to stop, ends with an explicit `ROLLBACK`
/// (`transactions::stopped`): after a stop it runs at most the one statement it was in, inside the
/// server's cleanup deadline. A loop that isn't stopping passes [`Stop::NEVER`] in effect.
#[derive(Clone, Copy)]
pub struct Stop<'a>(&'a (dyn Fn() -> bool + Send + Sync));

fn never() -> bool {
    false
}

impl<'a> Stop<'a> {
    /// Never stops: for a caller with no loop to stop (a test, a one-off).
    pub const NEVER: Stop<'static> = Stop(&never);

    /// Stops once `requested` says so.
    pub const fn new(requested: &'a (dyn Fn() -> bool + Send + Sync)) -> Self {
        Self(requested)
    }

    /// Whether a stop has been requested.
    pub fn requested(&self) -> bool {
        (self.0)()
    }
}

/// How often a background unit still waiting for a connection asks whether it was told to stop.
const STOP_ASKED_EVERY: Duration = Duration::from_millis(25);

/// Begins a background unit's transaction, bounded ([`bound_background`]), unless `stop` is
/// requested first. The wait for a connection and the `BEGIN` are raced against the stop (asked
/// every 25 ms): dropping a [`begin_tx`] is safe at any point, since its task finishes the
/// `BEGIN` and a transaction nobody holds rolls back before its connection is pooled again
/// (D-217). So a unit still queued for a connection when its loop is told to stop never begins,
/// and a shutdown never waits on a pool's acquire timeout plus a statement, only on the one
/// statement a unit already began is in. Asked once more after the `BEGIN`, a stop ends the unit
/// before its first statement, with an explicit `ROLLBACK`.
pub async fn begin_background(
    pool: &PgPool,
    stop: Stop<'_>,
) -> Result<Transaction<'static, Postgres>, crate::DbError> {
    if stop.requested() {
        return Err(crate::DbError::Stopped);
    }
    let pending = begin_tx(pool);
    tokio::pin!(pending);
    let mut asks = tokio::time::interval(STOP_ASKED_EVERY);
    asks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut transaction = loop {
        tokio::select! {
            biased;
            outcome = &mut pending => break outcome.map_err(crate::DbError::from_query)?,
            _ = asks.tick() => {
                if stop.requested() {
                    return Err(crate::DbError::Stopped);
                }
            }
        }
    };
    bound_background(&mut transaction)
        .await
        .map_err(crate::DbError::from_query)?;
    if stop.requested() {
        return stopped(transaction).await;
    }
    Ok(transaction)
}

/// [`begin_background`], then `SET LOCAL ROLE colophon_worker`: the transaction of a background
/// unit that runs as the worker role (the sweeps, the reconciler, the claims).
pub(crate) async fn background_worker(
    pool: &PgPool,
    stop: Stop<'_>,
) -> Result<Transaction<'static, Postgres>, crate::DbError> {
    let mut transaction = begin_background(pool, stop).await?;
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *transaction)
        .await
        .map_err(crate::DbError::from_query)?;
    Ok(transaction)
}

/// Ends a unit [`Stop`] told to stop: rolls `transaction` back, explicitly, and says so.
pub(crate) async fn stopped<T>(
    transaction: Transaction<'static, Postgres>,
) -> Result<T, crate::DbError> {
    transaction
        .rollback()
        .await
        .map_err(crate::DbError::from_query)?;
    Err(crate::DbError::Stopped)
}

/// Begins a `REPEATABLE READ` transaction: one snapshot for all its reads.
pub async fn begin_tx_snapshot(
    pool: &PgPool,
) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
    let pool = pool.clone();
    detached(async move {
        pool.begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ")
            .await
    })
    .await
}

/// Runs `begin` to completion in its own task, even if the caller stops waiting. A task that
/// panicked or was cancelled (the runtime shutting down) is an error, never a panic here.
async fn detached<F>(begin: F) -> Result<Transaction<'static, Postgres>, sqlx::Error>
where
    F: Future<Output = Result<Transaction<'static, Postgres>, sqlx::Error>> + Send + 'static,
{
    let started = std::time::Instant::now();
    let (transaction, _seen) = watched(begin, true).await?;
    // D-221's evidence: a request's time spent getting a connection and beginning, for its slow-request line.
    let _ = PHASES.try_with(|phases| phases.begun(started.elapsed()));
    transaction
}

/// What the begin tripwire saw while one `BEGIN` ran.
#[derive(Debug, Default)]
struct Watch {
    /// A backend notice reached the tripwire inside this begin's task.
    notice: Cell<bool>,
    /// That notice said the connection was already inside a transaction.
    tripped: Cell<bool>,
    /// This begin is the startup self-test's, which trips on purpose: its notice is expected, and
    /// [`hide_the_self_test_notice`] keeps it out of the logs.
    self_test: bool,
}

tokio::task_local! {
    static BEGINNING: Watch;
}

/// Runs `begin` in its own task with the tripwire watching it. A tripped begin is rolled back in
/// that task (so a caller that stops waiting can't cut the rollback short) and becomes an error;
/// `report` counts and logs it (the startup self-test trips on purpose and doesn't). Returns the
/// outcome and whether any notice reached the tripwire.
async fn watched<F>(
    begin: F,
    report: bool,
) -> Result<(Result<Transaction<'static, Postgres>, sqlx::Error>, bool), sqlx::Error>
where
    F: Future<Output = Result<Transaction<'static, Postgres>, sqlx::Error>> + Send + 'static,
{
    let watch = Watch {
        self_test: !report,
        ..Watch::default()
    };
    tokio::spawn(BEGINNING.scope(watch, async move {
        let outcome = begin.await;
        let (noticed, tripped) = BEGINNING.with(|watch| (watch.notice.get(), watch.tripped.get()));
        let Ok(transaction) = outcome else {
            return (outcome, noticed);
        };
        if !tripped {
            return (Ok(transaction), noticed);
        }
        // The connection was inside a transaction before this BEGIN: whatever that one held, it
        // must not commit as this caller's. Rolling back ends it on the server too.
        let rolled_back = transaction.rollback().await.is_ok();
        if report {
            let trips = TRIPS.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::error!(
                trips,
                rolled_back,
                "BEGIN found its connection already inside a transaction; rolled it back and refused it (D-226)"
            );
        }
        (
            Err(sqlx::Error::Protocol(
                "the pooled connection was already inside a transaction".to_owned(),
            )),
            noticed,
        )
    }))
    .await
    .map_err(|_| sqlx::Error::WorkerCrashed)
}

/// Begins that found their connection already inside a transaction (D-226). Anything above zero is
/// a bug: some path left a transaction open on a pooled connection.
static TRIPS: AtomicU64 = AtomicU64::new(0);

/// How many begins the tripwire has refused since the process started.
pub fn begin_trips() -> u64 {
    TRIPS.load(Ordering::Relaxed)
}

/// What Postgres says, in its C locale, when a `BEGIN` arrives inside a transaction (SQLSTATE
/// 25001). sqlx passes on the text only, so the startup self-test proves this server speaks it.
const ALREADY_IN_TRANSACTION: &str = "there is already a transaction in progress";

/// sqlx's target for backend notices (sqlx-postgres `connection/stream.rs`).
const NOTICE_TARGET: &str = "sqlx::postgres::notice";

/// Notices that reached the tripwire anywhere, inside a begin or not (the self-test's diagnosis).
static NOTICES: AtomicU64 = AtomicU64::new(0);

/// The begin tripwire as a tracing layer, with its own filter: it sees sqlx's notices whatever the
/// rest of the subscriber's filter says. Install it beside the log layers, which must then filter
/// per layer rather than globally (a global filter would hide the notices from it too).
pub fn begin_tripwire<S>() -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use tracing_subscriber::Layer as _;
    BeginTripwire.with_filter(
        tracing_subscriber::filter::Targets::new().with_target(NOTICE_TARGET, tracing::Level::WARN),
    )
}

struct BeginTripwire;

/// A per-layer filter for the log layers: it drops sqlx's backend notices raised inside the startup
/// self-test's own begin, the one place a nested `BEGIN` is made on purpose, so the self-test logs
/// no WARN. It goes by where the notice was raised (the self-test's begin task, marked in its
/// task-local watch), never by the notice's text: a real nested `BEGIN` anywhere else still logs
/// its WARN, beside the tripwire's own ERROR. The tripwire layer keeps its own filter and still
/// sees the notice, which is how the self-test proves it works.
pub fn hide_the_self_test_notice()
-> tracing_subscriber::filter::FilterFn<fn(&tracing::Metadata<'_>) -> bool> {
    tracing_subscriber::filter::filter_fn(not_the_self_test_notice)
}

fn not_the_self_test_notice(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.target() != NOTICE_TARGET
        || !BEGINNING.try_with(|watch| watch.self_test).unwrap_or(false)
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for BeginTripwire {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != NOTICE_TARGET {
            return;
        }
        NOTICES.fetch_add(1, Ordering::Relaxed);
        let mut message = AlreadyInTransaction(false);
        event.record(&mut message);
        // Outside a begin's task there's nothing to mark; sqlx logs the notice as usual.
        let _ = BEGINNING.try_with(|watch| {
            watch.notice.set(true);
            if message.0 {
                watch.tripped.set(true);
            }
        });
    }
}

/// Whether an event's `message` field is the already-in-a-transaction warning.
struct AlreadyInTransaction(bool);

impl tracing::field::Visit for AlreadyInTransaction {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" && value.contains(ALREADY_IN_TRANSACTION) {
            self.0 = true;
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && format!("{value:?}").contains(ALREADY_IN_TRANSACTION) {
            self.0 = true;
        }
    }
}

/// Releases between two release checks once the tripwire is armed: a backstop for a desync the
/// tripwire can't see (sqlx changing how it reports notices), at one round trip in this many.
pub const CANARY_EVERY: u64 = 256;

/// Whether every release is checked. True until [`arm_begin_tripwire`] proves the tripwire works,
/// and again whenever that proof fails: safety never degrades silently.
static CHECK_EVERY_RELEASE: AtomicBool = AtomicBool::new(true);

/// Releases seen while the tripwire is armed, to pick the one in [`CANARY_EVERY`] that is checked.
static RELEASES: AtomicU64 = AtomicU64::new(0);

/// Why the begin tripwire isn't armed.
#[derive(Debug, thiserror::Error)]
pub enum TripwireOff {
    /// No notice reached the tripwire: it isn't installed, or a filter hides sqlx's notices.
    #[error(
        "no backend notice reached the begin tripwire: is `begin_tripwire()` installed, and is the `sqlx::postgres::notice` target filtered out?"
    )]
    NoNotice,
    /// A notice arrived, but not with Postgres's C-locale text.
    #[error(
        "a notice reached the begin tripwire, but not in Postgres's C-locale wording: is the server's lc_messages localised?"
    )]
    OtherText,
    /// Notices reach the tripwire, but not inside the begin's task.
    #[error("backend notices reach the begin tripwire outside the begin's task")]
    OutsideTheBegin,
    /// A desynced begin was handed out: the proof's own failure.
    #[error("a BEGIN on a connection already inside a transaction was handed out")]
    NotRefused,
    /// The proof couldn't run (the database unreachable, a timeout).
    #[error("the begin tripwire self-test could not run: {0}")]
    CouldNotRun(String),
}

/// Proves the begin tripwire works in this process, on one throwaway connection to `pool`'s
/// database, and arms it: a `BEGIN` sent by hand leaves the connection inside a transaction, then a
/// begin through the guarded path must trip. Armed, the release check runs on one release in
/// [`CANARY_EVERY`]; otherwise every release is checked, and the error says which dependency failed
/// (the server logs it at ERROR). Call once at startup, after the subscriber is installed.
pub async fn arm_begin_tripwire(
    options: sqlx::postgres::PgConnectOptions,
) -> Result<(), TripwireOff> {
    let outcome = tokio::time::timeout(Duration::from_secs(10), prove_the_tripwire(options))
        .await
        .unwrap_or_else(|_| Err(TripwireOff::CouldNotRun("timed out after 10 s".to_owned())));
    CHECK_EVERY_RELEASE.store(outcome.is_err(), Ordering::Relaxed);
    outcome
}

async fn prove_the_tripwire(options: sqlx::postgres::PgConnectOptions) -> Result<(), TripwireOff> {
    let could_not = |error: sqlx::Error| TripwireOff::CouldNotRun(error.to_string());
    // One connection and no release hook: the hand-made transaction must survive the release.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect_lazy_with(options);
    let outcome = async {
        let mut conn = pool.acquire().await.map_err(could_not)?;
        sqlx::raw_sql("BEGIN")
            .execute(&mut *conn)
            .await
            .map_err(could_not)?;
        drop(conn);
        let seen_before = NOTICES.load(Ordering::Relaxed);
        let one = pool.clone();
        let (begun, noticed) = watched(async move { one.begin().await }, false)
            .await
            .map_err(could_not)?;
        match begun {
            Ok(transaction) => {
                let _ = transaction.rollback().await;
                Err(if noticed {
                    TripwireOff::OtherText
                } else if NOTICES.load(Ordering::Relaxed) > seen_before {
                    TripwireOff::OutsideTheBegin
                } else {
                    TripwireOff::NoNotice
                })
            }
            Err(sqlx::Error::Protocol(_)) => Ok(()),
            Err(error) => Err(could_not(error)),
        }
    }
    .await;
    pool.close().await;
    outcome
}

/// What one request spent beginning transactions (D-221): the pool's wait for a connection and the `BEGIN`
/// itself, summed, and how many. A request runs inside [`with_phases`]; outside one, nothing is counted.
#[derive(Debug, Default)]
pub struct Phases {
    begin_micros: AtomicU64,
    begins: AtomicU64,
}

impl Phases {
    fn begun(&self, took: std::time::Duration) {
        let micros = u64::try_from(took.as_micros()).unwrap_or(u64::MAX);
        self.begin_micros.fetch_add(micros, Ordering::Relaxed);
        self.begins.fetch_add(1, Ordering::Relaxed);
    }

    /// Microseconds spent getting a connection and beginning, over every transaction so far.
    pub fn begin_micros(&self) -> u64 {
        self.begin_micros.load(Ordering::Relaxed)
    }

    /// Transactions begun so far.
    pub fn begins(&self) -> u64 {
        self.begins.load(Ordering::Relaxed)
    }
}

tokio::task_local! {
    static PHASES: std::sync::Arc<Phases>;
}

/// Runs `work` with `phases` counting its transaction begins (D-221).
pub async fn with_phases<F: Future>(phases: std::sync::Arc<Phases>, work: F) -> F::Output {
    PHASES.scope(phases, work).await
}

/// Connections the pool closed because they came back inside a transaction.
static DIRTY: AtomicU64 = AtomicU64::new(0);

/// Statements the release check has sent: each is a round trip on the connection before the pool
/// can hand it out again (D-226).
static CHECKS: AtomicU64 = AtomicU64::new(0);

/// How many statements [`release_check`] has sent since the process started.
pub fn release_checks() -> u64 {
    CHECKS.load(Ordering::Relaxed)
}

/// How many connections [`release_check`] has closed since the process started. Anything above
/// zero is a bug: a transaction began outside [`begin_tx`] and was left open.
pub fn dirty_releases() -> u64 {
    DIRTY.load(Ordering::Relaxed)
}

/// Outside a transaction a statement's transaction is the statement itself, so `now()` (the
/// transaction's start) equals `statement_timestamp()`; inside one they differ. The id check
/// catches a transaction that has written something, too. It goes as a simple query, one
/// message: the extended protocol's Parse, Bind and Execute each move `statement_timestamp()`
/// on, so the two would differ outside a transaction too.
const IDLE: &str =
    "SELECT now() = statement_timestamp() AND pg_current_xact_id_if_assigned() IS NULL";

/// The pool's `after_release` hook: `true` keeps the connection, `false` closes it. A
/// connection inside a transaction, or one the check can't run on (an aborted transaction
/// refuses every statement), is closed, counted and reported once, with no SQL in the report.
pub fn release_check(
    conn: &mut PgConnection,
    _meta: PoolConnectionMetadata,
) -> Pin<Box<dyn Future<Output = Result<bool, sqlx::Error>> + Send + '_>> {
    let checked = CHECK_EVERY_RELEASE.load(Ordering::Relaxed)
        || RELEASES
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(CANARY_EVERY);
    Box::pin(async move {
        if !checked {
            return Ok(true);
        }
        CHECKS.fetch_add(1, Ordering::Relaxed);
        let idle = sqlx::raw_sql(IDLE)
            .fetch_one(&mut *conn)
            .await
            .ok()
            .and_then(|row| sqlx::Row::try_get::<Option<bool>, _>(&row, 0).ok())
            .flatten()
            .unwrap_or(false);
        if !idle {
            let closed = DIRTY.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::error!(
                closed,
                "a pooled connection came back inside a transaction; closed it (D-217)"
            );
        }
        Ok(idle)
    })
}
