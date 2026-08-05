//! Typed `NGSILD-Warning` response header (ETSI GS CIM 009 clause 6.3.17).
//!
//! Uses IANA Warning Codes to signal non-fatal issues encountered during
//! distributed operations (forwarding to context sources).

use axum::http::{HeaderName, HeaderValue};

static NGSILD_WARNING: HeaderName = HeaderName::from_static("ngsild-warning");

/// IANA warning codes for distributed operations (clause 6.3.17).
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub enum NgsiLdWarning {
    /// 110: Cached data reused; context source responded but stale data was served.
    #[display("110 - \"{_0}\"")]
    ResponseIsStale(String),
    /// 111: Context source responded but payload was invalid or non-NGSI-LD.
    #[display("111 - \"{_0}\"")]
    RevalidationFailed(String),
    /// 199: No response within timeout or registration loop detected.
    #[display("199 - \"{_0}\"")]
    MiscellaneousWarning(String),
    /// 299: Error response from registration endpoint (e.g., 403).
    #[display("299 - \"{_0}\"")]
    MiscellaneousPersistentWarning(String),
}

/// Typed `NGSILD-Warning` response header (clause 6.3.17).
///
/// Wraps multiple [`NgsiLdWarning`] values for multi-value header encoding.
#[derive(Debug, Clone)]
pub struct NgsiLdWarningHeader(Vec<NgsiLdWarning>);

impl NgsiLdWarningHeader {
    /// Create a new `NGSILD-Warning` header from the given warnings.
    pub const fn new(warnings: Vec<NgsiLdWarning>) -> NgsiLdWarningHeader {
        NgsiLdWarningHeader(warnings)
    }
}

/// Parse a single warning value like `199 - "timeout"` into an `NgsiLdWarning`.
fn parse_warning(s: &str) -> Option<NgsiLdWarning> {
    let trimmed = s.trim();
    let (code_str, rest) = trimmed.split_once(char::is_whitespace)?;
    let code: u16 = code_str.parse().ok()?;

    // Extract quoted text: find first `"` and last `"`
    let start = rest.find('"')? + 1;
    let end = rest.rfind('"')?;
    if start >= end {
        return None;
    }
    let text = rest[start..end].to_string();

    match code {
        110 => Some(NgsiLdWarning::ResponseIsStale(text)),
        111 => Some(NgsiLdWarning::RevalidationFailed(text)),
        199 => Some(NgsiLdWarning::MiscellaneousWarning(text)),
        299 => Some(NgsiLdWarning::MiscellaneousPersistentWarning(text)),
        _ => None,
    }
}

/// Split a string on commas that are not inside double-quoted segments.
///
/// Warning values contain quoted text (e.g. `199 - "timeout"`) and commas
/// inside quotes must not be treated as separators.
fn split_unquoted_commas(s: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;

    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                segments.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    segments.push(&s[start..]);
    segments
}

impl headers::Header for NgsiLdWarningHeader {
    fn name() -> &'static HeaderName {
        &NGSILD_WARNING
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdWarningHeader, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut warnings = Vec::new();
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            // Each header value may contain multiple comma-separated warnings,
            // but warning text itself may contain commas inside quotes.
            for segment in split_unquoted_commas(s) {
                if let Some(w) = parse_warning(segment) {
                    warnings.push(w);
                }
            }
        }
        if warnings.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(NgsiLdWarningHeader(warnings))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        for warning in &self.0 {
            match HeaderValue::from_str(&warning.to_string()) {
                Ok(val) => values.extend(std::iter::once(val)),
                Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Warning header value"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    #[test]
    fn encode_single_warning() {
        let header = NgsiLdWarningHeader::new(vec![NgsiLdWarning::MiscellaneousWarning("msg".to_string())]);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "199 - \"msg\"");
    }

    #[test]
    fn encode_multiple_warnings() {
        let header = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::ResponseIsStale("cached".to_string()),
            NgsiLdWarning::MiscellaneousPersistentWarning("forbidden".to_string()),
        ]);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 2);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "110 - \"cached\"");
        pretty_assertions::assert_eq!(encoded[1].to_str().expect("valid str"), "299 - \"forbidden\"");
    }

    #[test]
    fn decode_parses_response_is_stale() {
        let val = HeaderValue::from_str("110 - \"cached\"").expect("valid");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::ResponseIsStale("cached".to_string())]);
    }

    #[test]
    fn decode_parses_revalidation_failed() {
        let val = HeaderValue::from_str("111 - \"bad\"").expect("valid");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::RevalidationFailed("bad".to_string())]);
    }

    #[test]
    fn decode_parses_miscellaneous_warning() {
        let val = HeaderValue::from_str("199 - \"timeout\"").expect("valid");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::MiscellaneousWarning("timeout".to_string())]);
    }

    #[test]
    fn decode_parses_persistent_warning() {
        let val = HeaderValue::from_str("299 - \"forbidden\"").expect("valid");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::MiscellaneousPersistentWarning("forbidden".to_string())]);
    }

    #[test]
    fn roundtrip_all_variants() {
        let original = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::ResponseIsStale("stale".to_string()),
            NgsiLdWarning::RevalidationFailed("invalid".to_string()),
            NgsiLdWarning::MiscellaneousWarning("timeout".to_string()),
            NgsiLdWarning::MiscellaneousPersistentWarning("error".to_string()),
        ]);
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 4);

        let mut iter = encoded.iter();
        let decoded = NgsiLdWarningHeader::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original.0, decoded.0);
    }

    #[test]
    fn warning_header_format_miscellaneous() {
        let w = NgsiLdWarning::MiscellaneousWarning("timeout".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "199 - \"timeout\"");
    }

    #[test]
    fn warning_header_format_persistent() {
        let w = NgsiLdWarning::MiscellaneousPersistentWarning("403 Forbidden".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "299 - \"403 Forbidden\"");
    }

    #[test]
    fn warning_header_format_response_is_stale() {
        let w = NgsiLdWarning::ResponseIsStale("cached data".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "110 - \"cached data\"");
    }

    #[test]
    fn warning_header_format_revalidation_failed() {
        let w = NgsiLdWarning::RevalidationFailed("invalid JSON".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "111 - \"invalid JSON\"");
    }

    #[test]
    fn decode_comma_separated_warnings_in_single_header_value() {
        let combined = "199 - \"timeout\", 299 - \"forbidden\"";
        let val = HeaderValue::from_str(combined).expect("valid header value");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode multiple");
        pretty_assertions::assert_eq!(header.0.len(), 2);
        pretty_assertions::assert_eq!(header.0[0], NgsiLdWarning::MiscellaneousWarning("timeout".to_string()));
        pretty_assertions::assert_eq!(header.0[1], NgsiLdWarning::MiscellaneousPersistentWarning("forbidden".to_string()));
    }

    #[test]
    fn decode_warning_with_comma_in_quoted_text() {
        let raw = "199 - \"timeout, retry later\"";
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.len(), 1);
        pretty_assertions::assert_eq!(header.0[0], NgsiLdWarning::MiscellaneousWarning("timeout, retry later".to_string()));
    }
}
