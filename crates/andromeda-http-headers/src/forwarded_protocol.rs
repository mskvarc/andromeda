//! The protocol a client used to reach the reverse proxy in front of the broker.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2 requires that "the API shall support HTTP
//! over TLS (also known as HTTPS)". Andromeda meets that by terminating TLS at
//! a reverse proxy (see `docs/tls-deployment.md`), which leaves the broker
//! serving plaintext HTTP behind it. The same clause makes the `apiRoot`
//! include "the scheme ("http" or "https")", so a broker that ignored the
//! terminator would advertise `http://` URLs for an `https://` deployment --
//! in `Location` fields, in pagination `Link` fields, and, durably, in the
//! `@context` URLs clause 5.13 has it serve.
//!
//! Two fields carry the answer. IETF RFC 7239 defines the standards-track
//! `Forwarded` field; `X-Forwarded-Proto` is the de-facto field that predates
//! it. Both are read, `Forwarded` first.

use http::{HeaderMap, HeaderName};
use strum::{Display, EnumString, IntoStaticStr};
use thiserror::Error;

/// The RFC 7239 field carrying what a proxy forwarded.
static FORWARDED: HeaderName = HeaderName::from_static("forwarded");

/// The de-facto field carrying the protocol a proxy forwarded.
static X_FORWARDED_PROTO: HeaderName = HeaderName::from_static("x-forwarded-proto");

/// The scheme a client used to reach the edge in front of the broker.
///
/// Clause 6.2 admits exactly two: "the `apiRoot` includes the scheme (`http`
/// or `https`)". A forwarded field naming anything else names something this
/// broker cannot mint an `apiRoot` from, so it is a parse failure rather than
/// a third variant.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum ForwardedProtocol {
    /// Plaintext HTTP.
    Http,
    /// HTTP over TLS, as clause 6.2 requires the deployment to offer.
    Https,
}

impl ForwardedProtocol {
    /// The URL scheme this protocol spells.
    #[must_use]
    pub fn as_scheme(self) -> &'static str {
        self.into()
    }
}

/// Why a forwarded-protocol field could not name a protocol.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ForwardedProtocolError {
    /// The `Forwarded` field value does not parse as RFC 7239 syntax.
    #[error("Forwarded field value is not RFC 7239 syntax: {detail}")]
    ForwardedSyntax {
        /// What the RFC 7239 parser reported.
        detail: String,
    },
    /// The field named no protocol at all.
    #[error("forwarded field carries no protocol")]
    NoProtocol,
    /// The named protocol is not one clause 6.2 admits in an `apiRoot`.
    #[error("forwarded protocol `{0}` is neither http nor https")]
    UnsupportedProtocol(String),
}

/// The protocol an RFC 7239 `Forwarded` field value names.
///
/// RFC 7239 clause 4 lets a proxy append its own element to the field, so the
/// list runs from the element closest to the original client to the element
/// added by the proxy nearest the broker. The public edge -- the one whose
/// scheme a client would use to come back to a minted URL -- is the leftmost,
/// so that is the element read.
///
/// # Errors
///
/// Returns [`ForwardedProtocolError::ForwardedSyntax`] when the leftmost
/// element is not RFC 7239 syntax, [`ForwardedProtocolError::NoProtocol`] when
/// the value is empty or its leftmost element carries no `proto` parameter,
/// and [`ForwardedProtocolError::UnsupportedProtocol`] when that parameter
/// names neither `http` nor `https`.
pub fn from_forwarded(value: &str) -> Result<ForwardedProtocol, ForwardedProtocolError> {
    // A field present but empty carries no element to read, which is a field
    // that names no protocol rather than one whose syntax is wrong.
    if value.trim().is_empty() {
        return Err(ForwardedProtocolError::NoProtocol);
    }
    let leftmost = rfc7239::parse(value).next().ok_or(ForwardedProtocolError::NoProtocol)?;
    let element = leftmost.map_err(|error| ForwardedProtocolError::ForwardedSyntax { detail: error.to_string() })?;
    let protocol = element.protocol.ok_or(ForwardedProtocolError::NoProtocol)?;
    parse_protocol(protocol)
}

/// The protocol an `X-Forwarded-Proto` field value names.
///
/// The field has no RFC. A proxy chain that appends rather than replaces
/// produces a comma-separated list ordered the way RFC 7239 orders its
/// elements, so the leftmost value is again the one closest to the original
/// client.
///
/// # Errors
///
/// Returns [`ForwardedProtocolError::NoProtocol`] when the value is empty and
/// [`ForwardedProtocolError::UnsupportedProtocol`] when it names neither
/// `http` nor `https`.
pub fn from_x_forwarded_proto(value: &str) -> Result<ForwardedProtocol, ForwardedProtocolError> {
    let leftmost = value.split(',').next().ok_or(ForwardedProtocolError::NoProtocol)?;
    parse_protocol(leftmost)
}

/// Read one protocol token, which RFC 7239 clause 4 permits as a quoted string
/// as well as a bare token.
fn parse_protocol(raw: &str) -> Result<ForwardedProtocol, ForwardedProtocolError> {
    let token = raw.trim().trim_matches('"').trim();
    if token.is_empty() {
        return Err(ForwardedProtocolError::NoProtocol);
    }
    token.parse().map_err(|_| ForwardedProtocolError::UnsupportedProtocol(token.to_owned()))
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
pub fn forwarded_protocol(headers: &HeaderMap) -> Option<ForwardedProtocol> {
    read_field(headers, &FORWARDED, from_forwarded).or_else(|| read_field(headers, &X_FORWARDED_PROTO, from_x_forwarded_proto))
}

/// Read one forwarded field with `parse`, reporting an unreadable value as an
/// absent one so a malformed field costs the request nothing.
fn read_field(headers: &HeaderMap, name: &HeaderName, parse: fn(&str) -> Result<ForwardedProtocol, ForwardedProtocolError>) -> Option<ForwardedProtocol> {
    let value = headers.get(name)?;
    let text = match value.to_str() {
        Ok(text) => text,
        Err(error) => {
            tracing::debug!(field = %name, error = %error, "forwarded field is not visible text");
            return None;
        }
    };
    match parse(text) {
        Ok(protocol) => Some(protocol),
        Err(error) => {
            tracing::debug!(field = %name, error = %error, "forwarded field names no usable protocol");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_protocol::{ForwardedProtocol, ForwardedProtocolError, forwarded_protocol, from_forwarded, from_x_forwarded_proto};
    use http::{HeaderMap, HeaderValue};

    /// A header map carrying the named fields.
    fn headers(fields: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in fields {
            map.append(*name, HeaderValue::from_str(value).expect("a valid header value"));
        }
        map
    }

    #[test]
    fn the_two_schemes_clause_6_2_admits_round_trip_through_their_tokens() {
        pretty_assertions::assert_eq!(ForwardedProtocol::Http.as_scheme(), "http");
        pretty_assertions::assert_eq!(ForwardedProtocol::Https.as_scheme(), "https");
        pretty_assertions::assert_eq!(ForwardedProtocol::Https.to_string(), "https");
        pretty_assertions::assert_eq!("HTTPS".parse::<ForwardedProtocol>(), Ok(ForwardedProtocol::Https));
    }

    #[test]
    fn an_rfc_7239_element_names_its_protocol() {
        pretty_assertions::assert_eq!(from_forwarded("proto=https"), Ok(ForwardedProtocol::Https));
        pretty_assertions::assert_eq!(from_forwarded("for=192.0.2.60;proto=https;by=203.0.113.43"), Ok(ForwardedProtocol::Https));
    }

    /// RFC 7239 clause 4 permits a quoted string wherever it permits a token.
    #[test]
    fn a_quoted_rfc_7239_protocol_is_read_as_its_token() {
        pretty_assertions::assert_eq!(from_forwarded("proto=\"https\""), Ok(ForwardedProtocol::Https));
    }

    /// RFC 7239 makes the parameter names case-insensitive and the protocol a
    /// scheme, which RFC 3986 clause 3.1 also makes case-insensitive.
    #[test]
    fn rfc_7239_parameter_and_protocol_casing_do_not_change_the_answer() {
        pretty_assertions::assert_eq!(from_forwarded("Proto=HTTPS"), Ok(ForwardedProtocol::Https));
    }

    /// RFC 7239 clause 4 orders the elements from the client outward, so the
    /// leftmost is the public edge whose scheme a minted URL must carry.
    #[test]
    fn the_leftmost_rfc_7239_element_decides_the_protocol() {
        pretty_assertions::assert_eq!(from_forwarded("proto=https,proto=http"), Ok(ForwardedProtocol::Https));
        pretty_assertions::assert_eq!(from_forwarded("proto=http,proto=https"), Ok(ForwardedProtocol::Http));
    }

    #[test]
    fn an_rfc_7239_element_with_no_protocol_names_none() {
        pretty_assertions::assert_eq!(from_forwarded("for=192.0.2.60"), Err(ForwardedProtocolError::NoProtocol));
        pretty_assertions::assert_eq!(from_forwarded(""), Err(ForwardedProtocolError::NoProtocol));
    }

    #[test]
    fn a_malformed_rfc_7239_element_is_a_syntax_failure_rather_than_a_protocol() {
        assert!(matches!(
            from_forwarded("this is not a forwarded element"),
            Err(ForwardedProtocolError::ForwardedSyntax { .. })
        ));
    }

    /// Clause 6.2 admits `http` and `https` only, so a third scheme is refused
    /// rather than minted into an `apiRoot`.
    #[test]
    fn a_protocol_clause_6_2_does_not_admit_is_refused() {
        pretty_assertions::assert_eq!(
            from_forwarded("proto=gopher"),
            Err(ForwardedProtocolError::UnsupportedProtocol("gopher".to_owned()))
        );
        pretty_assertions::assert_eq!(from_x_forwarded_proto("ws"), Err(ForwardedProtocolError::UnsupportedProtocol("ws".to_owned())));
    }

    #[test]
    fn an_x_forwarded_proto_value_names_its_protocol() {
        pretty_assertions::assert_eq!(from_x_forwarded_proto("https"), Ok(ForwardedProtocol::Https));
        pretty_assertions::assert_eq!(from_x_forwarded_proto(" HTTP "), Ok(ForwardedProtocol::Http));
    }

    #[test]
    fn the_leftmost_x_forwarded_proto_value_decides_the_protocol() {
        pretty_assertions::assert_eq!(from_x_forwarded_proto("https, http"), Ok(ForwardedProtocol::Https));
    }

    #[test]
    fn an_empty_x_forwarded_proto_value_names_no_protocol() {
        pretty_assertions::assert_eq!(from_x_forwarded_proto(""), Err(ForwardedProtocolError::NoProtocol));
    }

    /// RFC 7239 clause 1: `Forwarded` is the standards-track field the
    /// `X-Forwarded-*` family is being replaced by, so it decides.
    #[test]
    fn the_rfc_7239_field_wins_over_the_de_facto_one() {
        let map = headers(&[("forwarded", "proto=https"), ("x-forwarded-proto", "http")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ForwardedProtocol::Https));
    }

    /// A `Forwarded` field that names no protocol must not shadow an
    /// `X-Forwarded-Proto` that does: proxies commonly send `for=` alone.
    #[test]
    fn an_rfc_7239_field_without_a_protocol_falls_through_to_the_de_facto_one() {
        let map = headers(&[("forwarded", "for=192.0.2.60"), ("x-forwarded-proto", "https")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ForwardedProtocol::Https));
    }

    #[test]
    fn a_malformed_rfc_7239_field_falls_through_rather_than_failing() {
        let map = headers(&[("forwarded", "garbage"), ("x-forwarded-proto", "https")]);
        pretty_assertions::assert_eq!(forwarded_protocol(&map), Some(ForwardedProtocol::Https));
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
