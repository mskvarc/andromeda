//! Typed `NGSILD-Scope` request header for Andromeda scope-query extensions.
//!
//! Carries the same scope-query language as the `scopeQ` query parameter:
//! `,` for OR, `;` for AND, and `/`-separated scope path terms.
//! Wildcards `+` (single level) and `#` (multi-level, terminal) are supported.

use axum::http::{HeaderName, HeaderValue};

static NGSILD_SCOPE: HeaderName = HeaderName::from_static("ngsild-scope");

/// Parsed Andromeda scope-query expression carried by `NGSILD-Scope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdScope(String);

impl NgsiLdScope {
    /// Return the normalized scope expression string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(raw: &str) -> Result<NgsiLdScope, ()> {
        let mut normalized_or_terms = Vec::new();
        for or_term in raw.split(',').map(str::trim) {
            if or_term.is_empty() {
                return Err(());
            }
            let mut normalized_and_terms = Vec::new();
            for and_term in or_term.split(';').map(str::trim) {
                validate_scope_term(and_term)?;
                normalized_and_terms.push(and_term.to_string());
            }
            normalized_or_terms.push(normalized_and_terms.join(";"));
        }
        Ok(NgsiLdScope(normalized_or_terms.join(",")))
    }
}

impl headers::Header for NgsiLdScope {
    fn name() -> &'static HeaderName {
        &NGSILD_SCOPE
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdScope, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let value = values.next().ok_or_else(headers::Error::invalid)?;
        let s = value.to_str().map_err(|_| headers::Error::invalid())?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(headers::Error::invalid());
        }
        NgsiLdScope::parse(trimmed).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(self.as_str()) {
            Ok(val) => values.extend(std::iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Scope header"),
        }
    }
}

fn validate_scope_term(term: &str) -> Result<(), ()> {
    if term.is_empty() {
        return Err(());
    }
    if term == "urn:ngsi-ld:null" {
        return Ok(());
    }

    let path = term.strip_prefix('/').unwrap_or(term);
    if path.is_empty() {
        return Err(());
    }

    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(());
    }

    for (index, segment) in segments.iter().enumerate() {
        if *segment == "+" {
            continue;
        }
        if *segment == "#" {
            if index + 1 != segments.len() {
                return Err(());
            }
            continue;
        }
        validate_scope_label(segment)?;
    }

    Ok(())
}

fn validate_scope_label(segment: &str) -> Result<(), ()> {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return Err(());
    };
    if !first.is_alphabetic() {
        return Err(());
    }
    if chars.any(|ch| !ch.is_alphabetic() && !ch.is_ascii_digit() && ch != '_') {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<NgsiLdScope, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        NgsiLdScope::decode(&mut iter)
    }

    #[test]
    fn decode_valid_expression() {
        let header = decode_from("/A;/B,/C/#").expect("valid scope");
        pretty_assertions::assert_eq!(header.as_str(), "/A;/B,/C/#");
    }

    #[test]
    fn decode_valid_plus_wildcard() {
        let header = decode_from("/A/+/C").expect("valid scope");
        pretty_assertions::assert_eq!(header.as_str(), "/A/+/C");
    }

    #[test]
    fn decode_trims_and_normalizes_spaces() {
        let header = decode_from(" /A ; /B , /C/# ").expect("valid scope");
        pretty_assertions::assert_eq!(header.as_str(), "/A;/B,/C/#");
    }

    #[test]
    fn decode_rejects_empty() {
        assert!(decode_from("").is_err());
    }

    #[test]
    fn decode_rejects_empty_segment() {
        assert!(decode_from("/A//B").is_err());
    }

    #[test]
    fn decode_rejects_non_terminal_hash() {
        assert!(decode_from("/A/#/B").is_err());
    }

    #[test]
    fn decode_rejects_invalid_label() {
        assert!(decode_from("/1A/B").is_err());
    }
}
