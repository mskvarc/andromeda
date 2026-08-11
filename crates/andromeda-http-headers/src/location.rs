//! Strict `Location` response header (RFC 9110, section 10.2.2).

use http::{HeaderName, HeaderValue};

/// The lowercase `Location` field name this header is transmitted under.
static LOCATION: HeaderName = HeaderName::from_static("location");

/// A validated URI-reference used by a `Location` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location(HeaderValue);

impl Location {
    /// Construct a location from a non-empty valid HTTP header value.
    pub fn new(value: impl AsRef<str>) -> Option<Self> {
        let value = value.as_ref();
        (!value.is_empty()).then(|| HeaderValue::from_str(value).ok()).flatten().map(Self)
    }
    /// Borrow the encoded URI-reference.
    pub const fn as_header_value(&self) -> &HeaderValue {
        &self.0
    }
}

impl headers::Header for Location {
    fn name() -> &'static HeaderName {
        &LOCATION
    }
    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        Self::new(value.to_str().map_err(|_| headers::Error::invalid())?).ok_or_else(headers::Error::invalid)
    }
    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        values.extend(std::iter::once(self.0.clone()));
    }
}
