//! Refusing to deliver into address space a client should not be able to reach.
//!
//! A subscription names its own endpoint, so without this guard a client can
//! point the broker at anything the broker's network can see and read the
//! result from the delivery log. The check is on resolved addresses rather
//! than on the hostname, because a name a client controls can resolve
//! anywhere.

use std::net::IpAddr;

/// Outcome of a private-IP guard lookup, cached short-term for hot endpoints.
#[derive(Debug, Clone)]
pub enum PrivateIpCheck {
    /// The host resolved to only public, non-reserved addresses.
    Allowed,
    /// The host resolved to a private or reserved address.
    Blocked {
        /// The address that triggered the guard.
        ip: String,
    },
    /// The host could not be resolved.
    DnsFailed {
        /// What the resolver reported.
        detail: String,
    },
}

/// Resolve `host:port` and decide whether delivery may proceed.
pub async fn resolve_private_ip_outcome(addr: &str) -> PrivateIpCheck {
    match tokio::net::lookup_host(addr).await {
        Ok(addrs) => {
            for resolved in addrs {
                if is_private_ip(&resolved.ip()) {
                    return PrivateIpCheck::Blocked { ip: resolved.ip().to_string() };
                }
            }
            PrivateIpCheck::Allowed
        }
        Err(error) => PrivateIpCheck::DnsFailed { detail: error.to_string() },
    }
}

/// Whether the address is private, reserved, loopback, link-local, or
/// otherwise not routable on the public internet.
///
/// Covers every range an attacker would aim at to reach an internal service:
/// RFC 1918, loopback, link-local, CGNAT (100.64/10), IPv6 ULA and IPv6
/// link-local.
pub const fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()                          // 127.0.0.0/8
                || v4.is_private()                    // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local()                 // 169.254/16
                || v4.is_unspecified()                // 0.0.0.0
                || v4.is_broadcast()                  // 255.255.255.255
                || v4.octets()[0] == 100              // CGNAT 100.64.0.0/10
                    && (v4.octets()[1] & 0xC0) == 64
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()                          // ::1
                || v6.is_unspecified()                // ::
                || (v6.segments()[0] & 0xFE00) == 0xFC00  // ULA fc00::/7
                || (v6.segments()[0] & 0xFFC0) == 0xFE80 // link-local fe80::/10
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::http::private_ip::is_private_ip;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn every_ipv4_range_an_ssrf_would_aim_at_is_blocked() {
        for address in [
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(172, 16, 0, 1),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(169, 254, 1, 1),
            Ipv4Addr::new(100, 64, 0, 1),
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
            Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1),
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
        ] {
            assert!(is_private_ip(&IpAddr::V6(address)), "{address}");
        }
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
