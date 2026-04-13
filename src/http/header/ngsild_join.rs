//! Typed `NGSILD-Join` request header for Andromeda linked-entity traversal.
//!
//! Carries join mode equivalent to the `join` query parameter.

use crate::domain::response_format::JoinMode;
use axum::http::{HeaderName, HeaderValue};
use std::str::FromStr;

static NGSILD_JOIN: HeaderName = HeaderName::from_static("ngsild-join");

/// Typed `NGSILD-Join` header containing a validated join mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NgsiLdJoin(JoinMode);

impl NgsiLdJoin {
    /// Return the parsed join mode.
    pub const fn into_inner(self) -> JoinMode {
        self.0
    }
}

impl headers::Header for NgsiLdJoin {
    fn name() -> &'static HeaderName {
        &NGSILD_JOIN
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdJoin, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        let mode = JoinMode::from_str(trimmed).map_err(|_| headers::Error::invalid())?;
        Ok(NgsiLdJoin(mode))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(&self.0.to_string()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Join header"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdJoin, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdJoin::decode(&mut iter)
    }

    #[test]
    fn decode_valid_modes() {
        pretty_assertions::assert_eq!(decode_from("inline").expect("valid").into_inner(), JoinMode::Inline);
        pretty_assertions::assert_eq!(decode_from("flat").expect("valid").into_inner(), JoinMode::Flat);
    }

    #[test]
    fn decode_rejects_invalid_mode() {
        assert!(decode_from("deep").is_err());
    }
}
