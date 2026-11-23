//! Admission: rate limits per client address and per credential, shared by every server through
//! Valkey (milestone S, D-076); and, on the same machinery, the per-token request quota a
//! `requests_per_minute` ceiling asks for (D-118).
//!
//! Each limit is a generic cell rate algorithm (GCRA): a key's "theoretical arrival time" moves
//! one interval (a second over the rate) per admitted request, and a request is admitted while
//! that time is no more than the burst's worth of intervals ahead of now. One Valkey script checks
//! every limit a request counts against and moves them only if all admit it, on Valkey's clock, so
//! every server shares one count. When Valkey is down or slow, each server applies the same limits
//! on its own (looser across a cluster, never unlimited), and Valkey being down never refuses a
//! request by itself.
//!
//! What counts: the management API (`/api/`, `/auth/`) per client address and per credential
//! (a session cookie or bearer token, hashed: the secret never reaches Valkey), and public
//! delivery (`/content/`) per client address at its own rate. Health, version and the Studio's
//! files don't count. The client address is the connection's peer, or, when the peer is a trusted
//! proxy, the nearest address in `X-Forwarded-For` that isn't one.
//!
//! Beside those, the token quota (`colophon:quota:` keys): an API token's own
//! `requests_per_minute`, checked where its scoped transaction opens — the ceiling comes from
//! Postgres, the only authority for it, while the rolling count lives here like any other
//! cell's. It is a per-credential limit too, but its rate is the token's, configured by the
//! project, not the operator's floor the shaping limits keep.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use uuid::Uuid;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    ApiError, AppState,
    entries::{content_hash, sha256},
    valkey::{DEADLINE, Valkey},
};

/// A rate: requests a second, and how many may come at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// Sustained requests a second, at least 1.
    pub per_second: u32,
    /// Requests that may arrive together, at least 1.
    pub burst: u32,
}

/// A rate setting that isn't `off`, `<per second>` or `<per second>/<burst>` with both from 1
/// to 1,000,000.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "a rate is `off`, `<requests a second>` or `<requests a second>/<burst>`, each 1 to 1000000"
)]
pub struct InvalidRate;

impl Rate {
    /// Parses `off` (no limit), `<per second>` (a burst of twice that) or `<per second>/<burst>`.
    pub fn parse(text: &str) -> Result<Option<Self>, InvalidRate> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("off") {
            return Ok(None);
        }
        let number = |part: &str| {
            part.trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=1_000_000).contains(n))
                .ok_or(InvalidRate)
        };
        let (per_second, burst) = if let Some((rate, burst)) = text.split_once('/') {
            (number(rate)?, number(burst)?)
        } else {
            let rate = number(text)?;
            (rate, rate.saturating_mul(2))
        };
        Ok(Some(Self { per_second, burst }))
    }

    /// Microseconds between requests at the sustained rate.
    fn interval_us(self) -> u64 {
        (1_000_000 / u64::from(self.per_second)).max(1)
    }

    /// How far ahead of now a key's arrival time may be: the burst less one interval.
    fn tolerance_us(self) -> u64 {
        self.interval_us() * u64::from(self.burst.saturating_sub(1))
    }

    /// This rate as the cell the counter speaks.
    fn cell(self) -> Cell {
        Cell {
            interval_us: self.interval_us(),
            tolerance_us: self.tolerance_us(),
        }
    }
}

/// One GCRA cell: a sustained rate (one request every `interval_us`) and how far ahead of now
/// its arrival time may sit before the request waits (`tolerance_us`, always one burst less
/// one interval).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    interval_us: u64,
    tolerance_us: u64,
}

/// `requests` per `window_ms`, as a cell (D-118): the whole window's allowance may arrive at
/// once — the burst is the window's worth — then one request every `window/requests`. A
/// rolling window, so a boundary never serves two windows' worth the way a fixed one does.
fn quota_cell(requests: i64, window_ms: u64) -> Cell {
    let requests = u64::try_from(requests).unwrap_or(1).max(1);
    let interval_us = (window_ms.saturating_mul(1_000) / requests).max(1);
    Cell {
        interval_us,
        tolerance_us: interval_us.saturating_mul(requests.saturating_sub(1)),
    }
}

/// An address or a network: `10.0.0.0/8`, `127.0.0.1`, `::1`, `fd00::/8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpPrefix {
    network: IpAddr,
    bits: u8,
}

/// A trusted proxy that isn't an address or a network in CIDR form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a trusted proxy is an IP address or a network such as 10.0.0.0/8")]
pub struct InvalidPrefix;

impl IpPrefix {
    /// Parses an address, or a network with its prefix length.
    pub fn parse(text: &str) -> Result<Self, InvalidPrefix> {
        let text = text.trim();
        let (address, bits) = match text.split_once('/') {
            Some((address, bits)) => (address, Some(bits)),
            None => (text, None),
        };
        let network: IpAddr = address.parse().map_err(|_| InvalidPrefix)?;
        let most = if network.is_ipv4() { 32 } else { 128 };
        let bits = match bits {
            None => most,
            Some(bits) => bits
                .parse::<u8>()
                .ok()
                .filter(|bits| *bits <= most)
                .ok_or(InvalidPrefix)?,
        };
        Ok(Self { network, bits })
    }

    /// Whether `address` is in this network.
    pub fn contains(self, address: IpAddr) -> bool {
        let (network, address, most) = match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => (
                u128::from(network.to_bits()),
                u128::from(address.to_bits()),
                32,
            ),
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                (network.to_bits(), address.to_bits(), 128)
            }
            _ => return false,
        };
        let shift = most - u32::from(self.bits);
        shift >= most || network >> shift == address >> shift
    }
}

/// The limits, and whom to believe about client addresses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdmissionSettings {
    /// The management API, per client address.
    pub per_address: Option<Rate>,
    /// The management API, per session or token.
    pub per_credential: Option<Rate>,
    /// Public delivery, per client address.
    pub delivery_per_address: Option<Rate>,
    /// Password work — sign-in, sign-up, anything that reaches the Argon2 gate — per client
    /// address (D-213), on top of the management limits: one address can't keep every slot
    /// busy.
    pub password_per_address: Option<Rate>,
    /// Proxies whose `X-Forwarded-For` says who the client is.
    pub trusted_proxies: Vec<IpPrefix>,
    /// The quota's window in milliseconds (D-118): `requests_per_minute` is checked against
    /// this span — the minute it names, 60,000, unless a test shortens it
    /// (`COLOPHON_QUOTA_WINDOW_MS`). `0` means the minute.
    pub quota_window_ms: u64,
}

/// What a request is, for the limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Management,
    Delivery,
    /// A management request that does password work (D-213): its management limits, and the
    /// address's password limit as well.
    Password,
}

impl Class {
    /// The class of a `method` request to `path`; `None` if it counts against nothing.
    fn of(method: &Method, path: &str) -> Option<Self> {
        if *method == Method::POST && is_password_work(path) {
            Some(Self::Password)
        } else if path.starts_with("/content/") || path.starts_with("/media/") {
            // The media origin is delivery's (D-136): per address, at delivery's rate (D-231).
            Some(Self::Delivery)
        } else if path.starts_with("/api/")
            || path.starts_with("/auth/")
            || path.starts_with("/audience/")
            // Preview reads and exchanges (D-172): per address and per bearer, like the
            // management API — a preview token is a credential, not anonymous delivery.
            || path.starts_with("/preview/")
            // `/me` reads the session's identity from Postgres on every request (D-231).
            || path == "/me"
        {
            Some(Self::Management)
        } else {
            None
        }
    }
}

/// The routes that reach the password gate (D-150), as a `POST`: staff sign-in and sign-up, a
/// site's sign-up, sign-in and magic-link sign-in (which hashes when it makes the account),
/// and staff creating a site user.
fn is_password_work(path: &str) -> bool {
    if path == "/auth/signin" || path == "/auth/signup" {
        return true;
    }
    let mut parts = path.trim_start_matches('/').split('/');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some("audience"), Some("v1"), Some(_), Some("signup" | "signin"), None, _)
        | (Some("audience"), Some("v1"), Some(_), Some("magic-link"), Some(_), None) => true,
        (Some("api"), Some("v1"), Some("projects"), Some(_), Some("audience"), Some("users")) => {
            parts.next().is_none()
        }
        _ => false,
    }
}

/// The keys this process keeps on its own while Valkey is away. Past this many live keys, the
/// local limits start over: a flood of new addresses can't make a server run out of memory.
const MAX_LOCAL_KEYS: usize = 100_000;

/// GCRA over every limit a request counts against, in Valkey, on Valkey's clock (microseconds).
/// `KEYS` are the limits' keys; `ARGV` holds each one's interval and tolerance in turn. Returns
/// the microseconds to wait, or 0 after moving every key.
const SCRIPT: &str = r"
local time = redis.call('TIME')
local now = tonumber(time[1]) * 1000000 + tonumber(time[2])
local wait = 0
local next_arrival = {}
for i = 1, #KEYS do
  local interval = tonumber(ARGV[2 * i - 1])
  local tolerance = tonumber(ARGV[2 * i])
  local arrival = tonumber(redis.call('GET', KEYS[i])) or now
  if arrival < now then arrival = now end
  if arrival - tolerance > now then wait = math.max(wait, arrival - tolerance - now) end
  next_arrival[i] = arrival + interval
end
if wait > 0 then return wait end
for i = 1, #KEYS do
  redis.call('SET', KEYS[i], string.format('%.0f', next_arrival[i]), 'PX',
    math.ceil((next_arrival[i] - now) / 1000) + 1000)
end
return 0
";

/// How far ahead of now one key's arrival sits, on Valkey's clock (microseconds): what a
/// token's window currently spends, for its view's request usage (D-118). No cell is 0.
const AHEAD: &str = r"
local time = redis.call('TIME')
local now = tonumber(time[1]) * 1000000 + tonumber(time[2])
local arrival = tonumber(redis.call('GET', KEYS[1])) or now
if arrival < now then arrival = now end
return arrival - now
";

/// This process's own copy of the limits, for while Valkey is away.
struct Local {
    epoch: Instant,
    arrivals: HashMap<String, u64>,
}

impl Local {
    /// GCRA at `now` (microseconds since `epoch`): the wait, or `None` after moving every key.
    fn check(&mut self, now: u64, keys: &[(String, Cell)]) -> Option<u64> {
        if self.arrivals.len() >= MAX_LOCAL_KEYS {
            self.arrivals.retain(|_, arrival| *arrival > now);
            if self.arrivals.len() >= MAX_LOCAL_KEYS {
                self.arrivals.clear();
            }
        }
        let mut wait = 0;
        let mut next = Vec::with_capacity(keys.len());
        for (key, cell) in keys {
            let arrival = self.arrivals.get(key).copied().unwrap_or(now).max(now);
            let allowed_at = arrival.saturating_sub(cell.tolerance_us);
            if allowed_at > now {
                wait = wait.max(allowed_at - now);
            }
            next.push(arrival + cell.interval_us);
        }
        if wait > 0 {
            return Some(wait);
        }
        for ((key, _), arrival) in keys.iter().zip(next) {
            self.arrivals.insert(key.clone(), arrival);
        }
        None
    }

    /// How far ahead of `now` `key`'s arrival sits, for the usage a token view shows.
    fn ahead(&self, now: u64, key: &str) -> Option<u64> {
        self.arrivals
            .get(key)
            .map(|arrival| arrival.saturating_sub(now))
    }
}

/// The limits and where they are counted.
pub struct Admission {
    settings: AdmissionSettings,
    valkey: Valkey,
    local: Mutex<Local>,
    /// The quota window, milliseconds (`0` in the settings means the minute).
    quota_window_ms: u64,
    /// Tokens this process knows are throttled, until when: the transition the audit log
    /// records once an episode (D-118). Bounded the way [`Local`]'s arrivals are.
    throttled: Mutex<HashMap<Uuid, Instant>>,
    /// Tokens refused for their quota, by the hash of their secret, until the refusal's wait
    /// runs out (D-214): while one stands, the token's next request is answered from here and
    /// the limiter alone, before Postgres is asked anything. Refusal-only: an entry can refuse,
    /// never admit. At most [`MAX_REFUSED`] entries. An entry ends the moment the token's
    /// project's tokens, members or roles change ([`Self::tokens_changed`]).
    refused: Mutex<Memo>,
    /// Questions put to a limit's store (Valkey, or this process's own copy): the count a test
    /// reads to prove a remembered refusal costs one (D-214).
    queries: AtomicU64,
}

/// A token's quota refusal, remembered (D-214): what the token is, the ceiling Postgres gave it
/// when it was refused, until when the refusal can stand, and when the token expires (a
/// refusal never outlives the token, whose next request then hears 401).
#[derive(Debug, Clone, Copy)]
struct Refused {
    project: Uuid,
    token: Uuid,
    rpm: i64,
    until: Instant,
    expires: Instant,
}

/// The remembered refusals, and what ends them (D-214's addendum): a refusal is a judgement of
/// the token's row as it was read, so it stands only while nothing it was judged on has changed.
#[derive(Debug, Default)]
struct Memo {
    /// The refusals, by the hash of the token's secret.
    entries: HashMap<[u8; 32], Refused>,
    /// When this process last heard each project's tokens, members or roles change: a refusal
    /// judged on a row read before that is stale and is never remembered. Bounded like
    /// `entries`, and only changes newer than [`CHANGE_HORIZON`] are kept when it fills.
    changed: HashMap<Uuid, Instant>,
    /// Every judgement read before this is stale: set when changes may have been missed (the
    /// change-hint listener reconnected) or when `changed` overflowed.
    floor: Option<Instant>,
}

impl Memo {
    /// Whether a judgement of `project`'s token read at `read_at` may still be remembered.
    fn current(&self, project: Uuid, read_at: Instant) -> bool {
        self.floor.is_none_or(|floor| read_at > floor)
            && self.changed.get(&project).is_none_or(|at| read_at > *at)
    }
}

/// How long a change is kept to judge reads against once [`Memo`]'s change list is full: far
/// longer than any request is in flight (its timeout ends it sooner), so a read that old is
/// treated as stale by the floor instead.
const CHANGE_HORIZON: Duration = Duration::from_secs(600);

/// What a token's request was judged on, for remembering its refusal (D-214): the hash of its
/// secret, when its row was read, and when it expires. A refusal judged on a read older than
/// the project's last token change is not remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judged {
    /// The SHA-256 of the token's secret: what the refusal is remembered under.
    pub hash: [u8; 32],
    /// When the token's row was read (before Postgres was asked).
    pub read_at: Instant,
    /// When the token expires.
    pub expires: Instant,
}

/// How many refused tokens the memo holds (D-214). Each is a real token over a quota a project
/// set, so the cap is far above any honest count; past it, a refusal simply isn't remembered and
/// that token's requests take the full path, which refuses them as before.
pub const MAX_REFUSED: usize = 10_000;

impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Admission")
            .field("settings", &self.settings)
            .field("shared", &self.valkey.enabled())
            .finish_non_exhaustive()
    }
}

/// The Valkey URL was refused. Never includes the URL: it holds a password.
#[derive(Debug, thiserror::Error)]
#[error("COLOPHON_CACHE_URL is not a valid redis:// URL")]
pub struct InvalidValkeyUrl;

impl Admission {
    /// Limits counted in Valkey at `url`, or by this process alone without one.
    pub fn new(settings: AdmissionSettings, url: Option<&str>) -> Result<Self, InvalidValkeyUrl> {
        let valkey = match url {
            Some(url) => Valkey::new(url).ok_or(InvalidValkeyUrl)?,
            None => Valkey::disabled(),
        };
        let quota_window_ms = match settings.quota_window_ms {
            0 => 60_000,
            window => window,
        };
        Ok(Self {
            settings,
            valkey,
            local: Mutex::new(Local {
                epoch: Instant::now(),
                arrivals: HashMap::new(),
            }),
            quota_window_ms,
            throttled: Mutex::new(HashMap::new()),
            refused: Mutex::new(Memo::default()),
            queries: AtomicU64::new(0),
        })
    }

    /// The client's address: the peer, unless it is a trusted proxy, in which case the nearest
    /// address in `X-Forwarded-For` that isn't one. An entry that isn't an address ends the walk:
    /// whatever wrote it isn't believed.
    pub(crate) fn client(&self, peer: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
        let trusted = |address: IpAddr| {
            self.settings
                .trusted_proxies
                .iter()
                .any(|prefix| prefix.contains(address))
        };
        let mut client = peer?;
        if !trusted(client) {
            return Some(client);
        }
        let hops = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .collect::<Vec<_>>();
        for hop in hops.into_iter().rev() {
            let Ok(address) = hop.parse::<IpAddr>() else {
                break;
            };
            client = address;
            if !trusted(address) {
                break;
            }
        }
        Some(client)
    }

    /// The limits a request counts against, as Valkey keys with their cells.
    fn keys(&self, class: Class, peer: Option<IpAddr>, headers: &HeaderMap) -> Vec<(String, Cell)> {
        let mut keys = Vec::with_capacity(2);
        let (prefix, per_address) = match class {
            Class::Management | Class::Password => ("m", self.settings.per_address),
            Class::Delivery => ("d", self.settings.delivery_per_address),
        };
        if let (Some(rate), Some(client)) = (per_address, self.client(peer, headers)) {
            // IPv6 by its /64, as the sign-in keys are (D-162): one subscriber's block.
            let client = keyed_address(client);
            keys.push((format!("colophon:rl:{prefix}:a:{client}"), rate.cell()));
        }
        if class != Class::Delivery
            && let (Some(rate), Some(credential)) =
                (self.settings.per_credential, credential(headers))
        {
            keys.push((format!("colophon:rl:m:c:{credential}"), rate.cell()));
        }
        // Password work, per address (D-213), keyed like the sign-in throttle: IPv6 by its /64.
        if class == Class::Password
            && let (Some(rate), Some(client)) = (
                self.settings.password_per_address,
                self.client(peer, headers),
            )
        {
            keys.push((
                format!("colophon:rl:p:a:{}", keyed_address(client)),
                rate.cell(),
            ));
        }
        keys
    }

    /// The wait a request must take, in microseconds, or `None` if it is admitted.
    async fn check(&self, keys: &[(String, Cell)]) -> Option<u64> {
        if keys.is_empty() {
            return None;
        }
        self.queries.fetch_add(1, Ordering::Relaxed);
        if let Some(shared) = self.shared(keys).await {
            return shared;
        }
        let mut local = self.local.lock().unwrap_or_else(PoisonError::into_inner);
        let now = u64::try_from(local.epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        local.check(now, keys)
    }

    /// [`Self::check`] in Valkey; `None` if Valkey isn't there to ask.
    async fn shared(&self, keys: &[(String, Cell)]) -> Option<Option<u64>> {
        let mut connection = self.valkey.connection().await?;
        let mut command = redis::cmd("EVAL");
        command.arg(SCRIPT).arg(keys.len());
        for (key, _) in keys {
            command.arg(key);
        }
        for (_, cell) in keys {
            command.arg(cell.interval_us).arg(cell.tolerance_us);
        }
        if let Ok(Ok(wait)) =
            tokio::time::timeout(DEADLINE, command.query_async::<u64>(&mut connection)).await
        {
            Some((wait > 0).then_some(wait))
        } else {
            self.valkey.fail();
            None
        }
    }

    /// A token's own request ceiling (D-118): `rpm` requests a window — `rpm` at once, then
    /// one every `window/rpm` — in a cell keyed by project and token (`colophon:quota:`), on
    /// the same machinery and the same fallback as the shaping limits. `None`: admitted.
    pub(crate) async fn quota(&self, project: Uuid, token: Uuid, rpm: i64) -> Option<Throttle> {
        let keys = [(
            quota_key(project, token),
            quota_cell(rpm, self.quota_window_ms),
        )];
        let wait_us = self.check(&keys).await?;
        Some(Throttle {
            wait_us,
            transition: self.mark_throttled(token, wait_us),
        })
    }

    /// What a token's window currently spends, in requests: the cell's lead over now, in
    /// `rpm` units, for the token view's `usage.requests_last_minute`. `None`: the count
    /// isn't reachable — no cell in Valkey when it's there, none here when it isn't — and
    /// the view then keeps the ledger's 0.
    pub(crate) async fn quota_spent(&self, project: Uuid, token: Uuid, rpm: i64) -> Option<i64> {
        let ahead = self.ahead(&quota_key(project, token)).await?;
        let cell = quota_cell(rpm, self.quota_window_ms);
        let spent = i64::try_from(ahead.div_ceil(cell.interval_us)).unwrap_or(i64::MAX);
        Some(spent.min(rpm))
    }

    /// How far ahead of now `key`'s cell sits, in microseconds.
    async fn ahead(&self, key: &str) -> Option<u64> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        let mut connection = self.valkey.connection().await;
        if let Some(connection) = connection.as_mut() {
            let reply = tokio::time::timeout(
                DEADLINE,
                redis::cmd("EVAL")
                    .arg(AHEAD)
                    .arg(1)
                    .arg(key)
                    .query_async::<u64>(connection),
            )
            .await;
            return if let Ok(Ok(ahead)) = reply {
                Some(ahead)
            } else {
                self.valkey.fail();
                None
            };
        }
        let local = self.local.lock().unwrap_or_else(PoisonError::into_inner);
        let now = u64::try_from(local.epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        local.ahead(now, key)
    }

    /// Remembers that the token `judged` describes was refused for its quota (D-214), for
    /// `wait_us` or until it expires, whichever is sooner: until then [`Self::refused_again`]
    /// can answer its requests. Not remembered when the project's tokens changed after the
    /// token's row was read — the refusal judged a row that is no longer the token's — nor past
    /// [`MAX_REFUSED`] live entries (expired ones go first); either way the next request takes
    /// the full path, which judges the token as it is.
    pub(crate) fn remember_refusal(
        &self,
        judged: Judged,
        project: Uuid,
        token: Uuid,
        rpm: i64,
        wait_us: u64,
    ) {
        let mut memo = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        if !memo.current(project, judged.read_at) {
            return;
        }
        let now = Instant::now();
        let until = (now + Duration::from_micros(wait_us)).min(judged.expires);
        if until <= now {
            return;
        }
        let refused = &mut memo.entries;
        if refused.len() >= MAX_REFUSED && !refused.contains_key(&judged.hash) {
            refused.retain(|_, entry| entry.until > now);
            if refused.len() >= MAX_REFUSED {
                return;
            }
        }
        refused.insert(
            judged.hash,
            Refused {
                project,
                token,
                rpm,
                until,
                expires: judged.expires,
            },
        );
    }

    /// The tokens, members or roles of `project` changed (D-214's addendum): every refusal
    /// remembered for its tokens ends now, and a refusal judged on a row read before now is
    /// never remembered, so each token's next request is judged on its current state — a
    /// revoked token hears 401, a token whose ceiling was removed is admitted, one whose limits
    /// changed meets the new ones. The server that made the change calls this as it commits;
    /// every other server calls it when the change's hint arrives on `colophon_events`.
    pub fn tokens_changed(&self, project: Uuid) {
        let mut memo = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        memo.entries.retain(|_, entry| entry.project != project);
        let now = Instant::now();
        if memo.changed.len() >= MAX_REFUSED && !memo.changed.contains_key(&project) {
            // Older changes can only matter to reads older still, which the floor now covers.
            memo.changed
                .retain(|_, at| now.saturating_duration_since(*at) < CHANGE_HORIZON);
            let horizon = now.checked_sub(CHANGE_HORIZON).unwrap_or(now);
            memo.floor = Some(memo.floor.map_or(horizon, |floor| floor.max(horizon)));
            if memo.changed.len() >= MAX_REFUSED {
                memo.changed.clear();
                memo.floor = Some(now);
            }
        }
        memo.changed.insert(project, now);
    }

    /// Changes may have been missed (the change-hint listener reconnected, and Postgres delivers
    /// nothing retroactively): every remembered refusal ends, and none judged before now is
    /// remembered (D-214's addendum).
    pub fn forget_every_refusal(&self) {
        let mut memo = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        memo.entries.clear();
        memo.changed.clear();
        memo.floor = Some(Instant::now());
    }

    /// A remembered refusal that still stands for this request (D-214): `Some` when the token
    /// whose secret hashes to `hash` was refused for its quota, the refusal hasn't run out, the
    /// request is one the quota counts (`path` is in the token's own project's environments, as
    /// [`crate::entries::open`]'s requests are), and the limiter, asked without spending
    /// anything, says the window is still full. One question to the limiter; never Postgres.
    ///
    /// `None` sends the request down the full path: no memo, an expired one (forgotten), a path
    /// the quota doesn't count, a window with room again (forgotten, so the full path spends
    /// it), or a limiter that can't be asked (the full path has the same fallback as ever).
    pub(crate) async fn refused_again(&self, hash: &[u8; 32], path: &str) -> Option<Throttle> {
        let entry = {
            let mut memo = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = *memo.entries.get(hash)?;
            if entry.until <= Instant::now() {
                memo.entries.remove(hash);
                return None;
            }
            entry
        };
        let counted = path
            .strip_prefix(&format!("/api/v1/projects/{}/environments/", entry.project))
            .is_some_and(|rest| !rest.ends_with("/schedule"));
        if !counted {
            return None;
        }
        let cell = quota_cell(entry.rpm, self.quota_window_ms);
        let over = self
            .ahead(&quota_key(entry.project, entry.token))
            .await
            .and_then(|ahead| ahead.checked_sub(cell.tolerance_us))
            .filter(|wait| *wait > 0);
        let mut memo = self.refused.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(wait_us) = over else {
            memo.entries.remove(hash);
            return None;
        };
        // Only the entry this answer was judged on is refreshed: one a change ended meanwhile
        // stays ended (its token's next request takes the full path).
        if let Some(stored) = memo.entries.get_mut(hash) {
            stored.until = (Instant::now() + Duration::from_micros(wait_us)).min(stored.expires);
        }
        drop(memo);
        // The episode goes on: keep its marker with it, so the audit log still hears it once.
        self.mark_throttled(entry.token, wait_us);
        Some(Throttle {
            wait_us,
            transition: false,
        })
    }

    /// How many refusals the memo holds (D-214's bound, for tests).
    #[doc(hidden)]
    pub fn refused_len(&self) -> usize {
        self.refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .len()
    }

    /// How many questions this process has put to a limit's store (D-214, for tests).
    #[doc(hidden)]
    pub fn queries(&self) -> u64 {
        self.queries.load(Ordering::Relaxed)
    }

    /// Spends `token`'s quota until it refuses, then remembers the refusal under `hash`, as a
    /// refused request does; the refusal's `Retry-After` (D-214, for tests that start from a
    /// throttled token without a database).
    #[doc(hidden)]
    pub async fn throttle_for_tests(
        &self,
        hash: [u8; 32],
        project: Uuid,
        token: Uuid,
        rpm: i64,
    ) -> u32 {
        let read_at = Instant::now();
        loop {
            if let Some(throttle) = self.quota(project, token, rpm).await {
                let judged = Judged {
                    hash,
                    read_at,
                    expires: Instant::now() + Duration::from_hours(24),
                };
                self.remember_refusal(judged, project, token, rpm, throttle.wait_us);
                return throttle.retry_after();
            }
        }
    }

    /// Whether this refusal starts a throttle episode (`true`) or continues one the marker
    /// already covers: each wait moves the marker out, so a token that stays refused keeps
    /// one entry — and the audit log hears the transition once, never per call (D-118).
    /// Bounded like [`Local`]'s arrivals.
    fn mark_throttled(&self, token: Uuid, wait_us: u64) -> bool {
        let mut throttled = self
            .throttled
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if throttled.len() >= MAX_LOCAL_KEYS {
            throttled.retain(|_, until| *until > now);
            if throttled.len() >= MAX_LOCAL_KEYS {
                throttled.clear();
            }
        }
        let known = throttled.get(&token).is_some_and(|until| *until > now);
        throttled.insert(token, now + Duration::from_micros(wait_us));
        !known
    }
}

/// The `colophon:quota:` key a token's window lives under (D-118).
fn quota_key(project: Uuid, token: Uuid) -> String {
    format!("colophon:quota:{project}:{token}")
}

/// A token over its own quota: what its 429 carries (D-118).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Throttle {
    /// Microseconds until a request would be admitted; the `Retry-After` basis.
    pub(crate) wait_us: u64,
    /// This refusal begins a throttle episode: worth a `token.rate_limited` audit event;
    /// while the episode lasts, refusals stay quiet.
    pub(crate) transition: bool,
}

impl Throttle {
    /// `wait_us` in whole seconds, rounded up — what `Retry-After` says, at least one.
    pub(crate) fn retry_after(self) -> u32 {
        u32::try_from(self.wait_us.div_ceil(1_000_000))
            .unwrap_or(u32::MAX)
            .max(1)
    }
}

/// The request's credential, hashed: its session cookie or bearer token. Malformed ones count
/// too: a made-up token is still one client's.
fn credential(headers: &HeaderMap) -> Option<String> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let session = || {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .find_map(|pair| pair.trim().strip_prefix("colophon_session="))
    };
    let secret = bearer.or_else(session)?;
    let hash = content_hash(&sha256(secret.as_bytes()));
    Some(hash[..32].to_owned())
}

/// The middleware: refuses a request over a limit with 429 and `Retry-After`, before anything
/// else is done for it.
pub(crate) async fn admit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(admission) = state.admission.clone() else {
        return next.run(request).await;
    };
    let Some(class) = Class::of(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    let keys = admission.keys(class, peer, request.headers());
    match admission.check(&keys).await {
        None => next.run(request).await,
        Some(wait) => ApiError::TooManyRequests {
            retry_after: u32::try_from(wait.div_ceil(1_000_000))
                .unwrap_or(u32::MAX)
                .max(1),
        }
        .into_response(),
    }
}

/// Shared between the router and the server.
pub type SharedAdmission = Arc<Admission>;

/// The client a sign-in attempt is counted against (D-154): who admission believes sent the
/// request — the peer, or the nearest untrusted `X-Forwarded-For` hop behind a trusted proxy
/// — with an IPv6 address taken as its /64, the block one subscriber is usually handed, so
/// hopping within it doesn't start the count over. `-` when the server can't tell (an
/// in-process router with no connection info).
pub(crate) struct ClientAddress(pub(crate) String);

impl axum::extract::FromRequestParts<AppState> for ClientAddress {
    type Rejection = std::convert::Infallible;

    fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let client = match &state.admission {
            Some(admission) => admission.client(peer, &parts.headers),
            None => peer,
        };
        std::future::ready(Ok(Self(
            client.map_or_else(|| "-".to_owned(), keyed_address),
        )))
    }
}

/// An address as a sign-in key: IPv4 (and IPv4-mapped IPv6) whole, other IPv6 by its /64.
fn keyed_address(address: IpAddr) -> String {
    match address {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                v4.to_string()
            } else {
                let [a, b, c, d, ..] = v6.segments();
                format!("{a:x}:{b:x}:{c:x}:{d:x}::/64")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn sign_in_keys_take_ipv6_by_its_slash_64() {
        assert_eq!(
            keyed_address("198.51.100.7".parse().expect("v4")),
            "198.51.100.7"
        );
        assert_eq!(
            keyed_address("::ffff:198.51.100.7".parse().expect("mapped")),
            "198.51.100.7"
        );
        assert_eq!(
            keyed_address("2001:db8:1:2:aaaa::1".parse().expect("v6")),
            keyed_address("2001:db8:1:2:ffff::9".parse().expect("v6"))
        );
        assert_eq!(
            keyed_address("2001:db8:1:2::1".parse().expect("v6")),
            "2001:db8:1:2::/64"
        );
        assert_ne!(
            keyed_address("2001:db8:1:2::1".parse().expect("v6")),
            keyed_address("2001:db8:1:3::1".parse().expect("v6"))
        );
    }

    fn rate(per_second: u32, burst: u32) -> Rate {
        Rate { per_second, burst }
    }

    #[test]
    fn rates_parse_with_a_burst_or_off() {
        assert_eq!(Rate::parse("100/300"), Ok(Some(rate(100, 300))));
        assert_eq!(Rate::parse("50"), Ok(Some(rate(50, 100))));
        assert_eq!(Rate::parse(" OFF "), Ok(None));
        for bad in ["0", "-1", "1/0", "abc", "2000000", "1/", ""] {
            assert_eq!(Rate::parse(bad), Err(InvalidRate), "{bad:?}");
        }
    }

    #[test]
    fn a_burst_then_the_sustained_rate_then_a_wait() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        let keys = [("k".to_owned(), rate(10, 3).cell())];
        // Three at once, then the fourth waits one interval (100 ms).
        for _ in 0..3 {
            assert_eq!(local.check(0, &keys), None);
        }
        assert_eq!(local.check(0, &keys), Some(100_000));
        assert_eq!(local.check(99_999, &keys), Some(1));
        assert_eq!(
            local.check(100_000, &keys),
            None,
            "one more after an interval"
        );
        assert_eq!(local.check(100_000, &keys), Some(100_000));
        // Idle long enough, and the whole burst is back.
        for _ in 0..3 {
            assert_eq!(local.check(10_000_000, &keys), None);
        }
    }

    #[test]
    fn a_refusal_by_one_limit_costs_the_other_nothing() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        let tight = ("tight".to_owned(), rate(1, 1).cell());
        let loose = ("loose".to_owned(), rate(1, 2).cell());
        assert_eq!(local.check(0, &[tight.clone(), loose.clone()]), None);
        assert!(local.check(0, &[tight, loose.clone()]).is_some());
        assert_eq!(
            local.check(0, std::slice::from_ref(&loose)),
            None,
            "the loose one kept its second"
        );
        assert!(local.check(0, &[loose]).is_some());
    }

    #[test]
    fn a_flood_of_keys_never_grows_without_bound() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        for n in 0..(MAX_LOCAL_KEYS + 10) {
            assert_eq!(
                local.check(0, &[(format!("k{n}"), rate(1, 1).cell())]),
                None
            );
        }
        assert!(local.arrivals.len() <= MAX_LOCAL_KEYS);
    }

    #[test]
    fn prefixes_match_their_networks() {
        let lan = IpPrefix::parse("10.0.0.0/8").expect("valid");
        assert!(lan.contains("10.200.3.4".parse().expect("an address")));
        assert!(!lan.contains("11.0.0.1".parse().expect("an address")));
        assert!(!lan.contains("::1".parse().expect("an address")));
        let one = IpPrefix::parse("127.0.0.1").expect("valid");
        assert!(one.contains("127.0.0.1".parse().expect("an address")));
        assert!(!one.contains("127.0.0.2".parse().expect("an address")));
        let everything = IpPrefix::parse("0.0.0.0/0").expect("valid");
        assert!(everything.contains("8.8.8.8".parse().expect("an address")));
        assert!(
            IpPrefix::parse("fd00::/8")
                .expect("valid")
                .contains("fd12::1".parse().expect("an address"))
        );
        for bad in ["10.0.0.0/33", "nope", "::/129", ""] {
            assert_eq!(IpPrefix::parse(bad), Err(InvalidPrefix), "{bad:?}");
        }
    }

    #[test]
    fn the_client_is_believed_only_through_trusted_proxies() {
        let admission = Admission::new(
            AdmissionSettings {
                trusted_proxies: vec![IpPrefix::parse("10.0.0.0/8").expect("valid")],
                ..AdmissionSettings::default()
            },
            None,
        )
        .expect("no URL");
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, 9.9.9.9, 10.0.0.7"),
        );
        let proxy: IpAddr = "10.0.0.1".parse().expect("an address");
        let stranger: IpAddr = "5.6.7.8".parse().expect("an address");
        assert_eq!(
            admission.client(Some(stranger), &headers),
            Some(stranger),
            "a stranger's header is ignored"
        );
        assert_eq!(
            admission.client(Some(proxy), &headers),
            Some("9.9.9.9".parse().expect("an address")),
            "the nearest untrusted hop, not the first one, which the client wrote itself"
        );
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("1.2.3.4, junk, 10.0.0.7"),
        );
        assert_eq!(
            admission.client(Some(proxy), &headers),
            Some("10.0.0.7".parse().expect("an address")),
            "a hop that isn't an address ends the walk"
        );
        assert_eq!(admission.client(None, &headers), None);
    }

    /// The S0 droplet (deploy/droplet): Caddy tells the server one address, the visitor's
    /// Cf-Connecting-Ip, replacing any X-Forwarded-For that came in, and the server trusts loopback
    /// alone. Trusting Cloudflare's ranges here too (as S0 first did) let a request from a
    /// Cloudflare address, such as another account's Worker, have its own address skipped as "a
    /// proxy" and a forged one believed: a fresh address for every password attempt.
    #[test]
    fn behind_caddy_the_server_believes_caddys_one_address_and_no_forged_left_part() {
        let settings = |proxies: &[&str]| AdmissionSettings {
            trusted_proxies: proxies
                .iter()
                .map(|prefix| IpPrefix::parse(prefix).expect("valid"))
                .collect(),
            ..AdmissionSettings::default()
        };
        let caddy: IpAddr = "127.0.0.1".parse().expect("an address");
        let worker: IpAddr = "2a06:98c0:3600::103".parse().expect("an address");
        let droplet = Admission::new(settings(&["127.0.0.1/32"]), None).expect("no URL");

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("2a06:98c0:3600::103"),
        );
        assert_eq!(
            droplet.client(Some(caddy), &headers),
            Some(worker),
            "Caddy's one address is the client, a Cloudflare address included"
        );
        // Were a forged left part to reach the server anyway, it still isn't believed.
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("6.6.6.6, 2a06:98c0:3600::103"),
        );
        assert_eq!(droplet.client(Some(caddy), &headers), Some(worker));
        // A request that doesn't come through Caddy is its own peer, whatever it says.
        assert_eq!(droplet.client(Some(worker), &headers), Some(worker));

        // What trusting Cloudflare's ranges here did: the forged address was believed.
        let trusting_cloudflare =
            Admission::new(settings(&["127.0.0.1/32", "2a06:98c0::/29"]), None).expect("no URL");
        assert_eq!(
            trusting_cloudflare.client(Some(caddy), &headers),
            Some("6.6.6.6".parse().expect("an address")),
            "the reason the droplet's server trusts loopback alone"
        );
    }

    #[test]
    fn an_ipv6_client_is_limited_by_its_slash_64_like_a_sign_in_key() {
        // One subscriber is handed a /64; rotating inside it must not reset the address limit
        // (D-162), the same keying the sign-in throttle uses.
        let admission = Admission::new(
            AdmissionSettings {
                per_address: Rate::parse("10").expect("a rate"),
                ..AdmissionSettings::default()
            },
            None,
        )
        .expect("no URL");
        let headers = HeaderMap::new();
        let key = |address: &str| {
            admission.keys(
                Class::Management,
                Some(address.parse().expect("an address")),
                &headers,
            )[0]
            .0
            .clone()
        };
        assert_eq!(key("2001:db8:1:2::1"), key("2001:db8:1:2:ffff::9"));
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
        assert_eq!(key("198.51.100.7"), "colophon:rl:m:a:198.51.100.7");
    }

    #[test]
    fn credentials_are_counted_hashed_and_only_for_the_management_api() {
        let admission = Admission::new(
            AdmissionSettings {
                per_address: Some(rate(10, 10)),
                per_credential: Some(rate(5, 5)),
                delivery_per_address: Some(rate(100, 100)),
                password_per_address: Some(rate(5, 10)),
                trusted_proxies: Vec::new(),
                quota_window_ms: 0,
            },
            None,
        )
        .expect("no URL");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; colophon_session=s3cret"),
        );
        let peer = Some("127.0.0.1".parse().expect("an address"));
        let keys = admission.keys(Class::Management, peer, &headers);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, "colophon:rl:m:a:127.0.0.1");
        assert!(keys[1].0.starts_with("colophon:rl:m:c:") && !keys[1].0.contains("s3cret"));
        let delivery = admission.keys(Class::Delivery, peer, &headers);
        assert_eq!(
            delivery,
            [(
                "colophon:rl:d:a:127.0.0.1".to_owned(),
                rate(100, 100).cell()
            )]
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer colophon_api_x"),
        );
        assert_ne!(
            admission.keys(Class::Management, peer, &headers)[1].0,
            keys[1].0,
            "a token is its own credential"
        );
        assert_eq!(Class::of(&Method::GET, "/readyz"), None);
        assert_eq!(Class::of(&Method::GET, "/studio/_app/x.js"), None);
        assert_eq!(
            Class::of(&Method::GET, "/auth/login"),
            Some(Class::Management)
        );
        assert_eq!(
            Class::of(&Method::POST, "/preview/v1/p/production/exchange"),
            Some(Class::Management)
        );
        // Every route that reads Postgres counts (D-231): the session's `/me`, the media origin,
        // and a preview preflight.
        assert_eq!(Class::of(&Method::GET, "/me"), Some(Class::Management));
        assert_eq!(
            Class::of(&Method::GET, "/media/v1/p/production/a"),
            Some(Class::Delivery)
        );
        assert_eq!(
            Class::of(&Method::OPTIONS, "/preview/v1/p/production/articles"),
            Some(Class::Management)
        );
    }

    #[test]
    fn a_quota_is_a_rolling_window_not_a_fixed_one() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        // 5 per 2 seconds: 400_000 µs between requests, a burst of the whole window's five.
        let keys = [("q".to_owned(), quota_cell(5, 2_000))];
        for _ in 0..5 {
            assert_eq!(local.check(0, &keys), None);
        }
        assert_eq!(local.check(0, &keys), Some(400_000));
        // Unlike a fixed window, nothing resets at an edge: half the window later only the
        // rate's earnings are back (two intervals elapsed), then the next wait is the third.
        assert_eq!(local.check(1_000_000, &keys), None);
        assert_eq!(local.check(1_000_000, &keys), None);
        assert_eq!(local.check(1_000_000, &keys), Some(200_000));
    }

    #[test]
    fn quota_cells_floor_at_one_request_and_survive_huge_limits() {
        let single = quota_cell(1, 60_000);
        assert_eq!(
            (single.interval_us, single.tolerance_us),
            (60_000_000, 0),
            "one a minute admits one a minute"
        );
        // The column's maximum: interval and tolerance stay finite.
        let biggest = quota_cell(100_000, 60_000);
        assert!(biggest.interval_us >= 1);
        assert_eq!(biggest.tolerance_us, biggest.interval_us * 99_999);
    }

    /// D-238: a key that is actually being limited (its arrival far ahead: a password counter that
    /// spent its burst) stays limited through a flood of new keys while Valkey is away, and the
    /// map never holds more than [`MAX_LOCAL_KEYS`]. Clearing the whole map at the bound handed
    /// every limited key a fresh burst: it failed first that way.
    #[test]
    fn a_limited_key_stays_limited_through_a_flood_of_new_keys() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        let victim = [("victim".to_owned(), rate(1, 5).cell())];
        for _ in 0..5 {
            assert_eq!(local.check(0, &victim), None, "its burst");
        }
        assert!(
            local.check(0, &victim).is_some(),
            "limited before the flood"
        );
        for n in 0..(MAX_LOCAL_KEYS + 5_000) {
            local.check(0, &[(format!("flood{n}"), rate(1, 1).cell())]);
            assert!(
                local.arrivals.len() <= MAX_LOCAL_KEYS,
                "{} keys after {n}",
                local.arrivals.len()
            );
            if n % 10_000 == 0 {
                assert!(
                    local.check(0, &victim).is_some(),
                    "limited during the flood ({n})"
                );
            }
        }
        assert!(local.check(0, &victim).is_some(), "still limited after it");
    }

    /// D-238: a token over its D-118 quota keeps its throttle marker through a flood of new
    /// markers (each episode is audited once; a lost marker would audit it again and again), and
    /// the markers never number more than [`MAX_LOCAL_KEYS`].
    #[test]
    fn an_over_quota_token_stays_marked_through_a_flood() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let over = Uuid::now_v7();
        assert!(admission.mark_throttled(over, 60_000_000), "a long episode");
        for n in 0..(MAX_LOCAL_KEYS + 5_000) {
            admission.mark_throttled(Uuid::now_v7(), 1_000_000);
            if n % 10_000 == 0 {
                let markers = admission
                    .throttled
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .len();
                assert!(markers <= MAX_LOCAL_KEYS, "{markers} markers after {n}");
            }
        }
        assert!(
            !admission.mark_throttled(over, 60_000_000),
            "the episode is still the one already told"
        );
    }

    /// D-238's cost: a flood of 200,000 distinct keys, each one request, timed per request. Making
    /// room evicts a tenth of the bound in one pass, so all but one request in ten thousand is a
    /// map insert. Prints the p99 and the worst, which D-238 records; bounds them loosely (a debug
    /// build on a shared runner) so only a per-request scan fails it.
    #[test]
    fn a_flood_costs_a_map_insert_a_request() {
        let mut local = Local {
            epoch: Instant::now(),
            arrivals: HashMap::new(),
        };
        let cell = rate(1, 1).cell();
        let mut took: Vec<Duration> = Vec::with_capacity(200_000);
        for n in 0..200_000_u64 {
            let key = [(format!("flood{n}"), cell)];
            let started = Instant::now();
            local.check(n, &key);
            took.push(started.elapsed());
        }
        took.sort_unstable();
        let p99 = took[took.len() * 99 / 100];
        let worst = took[took.len() - 1];
        eprintln!("200,000-key flood: p99 {p99:?}, worst {worst:?}");
        assert!(p99 < Duration::from_millis(1), "p99 {p99:?}");
        assert!(worst < Duration::from_secs(1), "worst {worst:?}");
    }

    #[test]
    fn a_throttle_episode_is_marked_once() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let token = Uuid::now_v7();
        assert!(admission.mark_throttled(token, 1_000_000));
        assert!(
            !admission.mark_throttled(token, 1_000_000),
            "still refused, still one event"
        );
        let other = Uuid::now_v7();
        assert!(admission.mark_throttled(other, 1_000_000));
        // A wait already behind us holds nothing: the marker lapses and the next episode tells.
        assert!(admission.mark_throttled(Uuid::now_v7(), 0));
    }

    /// The path a remembered refusal answers for: the token's own project's content.
    fn counted(project: Uuid) -> String {
        format!("/api/v1/projects/{project}/environments/production/collections/a/entries")
    }

    /// A token judged now that expires in a day.
    fn judged(hash: [u8; 32]) -> Judged {
        let now = Instant::now();
        Judged {
            hash,
            read_at: now,
            expires: now + Duration::from_hours(24),
        }
    }

    #[tokio::test]
    async fn a_change_to_a_projects_tokens_ends_its_remembered_refusals_and_no_others() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let (project, elsewhere) = (Uuid::now_v7(), Uuid::now_v7());
        let (here, there) = ([1_u8; 32], [2_u8; 32]);
        admission
            .throttle_for_tests(here, project, Uuid::now_v7(), 2)
            .await;
        admission
            .throttle_for_tests(there, elsewhere, Uuid::now_v7(), 2)
            .await;
        assert!(
            admission
                .refused_again(&here, &counted(project))
                .await
                .is_some()
        );

        admission.tokens_changed(project);
        assert!(
            admission
                .refused_again(&here, &counted(project))
                .await
                .is_none(),
            "the changed project's token takes the full path, judged as it is now"
        );
        assert!(
            admission
                .refused_again(&there, &counted(elsewhere))
                .await
                .is_some(),
            "another project's refusal still stands"
        );
    }

    #[tokio::test]
    async fn a_refusal_judged_on_a_row_read_before_a_change_is_never_remembered() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let (project, token) = (Uuid::now_v7(), Uuid::now_v7());
        let hash = [4_u8; 32];
        // The row was read, then the token changed, then the (stale) refusal came back.
        let stale = judged(hash);
        admission.tokens_changed(project);
        admission.remember_refusal(stale, project, token, 2, 60_000_000);
        assert_eq!(
            admission.refused_len(),
            0,
            "a stale judgement isn't remembered"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
        // A row read after the change is current, and its refusal is remembered.
        admission.remember_refusal(judged(hash), project, token, 2, 60_000_000);
        assert_eq!(admission.refused_len(), 1);
        // Another project's change says nothing about this one's reads.
        let read = judged([5_u8; 32]);
        admission.tokens_changed(Uuid::now_v7());
        admission.remember_refusal(read, project, Uuid::now_v7(), 2, 60_000_000);
        assert_eq!(admission.refused_len(), 2);
    }

    #[tokio::test]
    async fn a_listener_gap_ends_every_refusal_and_every_judgement_before_it() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let (project, token) = (Uuid::now_v7(), Uuid::now_v7());
        let held = [6_u8; 32];
        admission.throttle_for_tests(held, project, token, 2).await;
        let in_flight = judged([8_u8; 32]);
        admission.forget_every_refusal();
        assert!(
            admission
                .refused_again(&held, &counted(project))
                .await
                .is_none()
        );
        admission.remember_refusal(in_flight, project, Uuid::now_v7(), 2, 60_000_000);
        assert_eq!(admission.refused_len(), 0, "read before the gap: stale");
    }

    #[tokio::test]
    async fn a_remembered_refusal_never_outlives_its_token() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let project = Uuid::now_v7();
        let now = Instant::now();
        // Expiring in 50 ms, refused for a minute: remembered for the 50 ms only.
        let expiring = Judged {
            hash: [10_u8; 32],
            read_at: now,
            expires: now + Duration::from_millis(50),
        };
        admission.remember_refusal(expiring, project, Uuid::now_v7(), 2, 60_000_000);
        assert_eq!(admission.refused_len(), 1);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            admission
                .refused_again(&expiring.hash, &counted(project))
                .await
                .is_none(),
            "expired: the full path answers 401"
        );
        // Already expired: never remembered at all.
        let expired = Judged {
            hash: [11_u8; 32],
            read_at: Instant::now(),
            expires: Instant::now(),
        };
        admission.remember_refusal(expired, project, Uuid::now_v7(), 2, 60_000_000);
        assert_eq!(admission.refused_len(), 0);
    }

    #[test]
    fn the_change_list_stays_bounded_and_turns_old_reads_stale_when_full() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let read = judged([12_u8; 32]);
        for _ in 0..MAX_REFUSED + 10 {
            admission.tokens_changed(Uuid::now_v7());
        }
        let memo = admission
            .refused
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert!(memo.changed.len() <= MAX_REFUSED, "{}", memo.changed.len());
        // Every change is newer than the horizon, so the list was cleared and the floor moved
        // to now: a read from before then is stale for every project.
        assert!(!memo.current(Uuid::now_v7(), read.read_at));
    }

    #[tokio::test]
    async fn the_refusal_memo_stays_bounded_and_keeps_refusing_what_it_holds() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let (project, token) = (Uuid::now_v7(), Uuid::now_v7());
        let held = [7_u8; 32];
        admission.throttle_for_tests(held, project, token, 2).await;

        // A flood of other refused tokens, far past the cap, all still waiting.
        let flood = MAX_REFUSED + 500;
        for n in 0..flood {
            let mut hash = [0_u8; 32];
            hash[..8].copy_from_slice(&(n as u64).to_le_bytes());
            hash[31] = 1;
            admission.remember_refusal(judged(hash), project, Uuid::now_v7(), 2, 60_000_000);
        }
        assert_eq!(
            admission.refused_len(),
            MAX_REFUSED,
            "never more than the cap"
        );
        assert!(
            admission
                .refused_again(&held, &counted(project))
                .await
                .is_some(),
            "a refusal it holds still refuses"
        );
        // One the full memo couldn't take answers nothing here: the full path refuses it instead.
        let mut late = [0_u8; 32];
        late[..8].copy_from_slice(&((flood - 1) as u64).to_le_bytes());
        late[31] = 1;
        assert!(
            admission
                .refused_again(&late, &counted(project))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn expired_refusals_make_room_and_are_never_answered_from() {
        let admission = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        let project = Uuid::now_v7();
        for n in 0..MAX_REFUSED {
            let mut hash = [0_u8; 32];
            hash[..8].copy_from_slice(&(n as u64).to_le_bytes());
            // A wait already behind us.
            admission.remember_refusal(judged(hash), project, Uuid::now_v7(), 2, 0);
        }
        assert!(
            admission
                .refused_again(&[0_u8; 32], &counted(project))
                .await
                .is_none(),
            "an expired refusal answers nothing"
        );
        let fresh = [9_u8; 32];
        let token = Uuid::now_v7();
        admission.throttle_for_tests(fresh, project, token, 2).await;
        assert!(admission.refused_len() <= MAX_REFUSED);
        assert!(
            admission
                .refused_again(&fresh, &counted(project))
                .await
                .is_some(),
            "a full memo of expired refusals makes room for a live one"
        );
    }

    #[tokio::test]
    async fn without_valkey_a_remembered_refusal_falls_back_to_the_full_path() {
        // Valkey configured but not there (nothing listens on port 1): the memo's question
        // fails, so it answers nothing and the request takes the full path, whose quota check
        // has the same fallback as ever (D-118, unchanged by D-214).
        let admission = Admission::new(AdmissionSettings::default(), Some("redis://127.0.0.1:1"))
            .expect("a valid URL");
        let (project, token) = (Uuid::now_v7(), Uuid::now_v7());
        let hash = [3_u8; 32];
        admission.remember_refusal(judged(hash), project, token, 2, 60_000_000);
        assert!(
            admission
                .refused_again(&hash, &counted(project))
                .await
                .is_none()
        );
        assert_eq!(admission.refused_len(), 0, "and the memo lets it go");
    }

    #[test]
    fn password_work_is_its_own_class_on_every_route_that_hashes() {
        for path in [
            "/auth/signin",
            "/auth/signup",
            "/audience/v1/p/signup",
            "/audience/v1/p/signin",
            "/audience/v1/p/magic-link/t0ken",
            "/api/v1/projects/p/audience/users",
        ] {
            assert_eq!(
                Class::of(&Method::POST, path),
                Some(Class::Password),
                "{path}"
            );
        }
        for (method, path) in [
            (Method::GET, "/auth/login"),
            (Method::POST, "/auth/device/approve"),
            (Method::GET, "/api/v1/projects/p/audience/users"),
            (Method::POST, "/api/v1/projects/p/audience/users/u"),
            (Method::POST, "/audience/v1/p/magic-link"),
            (Method::POST, "/audience/v1/p/signout"),
            (Method::GET, "/audience/v1/p/me"),
            (Method::POST, "/audience/v1/p/signin/more"),
        ] {
            assert_eq!(
                Class::of(&method, path),
                Some(Class::Management),
                "{method} {path}"
            );
        }
    }

    #[test]
    fn password_work_counts_the_management_limits_and_its_own_per_slash_64() {
        let admission = Admission::new(
            AdmissionSettings {
                per_address: Some(rate(300, 600)),
                per_credential: Some(rate(150, 300)),
                password_per_address: Some(rate(5, 10)),
                ..AdmissionSettings::default()
            },
            None,
        )
        .expect("no URL");
        let headers = HeaderMap::new();
        let keys = |address: &str| {
            admission
                .keys(
                    Class::Password,
                    Some(address.parse().expect("an address")),
                    &headers,
                )
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys("198.51.100.7"),
            [
                "colophon:rl:m:a:198.51.100.7",
                "colophon:rl:p:a:198.51.100.7"
            ]
        );
        assert_eq!(keys("2001:db8:1:2::1"), keys("2001:db8:1:2:ffff::9"));
        assert_ne!(keys("2001:db8:1:2::1"), keys("2001:db8:1:3::1"));
        // Off, it's management work and nothing more.
        let off = Admission::new(AdmissionSettings::default(), None).expect("no URL");
        assert_eq!(
            off.keys(
                Class::Password,
                Some("198.51.100.7".parse().expect("v4")),
                &headers
            ),
            Vec::new()
        );
    }

    #[test]
    fn a_password_burst_is_admitted_then_refused_and_another_address_is_not() {
        let admission = Admission::new(
            AdmissionSettings {
                password_per_address: Some(rate(5, 10)),
                ..AdmissionSettings::default()
            },
            None,
        )
        .expect("no URL");
        let headers = HeaderMap::new();
        let one = admission.keys(
            Class::Password,
            Some("198.51.100.7".parse().expect("v4")),
            &headers,
        );
        let other = admission.keys(
            Class::Password,
            Some("198.51.100.8".parse().expect("v4")),
            &headers,
        );
        let mut local = admission.local.lock().expect("lock");
        let admitted = (0..20).filter(|_| local.check(0, &one).is_none()).count();
        assert_eq!(
            admitted, 10,
            "the burst, and not one more at the same instant"
        );
        assert!(
            local.check(0, &other).is_none(),
            "another address has its own count"
        );
    }
}
