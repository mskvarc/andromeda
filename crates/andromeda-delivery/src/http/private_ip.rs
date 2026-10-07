//! Refusing to deliver into address space a client should not be able to reach.
//!
//! A subscription names its own endpoint, so without this guard a client can
//! point the broker at anything the broker's network can see and read the
//! result from the delivery log. The check is on resolved addresses rather
//! than on the hostname, because a name a client controls can resolve
//! anywhere.

use crate::host_port::HostPort;
use ip_network::{Ipv4Network, Ipv6Network};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};
use tokio::net::lookup_host;

/// Outcome of a private-IP guard lookup, cached short-term for hot endpoints.
#[derive(Debug, Clone)]
pub enum PrivateIpCheck {
    /// The host resolved to only public, non-reserved addresses.
    Allowed,
    /// The host resolved to a private or reserved address.
    Blocked {
        /// The address that triggered the guard.
        ip: IpAddr,
    },
    /// The host could not be resolved.
    DnsFailed {
        /// What the resolver reported, shared because the outcome is cached.
        source: Arc<io::Error>,
    },
}

/// Resolve an authority and decide whether delivery may proceed.
pub async fn resolve_private_ip_outcome(authority: &HostPort) -> PrivateIpCheck {
    match lookup_host(authority.to_string()).await {
        Ok(addrs) => {
            for resolved in addrs {
                if is_private_ip(&resolved.ip()) {
                    return PrivateIpCheck::Blocked { ip: resolved.ip() };
                }
            }
            PrivateIpCheck::Allowed
        }
        Err(error) => PrivateIpCheck::DnsFailed { source: Arc::new(error) },
    }
}

/// Whether the address is one delivery must not be made to: anything that is not
/// a globally-routable unicast address.
///
/// The classification is delegated to `ip_network`'s reserved-range predicates
/// rather than a hand-maintained table, so the ranges track the IANA special
/// registries: `is_global` already excludes RFC 1918, loopback, link-local,
/// CGNAT shared space, the TEST-NET documentation ranges, benchmarking, the
/// IETF protocol assignments and the `240.0.0.0/4` reserved block. Three cases
/// `is_global` does not cover for this guard are added here:
///
/// - IPv4-mapped IPv6 (`::ffff:a.b.c.d`) reaches the IPv4 it embeds, so it is
///   classified as that IPv4, so `::ffff:127.0.0.1` is refused as loopback.
/// - NAT64 (RFC 6052 well-known `64:ff9b::/96` and RFC 8215 local-use
///   `64:ff9b:1::/48`) is translated to an IPv4 destination, so `64:ff9b::7f00:1`
///   would reach `127.0.0.1`; the whole prefix is refused.
/// - Multicast is not a unicast endpoint and `is_global` does not exclude IPv4
///   multicast, so it is refused explicitly.
#[must_use]
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(*v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_v4(mapped);
            }
            if is_nat64(v6) {
                return true;
            }
            v6.is_multicast() || !Ipv6Network::from(*v6).is_global()
        }
    }
}

/// Whether an IPv4 address must not be delivered to.
fn is_blocked_v4(v4: Ipv4Addr) -> bool {
    v4.is_multicast() || !Ipv4Network::from(v4).is_global()
}

/// Whether an IPv6 address is a NAT64 translation prefix (RFC 6052 / RFC 8215).
const fn is_nat64(v6: &Ipv6Addr) -> bool {
    let s = v6.segments();
    let well_known = s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0;
    let local_use = s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0x0001;
    well_known || local_use
}

#[cfg(test)]
mod tests {
    use crate::http::private_ip::is_private_ip;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn every_ipv4_range_an_ssrf_would_aim_at_is_blocked() {
        for address in [
            Ipv4Addr::LOCALHOST,            // loopback 127.0.0.0/8
            Ipv4Addr::new(10, 0, 0, 1),     // private 10/8
            Ipv4Addr::new(172, 16, 0, 1),   // private 172.16/12
            Ipv4Addr::new(192, 168, 1, 1),  // private 192.168/16
            Ipv4Addr::new(169, 254, 1, 1),  // link-local 169.254/16
            Ipv4Addr::new(100, 64, 0, 1),   // CGNAT 100.64/10
            Ipv4Addr::new(192, 0, 0, 1),    // IETF protocol assignments 192.0.0.0/24
            Ipv4Addr::new(192, 0, 2, 1),    // TEST-NET-1 documentation
            Ipv4Addr::new(198, 51, 100, 1), // TEST-NET-2 documentation
            Ipv4Addr::new(203, 0, 113, 1),  // TEST-NET-3 documentation
            Ipv4Addr::new(198, 18, 0, 1),   // benchmarking 198.18/15
            Ipv4Addr::new(240, 0, 0, 1),    // reserved 240/4
            Ipv4Addr::new(224, 0, 0, 1),    // multicast 224/4
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::BROADCAST,
        ] {
            assert!(is_private_ip(&IpAddr::V4(address)), "{address}");
        }
    }

    #[test]
    fn every_ipv6_range_an_ssrf_would_aim_at_is_blocked() {
        for address in [
            Ipv6Addr::LOCALHOST,
            Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1), // ULA fc00::/7
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1), // link-local fe80::/10
            Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1), // multicast ff00::/8
        ] {
            assert!(is_private_ip(&IpAddr::V6(address)), "{address}");
        }
    }

    #[test]
    fn an_ipv4_mapped_ipv6_address_is_classified_as_the_ipv4_it_embeds() {
        // Testing only IPv6 ranges would let ::ffff:127.0.0.1 and ::ffff:10.0.0.1
        // through to loopback and RFC 1918 space.
        assert!(is_private_ip(&IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped())));
        assert!(is_private_ip(&IpAddr::V6(Ipv4Addr::new(10, 0, 0, 1).to_ipv6_mapped())));
        // A mapped public address is still allowed.
        assert!(!is_private_ip(&IpAddr::V6(Ipv4Addr::new(8, 8, 8, 8).to_ipv6_mapped())));
    }

    #[test]
    fn a_nat64_prefix_is_blocked_because_it_translates_to_an_ipv4_destination() {
        // 64:ff9b::7f00:1 translates to 127.0.0.1.
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x7f00, 0x0001))));
        // The RFC 8215 local-use prefix 64:ff9b:1::/48 too.
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0x0001, 0, 0, 0, 0x0a00, 0x0001))));
    }

    #[test]
    fn a_public_address_is_allowed() {
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_private_ip(&IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888))));
    }

    #[test]
    fn an_address_above_the_cgnat_block_is_not_mistaken_for_one_inside_it() {
        // CGNAT is 100.64.0.0/10, which ends at 100.127.255.255.
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 1))));
    }
}
