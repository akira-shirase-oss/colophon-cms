//! Admission: rate limits per client address and per credential, shared by every server through
//! Valkey (milestone S, D-076).
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

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, header},
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
    /// Proxies whose `X-Forwarded-For` says who the client is.
    pub trusted_proxies: Vec<IpPrefix>,
}

/// What a request is, for the limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Management,
    Delivery,
}

impl Class {
    /// The class of a request to `path`; `None` if it counts against nothing.
    fn of(path: &str) -> Option<Self> {
        if path.starts_with("/content/") {
            Some(Self::Delivery)
        } else if path.starts_with("/api/") || path.starts_with("/auth/") {
            Some(Self::Management)
        } else {
            None
        }
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

/// This process's own copy of the limits, for while Valkey is away.
struct Local {
    epoch: Instant,
    arrivals: HashMap<String, u64>,
}

impl Local {
    /// GCRA at `now` (microseconds since `epoch`): the wait, or `None` after moving every key.
    fn check(&mut self, now: u64, keys: &[(String, Rate)]) -> Option<u64> {
        if self.arrivals.len() >= MAX_LOCAL_KEYS {
            self.arrivals.retain(|_, arrival| *arrival > now);
            if self.arrivals.len() >= MAX_LOCAL_KEYS {
                self.arrivals.clear();
            }
        }
        let mut wait = 0;
        let mut next = Vec::with_capacity(keys.len());
        for (key, rate) in keys {
            let arrival = self.arrivals.get(key).copied().unwrap_or(now).max(now);
            let allowed_at = arrival.saturating_sub(rate.tolerance_us());
            if allowed_at > now {
                wait = wait.max(allowed_at - now);
            }
            next.push(arrival + rate.interval_us());
        }
        if wait > 0 {
            return Some(wait);
        }
        for ((key, _), arrival) in keys.iter().zip(next) {
            self.arrivals.insert(key.clone(), arrival);
        }
        None
    }
}

/// The limits and where they are counted.
pub struct Admission {
    settings: AdmissionSettings,
    valkey: Valkey,
    local: Mutex<Local>,
}

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
        Ok(Self {
            settings,
            valkey,
            local: Mutex::new(Local {
                epoch: Instant::now(),
                arrivals: HashMap::new(),
            }),
        })
    }

    /// The client's address: the peer, unless it is a trusted proxy, in which case the nearest
    /// address in `X-Forwarded-For` that isn't one. An entry that isn't an address ends the walk:
    /// whatever wrote it isn't believed.
    fn client(&self, peer: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
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

    /// The limits a request counts against, as Valkey keys with their rates.
    fn keys(&self, class: Class, peer: Option<IpAddr>, headers: &HeaderMap) -> Vec<(String, Rate)> {
        let mut keys = Vec::with_capacity(2);
        let (prefix, per_address) = match class {
            Class::Management => ("m", self.settings.per_address),
            Class::Delivery => ("d", self.settings.delivery_per_address),
        };
        if let (Some(rate), Some(client)) = (per_address, self.client(peer, headers)) {
            keys.push((format!("colophon:rl:{prefix}:a:{client}"), rate));
        }
        if class == Class::Management
            && let (Some(rate), Some(credential)) =
                (self.settings.per_credential, credential(headers))
        {
            keys.push((format!("colophon:rl:m:c:{credential}"), rate));
        }
        keys
    }

    /// The wait a request must take, in microseconds, or `None` if it is admitted.
    async fn check(&self, keys: &[(String, Rate)]) -> Option<u64> {
        if keys.is_empty() {
            return None;
        }
        if let Some(shared) = self.shared(keys).await {
            return shared;
        }
        let mut local = self.local.lock().unwrap_or_else(PoisonError::into_inner);
        let now = u64::try_from(local.epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        local.check(now, keys)
    }

    /// [`Self::check`] in Valkey; `None` if Valkey isn't there to ask.
    async fn shared(&self, keys: &[(String, Rate)]) -> Option<Option<u64>> {
        let mut connection = self.valkey.connection().await?;
        let mut command = redis::cmd("EVAL");
        command.arg(SCRIPT).arg(keys.len());
        for (key, _) in keys {
            command.arg(key);
        }
        for (_, rate) in keys {
            command.arg(rate.interval_us()).arg(rate.tolerance_us());
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
    let Some(class) = Class::of(request.uri().path()) else {
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

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

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
        let keys = [("k".to_owned(), rate(10, 3))];
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
        let tight = ("tight".to_owned(), rate(1, 1));
        let loose = ("loose".to_owned(), rate(1, 2));
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
            assert_eq!(local.check(0, &[(format!("k{n}"), rate(1, 1))]), None);
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

    #[test]
    fn credentials_are_counted_hashed_and_only_for_the_management_api() {
        let admission = Admission::new(
            AdmissionSettings {
                per_address: Some(rate(10, 10)),
                per_credential: Some(rate(5, 5)),
                delivery_per_address: Some(rate(100, 100)),
                trusted_proxies: Vec::new(),
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
            [("colophon:rl:d:a:127.0.0.1".to_owned(), rate(100, 100))]
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
        assert_eq!(Class::of("/readyz"), None);
        assert_eq!(Class::of("/studio/_app/x.js"), None);
        assert_eq!(Class::of("/auth/login"), Some(Class::Management));
    }
}
