//! The host a client addressed at the reverse proxy in front of the broker.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2 makes the `apiRoot` include the host and
//! optional port the API is reached at. A reverse proxy that rewrites `Host`
//! to the broker's internal name hands the broker an authority no client can
//! reach, and passes the one the client addressed in a forwarded field
//! instead: the `host` parameter of the IETF RFC 7239 `Forwarded` field, or
//! the de-facto `X-Forwarded-Host` field that predates it. Both are read,
//! `Forwarded` first. The host carries its port, so no separate port field is
//! read.

use crate::forwarded_element::{FORWARDED, leftmost_forwarded_element, leftmost_list_member, read_field, unquoted};
use axum::http::{
    HeaderMap,
    HeaderName,
    uri::{Authority, InvalidUri},
};
use rfc7239::RfcError;
use thiserror::Error;

/// The de-facto field carrying the host a proxy forwarded.
static X_FORWARDED_HOST: HeaderName = HeaderName::from_static("x-forwarded-host");

/// Why a forwarded-host field could not name a host.
#[derive(Debug, Error)]
pub enum ForwardedHostError {
    /// The `Forwarded` field value does not parse as RFC 7239 syntax.
    #[error("forwarded field value is not RFC 7239 syntax")]
    ForwardedSyntax(#[source] RfcError),
    /// The field named no host at all.
    #[error("forwarded field carries no host")]
    NoHost,
    /// The named host is not an authority.
    #[error("forwarded host is not an authority")]
    InvalidAuthority(#[source] InvalidUri),
    /// The named host carries user information, which RFC 9110 section 4.2.4
    /// forbids in the authority of an `http` or `https` URI.
    #[error("forwarded host carries user information")]
    UserInfo,
}

/// The host the leftmost element of an RFC 7239 `Forwarded` field value
/// names.
///
/// # Errors
///
/// Returns [`ForwardedHostError::ForwardedSyntax`] when the leftmost element
/// is not RFC 7239 syntax, [`ForwardedHostError::NoHost`] when the value is
/// empty or its leftmost element carries no `host` parameter, and the errors
/// of [`parse_host`] for a host that is no plain authority.
pub fn from_forwarded(value: &str) -> Result<Authority, ForwardedHostError> {
    let element = leftmost_forwarded_element(value)
        .map_err(ForwardedHostError::ForwardedSyntax)?
        .ok_or(ForwardedHostError::NoHost)?;
    parse_host(element.host.ok_or(ForwardedHostError::NoHost)?)
}

/// The host the leftmost member of an `X-Forwarded-Host` field value names.
///
/// # Errors
///
/// Returns [`ForwardedHostError::NoHost`] when the value is empty, and the
/// errors of [`parse_host`] for a host that is no plain authority.
pub fn from_x_forwarded_host(value: &str) -> Result<Authority, ForwardedHostError> {
    parse_host(leftmost_list_member(value))
}

/// Read one host token as an authority: a host and an optional port.
///
/// # Errors
///
/// Returns [`ForwardedHostError::NoHost`] for an empty token,
/// [`ForwardedHostError::InvalidAuthority`] for one that is no authority, and
/// [`ForwardedHostError::UserInfo`] for one carrying user information.
fn parse_host(raw: &str) -> Result<Authority, ForwardedHostError> {
    let authority: Authority = unquoted(raw)
        .ok_or(ForwardedHostError::NoHost)?
        .parse()
        .map_err(ForwardedHostError::InvalidAuthority)?;
    if authority.as_str().contains('@') {
        return Err(ForwardedHostError::UserInfo);
    }
    Ok(authority)
}

/// The host the request's forwarded fields name, if any of them do.
///
/// `Forwarded` is read first and its answer wins, as RFC 7239 clause 1 has it
/// replace the `X-Forwarded-*` family. A field that names nothing readable is
/// not a failure: the caller falls back to the authority the request
/// addressed.
///
/// This function does not decide whether the fields may be believed. That is
/// the caller's decision: both fields are ordinary request fields that
/// anything able to reach the listener can set.
#[must_use]
pub fn forwarded_host(headers: &HeaderMap) -> Option<Authority> {
    read_field(headers, &FORWARDED, from_forwarded).or_else(|| read_field(headers, &X_FORWARDED_HOST, from_x_forwarded_host))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::forwarded_host::{ForwardedHostError, forwarded_host, from_forwarded, from_x_forwarded_host};
    use axum::http::{HeaderMap, HeaderValue};
    use pretty_assertions::assert_eq;

    /// A header map carrying the named fields.
    fn headers(fields: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in fields {
            map.append(*name, HeaderValue::from_str(value).expect("a valid header value"));
        }
        map
    }

    /// The host as the request's forwarded fields name it.
    fn host_of(fields: &[(&'static str, &str)]) -> Option<String> {
        forwarded_host(&headers(fields)).map(|authority| authority.to_string())
    }

    #[test]
    fn an_rfc_7239_element_names_its_host_with_its_port() {
        assert_eq!(
            from_forwarded("host=broker.example:8443").map(|host| host.to_string()).ok(),
            Some("broker.example:8443".to_owned())
        );
        assert_eq!(
            from_forwarded("for=192.0.2.60;host=broker.example;proto=https")
                .map(|host| host.to_string())
                .ok(),
            Some("broker.example".to_owned())
        );
    }

    /// RFC 7239 clause 4 permits a quoted string wherever it permits a token,
    /// and an IPv6 host needs one for its colons.
    #[test]
    fn a_quoted_rfc_7239_host_is_read_as_its_token() {
        assert_eq!(
            from_forwarded("host=\"[2001:db8::1]:8443\"").map(|host| host.to_string()).ok(),
            Some("[2001:db8::1]:8443".to_owned())
        );
    }

    #[test]
    fn the_leftmost_rfc_7239_element_decides_the_host() {
        assert_eq!(
            host_of(&[("forwarded", "host=public.example, host=internal.example")]),
            Some("public.example".to_owned())
        );
    }

    #[test]
    fn an_rfc_7239_element_with_no_host_names_none() {
        assert!(matches!(from_forwarded("proto=https"), Err(ForwardedHostError::NoHost)));
        assert!(matches!(from_forwarded(""), Err(ForwardedHostError::NoHost)));
    }

    #[test]
    fn a_malformed_rfc_7239_element_is_a_syntax_failure_rather_than_a_host() {
        assert!(matches!(from_forwarded("not an element"), Err(ForwardedHostError::ForwardedSyntax(_))));
    }

    #[test]
    fn a_host_that_is_no_authority_is_refused() {
        assert!(matches!(
            from_x_forwarded_host("broker.example/path"),
            Err(ForwardedHostError::InvalidAuthority(_))
        ));
        assert!(matches!(from_x_forwarded_host("bro ker.example"), Err(ForwardedHostError::InvalidAuthority(_))));
    }

    #[test]
    fn a_host_carrying_user_information_is_refused() {
        assert!(matches!(from_x_forwarded_host("user@broker.example"), Err(ForwardedHostError::UserInfo)));
    }

    #[test]
    fn the_leftmost_x_forwarded_host_value_decides_the_host() {
        assert_eq!(
            host_of(&[("x-forwarded-host", "public.example:8443, internal.example")]),
            Some("public.example:8443".to_owned())
        );
    }

    #[test]
    fn an_empty_x_forwarded_host_value_names_no_host() {
        assert!(matches!(from_x_forwarded_host(""), Err(ForwardedHostError::NoHost)));
    }

    #[test]
    fn the_rfc_7239_field_wins_over_the_de_facto_one() {
        assert_eq!(
            host_of(&[("forwarded", "host=rfc.example"), ("x-forwarded-host", "de-facto.example")]),
            Some("rfc.example".to_owned())
        );
    }

    /// Proxies commonly send `Forwarded` with `for=` and `proto=` alone.
    #[test]
    fn an_rfc_7239_field_without_a_host_falls_through_to_the_de_facto_one() {
        assert_eq!(
            host_of(&[("forwarded", "for=192.0.2.60;proto=https"), ("x-forwarded-host", "de-facto.example")]),
            Some("de-facto.example".to_owned())
        );
    }

    #[test]
    fn a_request_whose_forwarded_fields_name_no_usable_host_names_none() {
        assert_eq!(host_of(&[]), None);
        assert_eq!(host_of(&[("forwarded", "garbage"), ("x-forwarded-host", "user@broker.example")]), None);
    }
}
