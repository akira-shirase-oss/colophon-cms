//! Where the worker may connect (D-046). The address checks live in
//! `colophon_domain::destination`; this adds the resolver that keeps them true at connection
//! time: it hands reqwest only public addresses, so a name can't resolve to a private one, then
//! or later. A development server bound to loopback may reach private addresses, such as a test
//! receiver on 127.0.0.1.

use std::{future::Future, net::SocketAddr, pin::Pin};

use colophon_domain::destination::is_public;

/// Refused: the address isn't public.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked(pub String);

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not a public address", self.0)
    }
}

impl std::error::Error for Blocked {}

/// A resolver that hands reqwest only public addresses (or any, when private destinations are
/// allowed), so the connection goes where the check said it would.
pub(crate) struct PublicOnly {
    pub(crate) allow_private: bool,
}

type Addrs = Box<dyn Iterator<Item = SocketAddr> + Send>;
type Resolved = Result<Addrs, Box<dyn std::error::Error + Send + Sync>>;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> Pin<Box<dyn Future<Output = Resolved> + Send>> {
        let allow_private = self.allow_private;
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let usable: Vec<SocketAddr> = found
                .into_iter()
                .filter(|addr| allow_private || is_public(addr.ip()))
                .collect();
            if usable.is_empty() {
                return Err(Box::new(Blocked(host)) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(usable.into_iter()) as Addrs)
        })
    }
}

/// Whether an error came from the guard refusing an address (a terminal failure, not a
/// transient one).
pub(crate) fn blocked(error: &(dyn std::error::Error + 'static)) -> Option<String> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        if let Some(blocked) = cause.downcast_ref::<Blocked>() {
            return Some(blocked.to_string());
        }
        current = cause.source();
    }
    None
}
