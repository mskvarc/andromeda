//! Typed `NGSILD-Response-Format` request header for Andromeda format overrides.
//!
//! Carries an extension token equivalent to the `format` query parameter.

use axum::http::{HeaderName, HeaderValue};
use std::str::FromStr;
use strum::{Display, EnumString};

static NGSILD_RESPONSE_FORMAT: HeaderName = HeaderName::from_static("ngsild-response-format");

/// Supported extension values for `NGSILD-Response-Format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum NgsiLdResponseFormat {
    /// Normalized representation.
    Normalized,
    /// Key-values representation.
    #[strum(serialize = "keyValues")]
    KeyValues,
    /// Simplified representation.
    Simplified,
    /// Concise representation.
    Concise,
    /// Temporal values representation.
    TemporalValues,
    /// Aggregated values representation.
    AggregatedValues,
}

impl NgsiLdResponseFormat {
    /// Return this format token as the query-parameter wire value.
    pub fn as_query_value(&self) -> &'static str {
        match self {
            NgsiLdResponseFormat::Normalized => "normalized",
            NgsiLdResponseFormat::KeyValues => "keyValues",
            NgsiLdResponseFormat::Simplified => "simplified",
            NgsiLdResponseFormat::Concise => "concise",
            NgsiLdResponseFormat::TemporalValues => "temporalValues",
            NgsiLdResponseFormat::AggregatedValues => "aggregatedValues",
        }
    }
}

impl headers::Header for NgsiLdResponseFormat {
    fn name() -> &'static HeaderName {
        &NGSILD_RESPONSE_FORMAT
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdResponseFormat, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        NgsiLdResponseFormat::from_str(trimmed).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.as_query_value()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Response-Format header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdResponseFormat, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdResponseFormat::decode(&mut iter)
    }

    #[test]
    fn decode_valid_values() {
        assert_eq!(decode_from("normalized").expect("valid"), NgsiLdResponseFormat::Normalized);
        assert_eq!(decode_from("keyValues").expect("valid"), NgsiLdResponseFormat::KeyValues);
        assert_eq!(decode_from("temporalValues").expect("valid"), NgsiLdResponseFormat::TemporalValues);
    }

    #[test]
    fn decode_rejects_invalid() {
        assert!(decode_from("xml").is_err());
    }

    #[test]
    fn encode_roundtrip() {
        let header = NgsiLdResponseFormat::Concise;
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        let mut iter = encoded.iter();
        let decoded = NgsiLdResponseFormat::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(decoded, header);
    }
}
