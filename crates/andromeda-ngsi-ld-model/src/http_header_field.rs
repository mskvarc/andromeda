//! A [`KeyValuePair`] read as one HTTP header field.
//!
//! ETSI GS CIM 009 clauses 6.3.8, 6.3.9 and 6.3.19 require the key and the
//! value of an HTTP-bound `receiverInfo` or `contextSourceInfo` entry to adhere
//! to IETF RFC 7230: the key is a token and the value a field value without
//! control characters. The pair keeps its stored shape (clause 5.2.22); this
//! module is the single reading admission and delivery share, so what is
//! accepted at registration time is exactly what is sent later.

use crate::key_value_pair::KeyValuePair;
use http::{
    HeaderName,
    HeaderValue,
    header::{InvalidHeaderName, InvalidHeaderValue},
};
use thiserror::Error;

/// Why a [`KeyValuePair`] is not an HTTP header field (IETF RFC 7230 section 3.2).
#[derive(Debug, Error)]
pub enum HttpHeaderFieldError {
    /// The key is empty or holds a character outside the RFC 7230 token set.
    #[error("the key is not an HTTP header field name: {0}")]
    Name(#[source] InvalidHeaderName),
    /// The value holds a control character, such as a line break.
    #[error("the value is not an HTTP header field value: {0}")]
    Value(#[source] InvalidHeaderValue),
}

/// One validated HTTP header field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpHeaderField {
    /// The header field name.
    pub name: HeaderName,
    /// The header field value.
    pub value: HeaderValue,
}

impl TryFrom<&KeyValuePair> for HttpHeaderField {
    type Error = HttpHeaderFieldError;

    fn try_from(pair: &KeyValuePair) -> Result<HttpHeaderField, HttpHeaderFieldError> {
        Ok(HttpHeaderField {
            name: HeaderName::from_bytes(pair.key.as_bytes()).map_err(HttpHeaderFieldError::Name)?,
            value: HeaderValue::from_str(pair.value.as_str()).map_err(HttpHeaderFieldError::Value)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        http_header_field::{HttpHeaderField, HttpHeaderFieldError},
        key_value_pair::KeyValuePair,
    };

    fn pair(key: &str, value: &str) -> KeyValuePair {
        KeyValuePair {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_pair_with_a_token_name_and_a_visible_value_becomes_a_header_field() {
        let field = HttpHeaderField::try_from(&pair("Authorization", "Bearer abc"));
        assert!(matches!(field, Ok(field) if field.name == "authorization" && field.value == "Bearer abc"));
    }

    #[test]
    fn a_key_with_a_space_is_refused() {
        assert!(matches!(HttpHeaderField::try_from(&pair("Bad Name", "x")), Err(HttpHeaderFieldError::Name(_))));
    }

    #[test]
    fn a_key_with_a_colon_is_refused() {
        assert!(matches!(HttpHeaderField::try_from(&pair("Bad:Name", "x")), Err(HttpHeaderFieldError::Name(_))));
    }

    #[test]
    fn an_empty_key_is_refused() {
        assert!(matches!(HttpHeaderField::try_from(&pair("", "x")), Err(HttpHeaderFieldError::Name(_))));
    }

    #[test]
    fn a_value_with_a_newline_is_refused() {
        assert!(matches!(HttpHeaderField::try_from(&pair("X-A", "a\nb")), Err(HttpHeaderFieldError::Value(_))));
    }

    #[test]
    fn a_value_with_a_carriage_return_is_refused() {
        assert!(matches!(HttpHeaderField::try_from(&pair("X-A", "a\rb")), Err(HttpHeaderFieldError::Value(_))));
    }
}
