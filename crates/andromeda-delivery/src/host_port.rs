//! A network authority: a host paired with the port it is reached on.

use std::{borrow::Cow, str::FromStr};
use url::{Host, Url};

/// A broker or endpoint authority — a host and the port it answers on.
///
/// Composes [`url::Host`] rather than a bare `String` so the host keeps the
/// distinction the URL parser already drew between a domain, an IPv4 literal and
/// an IPv6 literal, and so equality and hashing are the host's own rather than a
/// byte comparison of two spellings of the same address. That matters because a
/// `HostPort` is used as a pooled-connection key and as a DNS-guard cache key,
/// where two subscriptions naming the same broker must collide.
///
/// [`Display`](std::fmt::Display) renders the authority form `host:port`, with
/// an IPv6 host bracketed as `[::1]:8883`, so the rendering round-trips through
/// both [`FromStr`] and [`std::net::ToSocketAddrs`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{host}:{port}")]
pub struct HostPort {
    /// The host, parsed into whichever of domain, IPv4 or IPv6 it is.
    host: Host,
    /// The port the host is reached on.
    port: u16,
}

impl HostPort {
    /// An authority for the given host and port.
    pub const fn new(host: Host, port: u16) -> HostPort {
        HostPort { host, port }
    }

    /// The host, in the parsed form that distinguishes a domain from an IP
    /// literal.
    pub const fn host(&self) -> &Host {
        &self.host
    }

    /// The port the host is reached on.
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The host as a socket API expects it: a domain verbatim, or an IP literal
    /// without the brackets a URL authority wraps an IPv6 address in.
    ///
    /// Borrows for a domain and allocates only for an IP literal, whose textual
    /// form the parsed address does not keep.
    pub fn host_str(&self) -> Cow<'_, str> {
        match &self.host {
            Host::Domain(domain) => Cow::Borrowed(domain.as_str()),
            Host::Ipv4(address) => Cow::Owned(address.to_string()),
            Host::Ipv6(address) => Cow::Owned(address.to_string()),
        }
    }
}

/// A `host:port` string that is not a valid authority.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid host:port authority")]
pub struct HostPortParseError;

// Parse via the URL authority parser rather than by splitting on `:`, which an
// IPv6 literal's own colons would defeat. A throwaway `http` scheme gives the
// parser an authority to read the host and port out of; `http` specifically
// because the URL parser only recognises an IPv4 literal as one under a special
// scheme, keeping it an opaque domain otherwise. The port is required — a
// `HostPort` is a host *and* a port, so an authority without one is rejected.
impl FromStr for HostPort {
    type Err = HostPortParseError;

    fn from_str(s: &str) -> Result<HostPort, HostPortParseError> {
        let url = Url::parse(&format!("http://{s}")).map_err(|_parse| HostPortParseError)?;
        let host = url.host().ok_or(HostPortParseError)?.to_owned();
        let port = url.port().ok_or(HostPortParseError)?;
        Ok(HostPort::new(host, port))
    }
}

#[cfg(test)]
mod tests {
    use crate::host_port::{HostPort, HostPortParseError};
    use std::{
        collections::HashSet,
        net::{Ipv4Addr, Ipv6Addr},
        str::FromStr,
    };
    use url::Host;

    #[test]
    fn a_domain_authority_round_trips_through_its_authority_string() {
        let authority = HostPort::from_str("broker.example.com:1883").expect("a valid authority");
        pretty_assertions::assert_eq!(authority.host_str().as_ref(), "broker.example.com");
        pretty_assertions::assert_eq!(authority.port(), 1883);
        pretty_assertions::assert_eq!(authority.to_string(), "broker.example.com:1883");
        pretty_assertions::assert_eq!(authority.host(), &Host::<String>::Domain("broker.example.com".to_owned()));
    }

    #[test]
    fn an_ipv4_authority_keeps_its_address_form() {
        let authority = HostPort::from_str("127.0.0.1:8883").expect("a valid authority");
        pretty_assertions::assert_eq!(authority.host(), &Host::<String>::Ipv4(Ipv4Addr::LOCALHOST));
        pretty_assertions::assert_eq!(authority.host_str().as_ref(), "127.0.0.1");
        pretty_assertions::assert_eq!(authority.to_string(), "127.0.0.1:8883");
    }

    #[test]
    fn an_ipv6_authority_is_bracketed_in_the_authority_form_but_bare_for_a_socket() {
        let authority = HostPort::from_str("[::1]:8883").expect("a valid authority");
        pretty_assertions::assert_eq!(authority.host(), &Host::<String>::Ipv6(Ipv6Addr::LOCALHOST));
        // The socket form drops the brackets a URL authority wraps an IPv6 in.
        pretty_assertions::assert_eq!(authority.host_str().as_ref(), "::1");
        // The authority form keeps them, so it parses back as a `SocketAddrV6`.
        pretty_assertions::assert_eq!(authority.to_string(), "[::1]:8883");
    }

    #[test]
    fn an_authority_with_no_port_is_rejected_because_a_host_port_is_a_pair() {
        pretty_assertions::assert_eq!(HostPort::from_str("broker.example.com"), Err(HostPortParseError));
    }

    #[test]
    fn a_malformed_authority_is_rejected() {
        pretty_assertions::assert_eq!(HostPort::from_str(""), Err(HostPortParseError));
        pretty_assertions::assert_eq!(HostPort::from_str("broker:not-a-port"), Err(HostPortParseError));
    }

    #[test]
    fn two_authorities_naming_the_same_broker_are_equal_and_hash_together() {
        let first = HostPort::new(Host::Domain("broker".to_owned()), 1883);
        let second = HostPort::from_str("broker:1883").expect("a valid authority");
        pretty_assertions::assert_eq!(first, second);

        let mut set = HashSet::new();
        set.insert(first);
        assert!(set.contains(&second), "an equal authority collides in a hashed set");
    }
}
