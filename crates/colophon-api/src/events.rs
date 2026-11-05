//! Live change hints (D-108, docs/modules/04-api.md `{P}/events`; step E3): `GET {P}/events`
//! holds a server-sent-events stream that says *what* changed, so lists, drafts and review
//! screens can refetch instead of polling.
//!
//! The chain: migration 0044's row triggers `pg_notify` a compact payload on `colophon_events`
//! only when a transaction commits → the server's own listener connection (one per API
//! process, spawned in `colophon-server`) decodes it into a [`Hint`] and calls
//! [`EventBus::publish`] → the hint fans out on that project's `tokio::broadcast` channel to
//! every open stream of it, which forwards it as a `change` event.
//!
//! Hints carry no content — a kind, an environment and a collection — so a client learns only
//! that it should re-read through the API it was already authorized for. Ordering is best-effort
//! commit order and nothing is replayed: a stream that fell behind (its broadcast queue
//! overflowed, or the listener reconnected and Postgres may have skipped payloads) gets one
//! `resync` event instead, after which the client reloads. `Last-Event-ID` is accepted and
//! ignored for the same reason.
//!
//! Bounds (invariant 4; `COLOPHON_EVENTS_*`): streams per credential, per project on this
//! server, and on this server in all are capped *before* subscribing (429 / 503), each channel
//! buffers at most `channel_capacity` frames, a stream lives at most `lifetime`, a comment line
//! keeps the connection warm every `keep_alive`, and every `revalidate` the membership check
//! runs again so a removed member's or a revoked token's stream dies instead of leaking hints.

use std::{
    collections::HashMap,
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use axum::{
    extract::{Path, State},
    http::{HeaderValue, header::HeaderName},
    response::{IntoResponse, Response, Sse, sse::Event, sse::KeepAlive},
};
use colophon_db::events::{Hint, Kind};
use futures_util::{Stream, StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tokio::time::Instant;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{self},
    session::{SignedIn, services},
};

/// How the streams are bounded (`COLOPHON_EVENTS_*`; D-108).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLimits {
    /// Streams one credential — a session or a token — may hold on this server.
    pub per_credential: usize,
    /// Streams of one project this server carries.
    pub per_project: usize,
    /// Streams this server carries in all.
    pub max_streams: usize,
    /// Buffered frames per project channel before a lagging stream is told to resync.
    pub channel_capacity: usize,
    /// How often a comment line keeps an idle stream warm through proxies.
    pub keep_alive: Duration,
    /// The longest one stream lives; the client reconnects after it ends.
    pub lifetime: Duration,
    /// How often a live stream re-runs the membership check; `None` never re-checks.
    pub revalidate: Option<Duration>,
}

impl Default for EventLimits {
    /// The documented bounds: 8 streams per credential, 256 per project on one server, 2048 on a
    /// server in all, a 256-frame channel buffer, a 15-second keep-alive, a 30-minute lifetime
    /// and a 5-minute membership re-check.
    fn default() -> Self {
        Self {
            per_credential: 8,
            per_project: 256,
            max_streams: 2048,
            channel_capacity: 256,
            keep_alive: Duration::from_secs(15),
            lifetime: Duration::from_mins(30),
            revalidate: Some(Duration::from_mins(5)),
        }
    }
}

/// What one project's channel carries to every stream on it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Frame {
    /// A committed change: a `change` event.
    Change(Hint),
    /// Something was missed; refetch rather than trust the last known state: `resync`.
    Resync,
}

/// Why a subscription was refused, mapped to the envelope by the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamRefused {
    /// The credential already holds `per_credential` streams: 429.
    Credential,
    /// This server already carries `per_project` streams of the project: 503.
    Project,
    /// This server already carries `max_streams` streams in all: 503.
    Server,
}

/// The fan-out: one `tokio::broadcast` channel per watched project, fed by the server's
/// `LISTEN` connection. A channel exists only while at least one stream watches it.
pub struct EventBus {
    limits: EventLimits,
    shared: Mutex<Shared>,
}

#[derive(Default)]
struct Shared {
    /// Live channels by project.
    channels: HashMap<Uuid, broadcast::Sender<Frame>>,
    /// Open streams per credential (a session or token id).
    per_credential: HashMap<Uuid, usize>,
    /// Open streams in all on this server.
    total: usize,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

impl EventBus {
    /// An empty bus under `limits`.
    pub fn new(limits: EventLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            shared: Mutex::new(Shared::default()),
        })
    }

    /// The configured bounds, for the handler's keep-alive and stream limits.
    pub fn limits(&self) -> EventLimits {
        self.limits
    }

    /// A committed change, to every stream watching `hint.project`. Cheap when nobody does:
    /// there is no channel to send on.
    pub fn publish(&self, hint: Hint) {
        let shared = lock(&self.shared);
        if let Some(sender) = shared.channels.get(&hint.project) {
            // A channel with no receivers is removed when the last subscription drops, so a
            // send error can only mean the last receiver is dropping right now.
            let _sent = sender.send(Frame::Change(hint));
        }
    }

    /// Tells every open stream on this server that hints may have been missed (the listener
    /// reconnected and Postgres delivers nothing retroactively): `resync`, never a replay.
    pub fn resync(&self) {
        let shared = lock(&self.shared);
        for sender in shared.channels.values() {
            let _sent = sender.send(Frame::Resync);
        }
    }

    /// A stream on `project` for `credential`, refused over the caps *before* subscribing.
    /// Dropping the returned [`Subscription`] gives the slot back and tears the channel down
    /// when it was the project's last stream.
    fn subscribe(
        self: &Arc<Self>,
        project: Uuid,
        credential: Uuid,
    ) -> Result<Subscription, StreamRefused> {
        self.subscribe_as(project, Some(credential))
    }

    /// A public delivery stream (D-139): no credential to count, still bounded per project and
    /// per server, so anonymous listeners can never crowd out a project's staff streams.
    fn subscribe_public(self: &Arc<Self>, project: Uuid) -> Result<Subscription, StreamRefused> {
        self.subscribe_as(project, None)
    }

    fn subscribe_as(
        self: &Arc<Self>,
        project: Uuid,
        credential: Option<Uuid>,
    ) -> Result<Subscription, StreamRefused> {
        let mut shared = lock(&self.shared);
        if let Some(credential) = credential
            && shared.per_credential.get(&credential).copied().unwrap_or(0)
                >= self.limits.per_credential
        {
            return Err(StreamRefused::Credential);
        }
        if shared.total >= self.limits.max_streams {
            return Err(StreamRefused::Server);
        }
        let sender = shared
            .channels
            .entry(project)
            .or_insert_with(|| broadcast::channel(self.limits.channel_capacity).0);
        if sender.receiver_count() >= self.limits.per_project {
            return Err(StreamRefused::Project);
        }
        let receiver = sender.subscribe();
        if let Some(credential) = credential {
            *shared.per_credential.entry(credential).or_insert(0) += 1;
        }
        shared.total += 1;
        Ok(Subscription {
            receiver,
            bus: Arc::clone(self),
            project,
            credential,
        })
    }

    /// Open streams in all, for the tests.
    #[cfg(test)]
    fn streams(&self) -> usize {
        lock(&self.shared).total
    }
}

/// One open stream: a receiver on the project's channel plus the bookkeeping that returns its
/// slot on drop.
struct Subscription {
    receiver: broadcast::Receiver<Frame>,
    bus: Arc<EventBus>,
    project: Uuid,
    /// The credential the stream is counted against; `None` for a public delivery stream.
    credential: Option<Uuid>,
}

impl Subscription {
    /// Whatever is already queued, without waiting: used to coalesce a burst into one frame.
    fn try_next(&mut self) -> Option<Frame> {
        match self.receiver.try_recv() {
            Ok(frame) => Some(frame),
            // A lagging receiver's backlog is not replayed; the burst it missed is answered by
            // the frame the caller is about to send anyway.
            Err(broadcast::error::TryRecvError::Lagged(_)) => Some(Frame::Resync),
            Err(_) => None,
        }
    }

    /// The next frame; a lagging receiver answers one `Resync`, never a replay (D-108).
    async fn next(&mut self) -> Option<Frame> {
        match self.receiver.recv().await {
            Ok(frame) => Some(frame),
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Re-subscribing puts the cursor at the channel's tail: the retained backlog
                // is never delivered — it precedes the resync, and the client's refetch after
                // `resync` covers it. Anything published from here on arrives normally.
                self.receiver = self.receiver.resubscribe();
                Some(Frame::Resync)
            }
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut shared = lock(&self.bus.shared);
        if let Some(credential) = self.credential
            && let Some(count) = shared.per_credential.get_mut(&credential)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                shared.per_credential.remove(&credential);
            }
        }
        shared.total = shared.total.saturating_sub(1);
        // This receiver is still alive while `drop` runs, so a count of 1 means it was the
        // channel's last: once the map forgets the sender nobody can subscribe to it again.
        if shared
            .channels
            .get(&self.project)
            .is_some_and(|sender| sender.receiver_count() <= 1)
        {
            shared.channels.remove(&self.project);
        }
    }
}

/// The `{P}` of `{P}/events`.
#[derive(Deserialize)]
pub(crate) struct EventsPath {
    project: String,
    environment: String,
}

/// `GET /content/v1/{project}/{environment}/events` (D-139): the public live-hint stream.
///
/// Anonymous, and deliberately bare: a frame says only that published content changed — no
/// ids, no kinds, no collections — so a gated entry's existence cannot leak through it. A site
/// reacts by re-reading through the delivery API it already reads, at the new generation.
///
/// Only `releases` hints are forwarded: a publish inserts a release row, while draft saves and
/// job/probe churn are `entries`/`releases`-adjacent noise a reader must not be woken for.
/// Bursts coalesce — a publish's several hints become one frame — and the stream is bounded by
/// the same per-project and per-server caps as the staff bus, so anonymous listeners can never
/// crowd out a project's editors.
pub(crate) async fn delivery_events(
    State(state): State<AppState>,
    Path(path): Path<EventsPath>,
) -> Result<Response, ApiError> {
    let services = services(&state)?;
    let project = Uuid::try_parse(&path.project).map_err(|_| ApiError::NotFound)?;
    // The environment must resolve exactly as a delivery read would, so an unknown project or
    // slug answers 404 rather than holding a stream open.
    let environment =
        colophon_db::delivery::environment(&services.pool, project, &path.environment)
            .await
            .map_err(ApiError::from_db)?
            .ok_or(ApiError::NotFound)?;
    let subscription = services.events.subscribe_public(project).map_err(|_| {
        ApiError::Unavailable("this server is carrying as many live streams as it can")
    })?;

    let limits = services.events.limits();
    let frames = public_frames(subscription, environment.id, limits).map(Ok::<_, Infallible>);

    let mut response = Sse::new(frames)
        .keep_alive(
            KeepAlive::new()
                .interval(limits.keep_alive)
                .text("keep-alive"),
        )
        .into_response();
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    // A live stream is never a cacheable read.
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

/// The public stream: `hello`, then one `change` per burst of release hints for this
/// environment; ends at the bus's lifetime.
fn public_frames(
    subscription: Subscription,
    environment: Uuid,
    limits: EventLimits,
) -> impl Stream<Item = Event> + Send + 'static {
    struct Live {
        subscription: Subscription,
        environment: Uuid,
        ends_at: Instant,
    }

    let live = Live {
        subscription,
        environment,
        ends_at: Instant::now() + limits.lifetime,
    };

    let changes = stream::unfold(live, |mut live| async move {
        loop {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(live.ends_at) => return None,
                frame = live.subscription.next() => match frame {
                    Some(Frame::Change(hint)) => {
                        if !is_release_for(&hint, live.environment) {
                            continue;
                        }
                        // A publish commits several hints (the release, its jobs, probes):
                        // drain whatever is already queued and answer one frame for the burst.
                        while live.subscription.try_next().is_some() {}
                        return Some((public_change(), live));
                    }
                    // A missed hint means published content may have moved: tell the client to
                    // re-read, which is all the public stream ever says.
                    Some(Frame::Resync) => return Some((public_change(), live)),
                    None => return None,
                },
            }
        }
    });
    stream::once(async { Out::Hello.into_event() }).chain(changes)
}

/// A release hint in this stream's environment (or one with no environment at all).
fn is_release_for(hint: &Hint, environment: Uuid) -> bool {
    hint.kind == Kind::Releases && hint.environment.is_none_or(|id| id == environment)
}

/// The public `change` frame: no payload beyond "something published changed".
fn public_change() -> Event {
    Event::default().event("change").data("{}")
}

/// `GET {P}/events`: a server-sent-events stream of change hints (D-108). Any credential the
/// extractor accepts — cookie session, personal token or a project API token, which the
/// `environments/` path rule admits — may listen as a member of the project in this
/// environment, capped per credential, per project and per server before subscribing. The
/// stream sends `hello`, then `change` hints (`{"k","c"?,"e"?}`) and `resync`; it ends cleanly
/// at `lifetime` and dies when a periodic re-check finds the membership gone.
pub(crate) async fn events(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<EventsPath>,
) -> Result<Response, ApiError> {
    let tx = entries::open(&state, caller, &path.project, &path.environment).await?;
    let (project, environment) = (tx.project, tx.environment);
    tx.commit().await.map_err(ApiError::from_db)?;
    let services = services(&state)?;
    let subscription = services
        .events
        .subscribe(project, caller.session)
        .map_err(|refused| match refused {
            StreamRefused::Credential => ApiError::TooManyRequests { retry_after: 30 },
            _ => ApiError::Unavailable("this server is carrying as many live streams as it can"),
        })?;

    let limits = services.events.limits();
    let still_member = {
        let state = state.clone();
        move || {
            let (state, project, environment) = (
                state.clone(),
                path.project.clone(),
                path.environment.clone(),
            );
            Box::pin(async move { still_member(&state, caller, &project, &environment).await })
                as Pin<Box<dyn Future<Output = bool> + Send>>
        }
    };
    let frames = frames(subscription, environment, limits, Box::new(still_member))
        .map(|frame| Ok::<_, Infallible>(frame.into_event()));

    let mut response = Sse::new(frames)
        .keep_alive(
            KeepAlive::new()
                .interval(limits.keep_alive)
                .text("keep-alive"),
        )
        .into_response();
    // Ask any buffering proxy (nginx's default) not to hold events back.
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    Ok(response)
}

/// The membership check again: still a member of the project in this environment (or the API
/// token still valid for it). Any failure ends the stream — Postgres is the authority for who
/// may listen, and a check that can't run can't vouch for the stream.
async fn still_member(
    state: &AppState,
    caller: SignedIn,
    project: &str,
    environment: &str,
) -> bool {
    entries::open(state, caller, project, environment)
        .await
        .is_ok()
}

/// Re-checks membership; boxed so the stream loop stays concrete.
type StillMember = Box<dyn FnMut() -> Pin<Box<dyn Future<Output = bool> + Send>> + Send>;

/// One item the stream sends.
#[derive(Debug, PartialEq, Eq)]
enum Out {
    /// `hello`, first: the stream is up and the client's refetches can start.
    Hello,
    /// `change` with `{"k","c"?,"e"?}`.
    Change(Hint),
    /// `resync`: hints may have been missed; reload.
    Resync,
}

impl Out {
    fn into_event(self) -> Event {
        match self {
            Self::Hello => Event::default().event("hello").data("{}"),
            Self::Change(hint) => change(&hint),
            Self::Resync => Event::default().event("resync").data("{}"),
        }
    }
}

/// The stream behind [`Sse`]: `hello`, then `change`/`resync` until the subscription ends, the
/// `lifetime` deadline passes, or a re-check finds the membership gone. Environment-scoped
/// hints reach only the streams of their own environment; project-wide ones reach every stream
/// of the project.
fn frames(
    subscription: Subscription,
    environment: Uuid,
    limits: EventLimits,
    revalidate: StillMember,
) -> impl Stream<Item = Out> + Send + 'static {
    struct Live {
        subscription: Subscription,
        /// The stream's environment.
        environment: Uuid,
        /// When the whole stream ends.
        ends_at: Instant,
        /// The re-check's interval, next deadline and check; `None` never re-checks.
        revalidate: Option<(Duration, Instant, StillMember)>,
    }

    let live = Live {
        subscription,
        environment,
        ends_at: Instant::now() + limits.lifetime,
        revalidate: limits
            .revalidate
            .map(|every| (every, Instant::now() + every, revalidate)),
    };

    let changes = stream::unfold(live, |mut live| async move {
        loop {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(live.ends_at) => return None,
                () = until(live.revalidate.as_ref().map(|(_, at, _)| *at)) => {
                    if let Some((every, at, still_member)) = live.revalidate.as_mut() {
                        if !still_member().await {
                            return None;
                        }
                        *at = Instant::now() + *every;
                    }
                }
                frame = live.subscription.next() => match frame {
                    Some(Frame::Change(hint)) => {
                        if hint.environment.is_none_or(|id| id == live.environment) {
                            return Some((Out::Change(hint), live));
                        }
                    }
                    Some(Frame::Resync) => return Some((Out::Resync, live)),
                    None => return None,
                },
            }
        }
    });
    stream::once(async { Out::Hello }).chain(changes)
}

/// `sleep_until`, or never when there is no deadline.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

/// A `change` event's data: `{"k":"<kind>","c":"<collection>"?,"e":"<environment>"?}` — a hint
/// only, so the client refetches through the API its permissions already cover.
fn change(hint: &Hint) -> Event {
    let mut data = json!({ "k": hint.kind.as_str() });
    if let Some(collection) = &hint.collection {
        data["c"] = json!(collection);
    }
    if let Some(environment) = hint.environment {
        data["e"] = json!(environment);
    }
    Event::default().event("change").data(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use colophon_db::events::Kind;

    fn limits() -> EventLimits {
        EventLimits {
            per_credential: 2,
            per_project: 3,
            max_streams: 4,
            channel_capacity: 4,
            keep_alive: Duration::from_secs(15),
            lifetime: Duration::from_mins(30),
            revalidate: None,
        }
    }

    fn hint(project: Uuid, environment: Option<Uuid>, kind: Kind) -> Hint {
        Hint {
            project,
            environment,
            kind,
            collection: None,
        }
    }

    fn member(keep: bool) -> StillMember {
        Box::new(move || Box::pin(async move { keep }))
    }

    #[tokio::test]
    async fn a_hint_fans_out_to_the_projects_streams() {
        let bus = EventBus::new(limits());
        let (project, other) = (Uuid::now_v7(), Uuid::now_v7());
        let mut one = bus.subscribe(project, Uuid::now_v7()).expect("room");
        let mut two = bus.subscribe(project, Uuid::now_v7()).expect("room");
        let mut elsewhere = bus.subscribe(other, Uuid::now_v7()).expect("room");

        let change = hint(project, None, Kind::Members);
        bus.publish(change.clone());

        assert_eq!(one.next().await, Some(Frame::Change(change.clone())));
        assert_eq!(two.next().await, Some(Frame::Change(change.clone())));
        // Another project hears nothing of it — nothing arrives to unblock the check.
        assert!(
            tokio::time::timeout(Duration::from_millis(20), elsewhere.next())
                .await
                .is_err(),
            "hints stay inside their project"
        );
    }

    #[tokio::test]
    async fn the_public_stream_answers_only_release_hints() {
        let bus = EventBus::new(limits());
        let project = Uuid::now_v7();
        let environment = Uuid::now_v7();
        let subscription = bus.subscribe_public(project).expect("room");
        let mut frames = Box::pin(public_frames(subscription, environment, limits()));
        assert!(frames.next().await.is_some(), "hello comes first");

        // A draft save is an `entries` hint: a reader must not be woken for it.
        bus.publish(hint(project, Some(environment), Kind::Entries));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), frames.next())
                .await
                .is_err(),
            "draft saves stay off the public stream"
        );

        // A publish inserts a release: that is the signal.
        bus.publish(hint(project, Some(environment), Kind::Releases));
        let frame = tokio::time::timeout(Duration::from_millis(200), frames.next())
            .await
            .expect("a release wakes the stream")
            .expect("a frame");
        assert!(
            format!("{frame:?}").contains("change"),
            "the frame is a change"
        );
    }

    #[tokio::test]
    async fn the_public_stream_ignores_other_environments() {
        let bus = EventBus::new(limits());
        let project = Uuid::now_v7();
        let (here, elsewhere) = (Uuid::now_v7(), Uuid::now_v7());
        let subscription = bus.subscribe_public(project).expect("room");
        let mut frames = Box::pin(public_frames(subscription, here, limits()));
        assert!(frames.next().await.is_some(), "hello comes first");
        bus.publish(hint(project, Some(elsewhere), Kind::Releases));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), frames.next())
                .await
                .is_err(),
            "another environment's release stays off this stream"
        );
        bus.publish(hint(project, Some(here), Kind::Releases));
        assert!(
            tokio::time::timeout(Duration::from_millis(200), frames.next())
                .await
                .is_ok(),
            "this environment's release wakes the stream"
        );
    }

    #[tokio::test]
    async fn public_streams_are_capped_per_project() {
        let bus = EventBus::new(limits());
        let project = Uuid::now_v7();
        let held: Vec<_> = (0..limits().per_project)
            .map(|_| bus.subscribe_public(project).expect("room"))
            .collect();
        assert_eq!(
            bus.subscribe_public(project).err(),
            Some(StreamRefused::Project),
            "anonymous streams cannot exceed the project's cap"
        );
        drop(held);
        assert_eq!(bus.streams(), 0);
    }

    #[tokio::test]
    async fn the_public_change_frame_carries_nothing() {
        // The frame's data is exactly `{}`: no ids, kinds or collections can leak through it.
        let frame = public_change();
        assert!(format!("{frame:?}").contains("{}"));
    }

    #[tokio::test]
    async fn resync_reaches_every_stream() {
        let bus = EventBus::new(limits());
        let mut one = bus.subscribe(Uuid::now_v7(), Uuid::now_v7()).expect("room");
        let mut two = bus.subscribe(Uuid::now_v7(), Uuid::now_v7()).expect("room");
        bus.resync();
        assert_eq!(one.next().await, Some(Frame::Resync));
        assert_eq!(two.next().await, Some(Frame::Resync));
    }

    #[tokio::test]
    async fn caps_refuse_before_subscribing() {
        let bus = EventBus::new(limits());
        let credential = Uuid::now_v7();
        let held = [
            bus.subscribe(Uuid::now_v7(), credential).expect("first"),
            bus.subscribe(Uuid::now_v7(), credential).expect("second"),
        ];
        assert_eq!(
            bus.subscribe(Uuid::now_v7(), credential).err(),
            Some(StreamRefused::Credential),
            "a third stream on one credential is refused"
        );
        drop(held);
        assert_eq!(bus.streams(), 0, "dropping returns the slots");

        // Per project: three on one project, the fourth refused — but another project takes it.
        let project = Uuid::now_v7();
        let held: Vec<_> = (0..3)
            .map(|_| {
                bus.subscribe(project, Uuid::now_v7())
                    .expect("room in the project")
            })
            .collect();
        assert_eq!(
            bus.subscribe(project, Uuid::now_v7()).err(),
            Some(StreamRefused::Project)
        );
        let fourth = bus
            .subscribe(Uuid::now_v7(), Uuid::now_v7())
            .expect("another project has room");
        assert_eq!(
            bus.subscribe(Uuid::now_v7(), Uuid::now_v7()).err(),
            Some(StreamRefused::Server),
            "a fifth would be over the server's 4"
        );
        drop(held);
        drop(fourth);
        assert_eq!(bus.streams(), 0, "every slot is returned");
        assert!(
            lock(&bus.shared).channels.is_empty(),
            "no channel outlives its last stream"
        );
    }

    #[tokio::test]
    async fn a_lagging_stream_gets_one_resync_not_a_replay() {
        let bus = EventBus::new(limits()); // capacity 4
        let project = Uuid::now_v7();
        let mut subscription = bus.subscribe(project, Uuid::now_v7()).expect("room");
        for _ in 0..8 {
            bus.publish(hint(project, None, Kind::Members));
        }
        assert_eq!(subscription.next().await, Some(Frame::Resync));
        // Then it keeps receiving what arrives after the gap.
        bus.publish(hint(project, None, Kind::Roles));
        assert_eq!(
            subscription.next().await,
            Some(Frame::Change(hint(project, None, Kind::Roles)))
        );
    }

    #[tokio::test]
    async fn the_stream_says_hello_then_hints_and_resyncs() {
        let bus = EventBus::new(limits());
        let (project, environment) = (Uuid::now_v7(), Uuid::now_v7());
        let subscription = bus.subscribe(project, Uuid::now_v7()).expect("room");
        let mut stream = std::pin::pin!(frames(
            subscription,
            environment,
            bus.limits(),
            member(true)
        ));
        assert_eq!(stream.next().await, Some(Out::Hello));

        // Env-scoped hints only for this stream's environment; project-wide ones always.
        bus.publish(Hint {
            project,
            environment: Some(Uuid::now_v7()),
            kind: Kind::Entries,
            collection: Some("articles".to_owned()),
        });
        let own = Hint {
            project,
            environment: Some(environment),
            kind: Kind::Entries,
            collection: Some("articles".to_owned()),
        };
        bus.publish(own.clone());
        bus.publish(hint(project, None, Kind::Members));

        assert_eq!(stream.next().await, Some(Out::Change(own)));
        assert_eq!(
            stream.next().await,
            Some(Out::Change(hint(project, None, Kind::Members))),
            "the other environment's hint was skipped, the project-wide one arrives"
        );
        bus.resync();
        assert_eq!(stream.next().await, Some(Out::Resync));
    }

    #[tokio::test(start_paused = true)]
    async fn the_stream_ends_at_its_lifetime() {
        let bus = EventBus::new(EventLimits {
            lifetime: Duration::from_secs(60),
            ..limits()
        });
        let subscription = bus.subscribe(Uuid::now_v7(), Uuid::now_v7()).expect("room");
        let mut stream = std::pin::pin!(frames(
            subscription,
            Uuid::now_v7(),
            bus.limits(),
            member(true)
        ));
        assert_eq!(stream.next().await, Some(Out::Hello));
        tokio::time::advance(Duration::from_secs(120)).await;
        assert_eq!(stream.next().await, None, "the lifetime ends it");
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_membership_ends_the_stream_at_the_next_check() {
        let bus = EventBus::new(EventLimits {
            revalidate: Some(Duration::from_secs(60)),
            ..limits()
        });
        let subscription = bus.subscribe(Uuid::now_v7(), Uuid::now_v7()).expect("room");
        let mut stream = std::pin::pin!(frames(
            subscription,
            Uuid::now_v7(),
            bus.limits(),
            member(false)
        ));
        assert_eq!(stream.next().await, Some(Out::Hello));
        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(stream.next().await, None, "the re-check ends it");
    }
}
