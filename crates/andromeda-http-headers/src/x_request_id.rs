//! Typed `X-Request-ID` correlation header.

use http::{HeaderName, HeaderValue};
use std::str::FromStr;

/// The lowercase `X-Request-ID` field name this header is transmitted under.
static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// A non-empty, single-token request correlation identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRequestId(String);

impl XRequestId {
    /// Construct an identifier, rejecting whitespace and control characters.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty() && value.bytes().all(|b| b > 0x20 && b != 0x7f && b < 0x80)).then_some(Self(value))
    }
    /// Return the identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl headers::Header for XRequestId {
    fn name() -> &'static HeaderName {
        &X_REQUEST_ID
    }
    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        Self::new(value.to_str().map_err(|_| headers::Error::invalid())?.to_owned()).ok_or_else(headers::Error::invalid)
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        if let Ok(value) = HeaderValue::from_str(&self.0) {
            values.extend(std::iter::once(value));
        }
    }
}

impl FromStr for XRequestId {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value).ok_or(())
    }
}
