//! Reading the element of a forwarded field that the public edge wrote.
//!
//! IETF RFC 7239 clause 4 lets each proxy append its own element to
//! `Forwarded`, so the list runs from the element closest to the original
//! client to the element added by the proxy nearest the broker. The de-facto
//! `X-Forwarded-*` fields have no RFC, but a chain of proxies that appends to
//! them produces a comma-separated list in the same order. The public edge,
//! the one whose scheme and host a client would use to come back to a minted
//! URL, wrote the leftmost element, so that is the one read.

use axum::http::{HeaderMap, HeaderName};
use rfc7239::{Forwarded, RfcError};
use std::fmt::Display;

/// The RFC 7239 field carrying what a proxy forwarded.
pub(crate) static FORWARDED: HeaderName = HeaderName::from_static("forwarded");

/// The leftmost element of an RFC 7239 `Forwarded` field value, or `None`
/// when the value holds no element.
///
/// # Errors
///
/// Returns the parser's [`RfcError`] when the leftmost element is not RFC
/// 7239 syntax.
pub(crate) fn leftmost_forwarded_element(value: &str) -> Result<Option<Forwarded<'_>>, RfcError> {
    // A field present but empty carries no element to read, which is a field
    // that names nothing rather than one whose syntax is wrong.
    if value.trim().is_empty() {
        return Ok(None);
    }
    rfc7239::parse(value).next().transpose()
}

/// The leftmost member of a comma-separated `X-Forwarded-*` field value.
pub(crate) fn leftmost_list_member(value: &str) -> &str {
    value.split(',').next().unwrap_or(value)
}

/// One parameter value as its token, or `None` when it is empty.
///
/// RFC 7239 clause 4 permits a quoted string wherever it permits a token, and
/// a member of an `X-Forwarded-*` list may carry the whitespace that follows a
/// comma.
pub(crate) fn unquoted(raw: &str) -> Option<&str> {
    let token = raw.trim().trim_matches('"').trim();
    (!token.is_empty()).then_some(token)
}

/// Read one forwarded field with `parse`, reporting an unreadable value as an
/// absent one so a malformed field costs the request nothing.
pub(crate) fn read_field<T, E: Display>(headers: &HeaderMap, name: &HeaderName, parse: fn(&str) -> Result<T, E>) -> Option<T> {
    let value = headers.get(name)?;
    let text = match value.to_str() {
        Ok(text) => text,
        Err(error) => {
            tracing::debug!(field = %name, error = %error, "forwarded field is not visible text");
            return None;
        }
    };
    match parse(text) {
        Ok(read) => Some(read),
        Err(error) => {
            tracing::debug!(field = %name, error = %error, "forwarded field names nothing usable");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::forwarded_element::{leftmost_forwarded_element, leftmost_list_member, unquoted};
    use pretty_assertions::assert_eq;

    /// RFC 7239 clause 4 orders the elements from the client outward.
    #[test]
    fn the_leftmost_rfc_7239_element_is_read() {
        let element = leftmost_forwarded_element("host=a.example;proto=https, host=b.example").ok().flatten();
        assert_eq!(element.and_then(|element| element.host), Some("a.example"));
    }

    #[test]
    fn an_empty_rfc_7239_value_holds_no_element() {
        assert!(matches!(leftmost_forwarded_element(" "), Ok(None)));
    }

    #[test]
    fn a_malformed_leftmost_rfc_7239_element_is_a_syntax_failure() {
        assert!(leftmost_forwarded_element("not an element").is_err());
    }

    #[test]
    fn the_leftmost_list_member_is_read() {
        assert_eq!(leftmost_list_member("a.example, b.example"), "a.example");
        assert_eq!(leftmost_list_member("a.example"), "a.example");
    }

    #[test]
    fn a_quoted_or_padded_value_is_read_as_its_token_and_an_empty_one_as_none() {
        assert_eq!(unquoted(" \"https\" "), Some("https"));
        assert_eq!(unquoted(" https"), Some("https"));
        assert_eq!(unquoted("\"\""), None);
        assert_eq!(unquoted(""), None);
    }
}
