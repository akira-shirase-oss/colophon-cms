//! Where releases may be delivered (docs/modules/05-worker.md; D-046). A destination's address
//! is configured by a project's owners and developers, so it must not become a way into the
//! server's own network: the cloud metadata endpoint, the database, an admin port. These checks
//! run when a destination is saved and again before every attempt; the worker also resolves
//! names itself and keeps only public addresses, and follows no redirects.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `ip` is on the public internet: not loopback, private, link-local, shared, reserved,
/// documentation, benchmarking, multicast or unspecified.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..=19).contains(&b))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first == 0x2001 && ip.segments()[1] == 0x0db8)
        || (first == 0x0064 && ip.segments()[1] == 0xff9b))
}

/// Checks a destination address before any connection: http or https only, https unless
/// private destinations are allowed, and no private IP literal unless they are.
pub fn check_url(text: &str, allow_private: bool) -> Result<(), String> {
    let url = url::Url::parse(text).map_err(|_| "not a valid URL".to_owned())?;
    match url.scheme() {
        "https" => {}
        "http" if allow_private => {}
        "http" => return Err("destinations must use https".to_owned()),
        _ => return Err("destinations are http or https URLs".to_owned()),
    }
    let ip = match url.host() {
        Some(url::Host::Ipv4(v4)) => Some(IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => Some(IpAddr::V6(v6)),
        Some(url::Host::Domain(_)) => None,
        None => return Err("a destination needs a host".to_owned()),
    };
    if let Some(ip) = ip
        && !allow_private
        && !is_public(ip)
    {
        return Err(format!("{ip} is not a public address"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a destination URL can't carry credentials".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_public() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "192.0.2.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "64:ff9b::a00:1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
        ] {
            let ip: IpAddr = private.parse().expect("an address");
            assert!(!is_public(ip), "{private}");
        }
        for public in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            let ip: IpAddr = public.parse().expect("an address");
            assert!(is_public(ip), "{public}");
        }
    }

    #[test]
    fn urls_are_checked_before_connecting() {
        assert!(check_url("https://hooks.example.com/colophon", false).is_ok());
        assert!(
            check_url("http://hooks.example.com/colophon", false).is_err(),
            "https only"
        );
        assert!(
            check_url("http://127.0.0.1:9000/hook", true).is_ok(),
            "a development server"
        );
        assert!(
            check_url("https://169.254.169.254/latest/meta-data", false).is_err(),
            "cloud metadata"
        );
        assert!(check_url("https://[::1]/hook", false).is_err());
        assert!(check_url("https://10.0.0.5/hook", false).is_err());
        assert!(check_url("ftp://hooks.example.com", true).is_err());
        assert!(check_url("https://user:secret@hooks.example.com", false).is_err());
        assert!(check_url("not a url", true).is_err());
    }
}
