//! The protocol a client used to reach the reverse proxy in front of the broker.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2 requires that "the API shall support HTTP
//! over TLS (also known as HTTPS)". A deployment may meet that by terminating
//! TLS at a reverse proxy, which leaves the
//! broker serving plaintext HTTP behind it. The same clause makes the `apiRoot`
//! include "the scheme ("http" or "https")", so a broker that ignored the
//! terminator would advertise `http://` URLs for an `https://` deployment --
//! in `Location` fields, in pagination `Link` fields, and, durably, in the
//! `@context` URLs clause 5.13 has it serve.
//!
//! Two fields carry the answer. IETF RFC 7239 defines the standards-track
//! `Forwarded` field; `X-Forwarded-Proto` is the de-facto field that predates
//! it. Both are read, `Forwarded` first.

use crate::{
    api_root_scheme::ApiRootScheme,
    forwarded_element::{FORWARDED, leftmost_forwarded_element, leftmost_list_member, read_field, unquoted},
};
use axum::http::{HeaderMap, HeaderName};
use rfc7239::RfcError;
use thiserror::Error;

/// The de-facto field carrying the protocol a proxy forwarded.
static X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// Why a forwarded-protocol field could not name a protocol.
#[derive(Debug, Error)]
pub enum ForwardedProtocolError {
    /// The `Forwarded` field value does not parse as RFC 7239 syntax.
    #[error("forwarded field value is not RFC 7239 syntax")]
    ForwardedSyntax(#[source] RfcError),
    /// The field named no protocol at all.
    #[error("forwarded field carries no protocol")]
    NoProtocol,
    /// The named protocol is not one clause 6.2 admits in an `apiRoot`.
    #[error("forwarded protocol is neither http nor https")]
    UnsupportedProtocol(#[source] strum::ParseError),
}

/// The protocol the leftmost element of an RFC 7239 `Forwarded` field value
/// names.
///
/// # Errors
///
/// Returns [`ForwardedProtocolError::ForwardedSyntax`] when the leftmost
/// element is not RFC 7239 syntax, [`ForwardedProtocolError::NoProtocol`] when
/// the value is empty or its leftmost element carries no `proto` parameter,
/// and [`ForwardedProtocolError::UnsupportedProtocol`] when that parameter
/// names neither `http` nor `https`.
pub fn from_forwarded(value: &str) -> Result<ApiRootScheme, ForwardedProtocolError> {
    let element = leftmost_forwarded_element(value)
        .map_err(ForwardedProtocolError::ForwardedSyntax)?
        .ok_or(ForwardedProtocolError::NoProtocol)?;
    parse_protocol(element.protocol.ok_or(ForwardedProtocolError::NoProtocol)?)
}

/// The protocol the leftmost member of an `X-Forwarded-Proto` field value
/// names.
///
/// # Errors
///
/// Returns [`ForwardedProtocolError::NoProtocol`] when the value is empty and
/// [`ForwardedProtocolError::UnsupportedProtocol`] when it names neither
/// `http` nor `https`.
pub fn from_x_forwarded_proto(value: &str) -> Result<ApiRootScheme, ForwardedProtocolError> {
    parse_protocol(leftmost_list_member(value))
}

/// Read one protocol token.
fn parse_protocol(raw: &str) -> Result<ApiRootScheme, ForwardedProtocolError> {
    unquoted(raw)
        .ok_or(ForwardedProtocolError::NoProtocol)?
        .parse()
        .map_err(ForwardedProtocolError::UnsupportedProtocol)
}

/// The protocol the request's forwarded fields name, if any of them do.
///
/// RFC 7239 clause 1 introduces `Forwarded` as the standards-track replacement
/// for the de-facto `X-Forwarded-*` family, so it is read first and its answer
/// wins. A field that names nothing readable is not a failure: the caller falls
/// back to the scheme the broker's own listener speaks, because a forwarded
/// scheme decides only how a minted URL is spelled and is never a precondition
/// of the operation the request asks for.
///
/// This function does not decide whether the fields may be believed. That is
/// the caller's decision, and it must be a deliberate one: both fields are
/// ordinary request fields that anything able to reach the listener can set.
#[must_use]
pub fn forwarded_protocol(headers: &HeaderMap) -> Option<ApiRootScheme> {
    read_field(headers, &FORWARDED, from_forwarded).or_else(|| read_field(headers, &X_FORWARDED_PROTO, from_x_forwarded_proto))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        api_root_scheme::ApiRootScheme,
        forwarded_protocol::{ForwardedProtocolError, forwarded_protocol, from_forwarded, from_x_forwarded_proto},
    };
    use axum::http::{HeaderMap, HeaderValue};

    /// A header map carrying the named fields.
    fn headers(fields: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in fields {
            map.append(*name, HeaderValue::from_str(value).expect("a valid header value"));
        }
        map
    }

    #[test]
    fn an_rfc_7239_element_names_its_protocol() {
        assert!(matches!(from_forwarded("proto=https"), Ok(ApiRootScheme::Https)));
        assert!(matches!(from_forwarded("for=192.0.2.60;proto=https;by=203.0.113.43"), Ok(ApiRootScheme::Https)));
    }

    /// RFC 7239 clause 4 permits a quoted string wherever it permits a token.
    #[test]
    fn a_quoted_rfc_7239_protocol_is_read_as_its_token() {
        assert!(matches!(from_forwarded("proto=\"https\""), Ok(ApiRootScheme::Https)));
    }

    /// RFC 7239 makes the parameter names case-insensitive and the protocol a
    /// scheme, which RFC 3986 clause 3.1 also makes case-insensitive.
    #[test]
    fn rfc_7239_parameter_and_protocol_casing_do_not_change_the_answer() {
        assert!(matches!(from_forwarded("Proto=HTTPS"), Ok(ApiRootScheme::Https)));
    }

    /// RFC 7239 clause 4 orders the elements from the client outward, so the
    /// leftmost is the public edge whose scheme a minted URL must carry.
    #[test]
    fn the_leftmost_rfc_7239_element_decides_the_protocol() {
        assert!(matches!(from_forwarded("proto=https,proto=http"), Ok(ApiRootScheme::Https)));
        assert!(matches!(from_forwarded("proto=http,proto=https"), Ok(ApiRootScheme::Http)));
    }

    #[test]
    fn an_rfc_7239_element_with_no_protocol_names_none() {
        assert!(matches!(from_forwarded("for=192.0.2.60"), Err(ForwardedProtocolError::NoProtocol)));
        assert!(matches!(from_forwarded(""), Err(ForwardedProtocolError::NoProtocol)));
    }

    #[test]
    fn a_malformed_rfc_7239_element_is_a_syntax_failure_rather_than_a_protocol() {
        assert!(matches!(
            from_forwarded("this is not a forwarded element"),
            Err(ForwardedProtocolError::ForwardedSyntax(_))
        ));
    }

    /// Clause 6.2 admits `http` and `https` only, so a third scheme is refused
    /// rather than minted into an `apiRoot`.
    #[test]
    fn a_protocol_clause_6_2_does_not_admit_is_refused() {
        assert!(matches!(from_forwarded("proto=gopher"), Err(ForwardedProtocolError::UnsupportedProtocol(_))));
        assert!(matches!(from_x_forwarded_proto("ws"), Err(ForwardedProtocolError::UnsupportedProtocol(_))));
    }

    #[test]
    fn an_x_forwarded_proto_value_names_its_protocol() {
        assert!(matches!(from_x_forwarded_proto("https"), Ok(ApiRootScheme::Https)));
        assert!(matches!(from_x_forwarded_proto(" HTTP "), Ok(ApiRootScheme::Http)));
    }

    #[test]
    fn the_leftmost_x_forwarded_proto_value_decides_the_protocol() {
        assert!(matches!(from_x_forwarded_proto("https, http"), Ok(ApiRootScheme::Https)));
    }

    #[test]
    fn an_empty_x_forwarded_proto_value_names_no_protocol() {
        assert!(matches!(from_x_forwarded_proto(""), Err(ForwardedProtocolError::NoProtocol)));
    }

    /// RFC 7239 clause 1: `Forwarded` is the standards-track field the
    /// `X-Forwarded-*` family is being replaced by, so it decides.
    #[test]
    fn the_rfc_7239_field_wins_over_the_de_facto_one() {
        let map = headers(&[("forwarded", "proto=https"), ("x-forwarded-proto", "http")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ApiRootScheme::Https));
    }

    /// A `Forwarded` field that names no protocol must not shadow an
    /// `X-Forwarded-Proto` that does: proxies commonly send `for=` alone.
    #[test]
    fn an_rfc_7239_field_without_a_protocol_falls_through_to_the_de_facto_one() {
        let map = headers(&[("forwarded", "for=192.0.2.60"), ("x-forwarded-proto", "https")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ApiRootScheme::Https));
    }

    #[test]
    fn a_malformed_rfc_7239_field_falls_through_rather_than_failing() {
        let map = headers(&[("forwarded", "garbage"), ("x-forwarded-proto", "https")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ApiRootScheme::Https));
    }

    #[test]
    fn a_request_carrying_no_forwarded_field_names_no_protocol() {
        pretty_assertions::assert_eq!(forwarded_protocol(&HeaderMap::new()), None);
    }

    #[test]
    fn a_request_whose_only_forwarded_fields_are_unusable_names_no_protocol() {
        let map = headers(&[("forwarded", "garbage"), ("x-forwarded-proto", "gopher")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), None);
    }
}
