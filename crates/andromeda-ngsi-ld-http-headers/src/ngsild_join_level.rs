//! Typed `NGSILD-Join-Level` request header for Andromeda linked-entity traversal.
//!
//! Carries join depth equivalent to the `joinLevel` query parameter.

use http::{HeaderName, HeaderValue};

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_JOIN_LEVEL: HeaderName = HeaderName::from_static("ngsild-joinlevel");

/// Typed `NGSILD-Join-Level` header containing traversal depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NgsiLdJoinLevel(u32);

impl NgsiLdJoinLevel {
    /// Return the parsed join level.
    pub const fn into_inner(self) -> u32 {
        self.0
    }
}

impl headers::Header for NgsiLdJoinLevel {
    fn name() -> &'static HeaderName {
        &NGSILD_JOIN_LEVEL
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdJoinLevel, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        let join_level = trimmed.parse::<u32>().map_err(|_| headers::Error::invalid())?;
        Ok(NgsiLdJoinLevel(join_level))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(&self.0.to_string()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Join-Level header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdJoinLevel, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdJoinLevel::decode(&mut iter)
    }

    #[test]
    fn decode_valid_number() {
        pretty_assertions::assert_eq!(decode_from("3").expect("valid").into_inner(), 3);
    }

    #[test]
    fn decode_rejects_invalid_number() {
        assert!(decode_from("x").is_err());
    }
}
