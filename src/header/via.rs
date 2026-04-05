//! Typed `Via` header per RFC 7230 (ETSI GS CIM 009 clause 6.3.18).
//!
//! The Via header field indicates the presence of intermediate protocols
//! and recipients between the user agent and the server. Used for loop
//! detection in distributed NGSI-LD forwarding.

use axum::http::{HeaderName, HeaderValue};

static VIA: HeaderName = HeaderName::from_static("via");

/// A single entry in a Via header chain.
///
/// Each entry represents an intermediate protocol hop in the form
/// `"protocol-version pseudonym (optional comment)"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaEntry {
    /// The protocol version (e.g., `"1.1"` or `"HTTP/1.1"`).
    pub protocol_version: String,
    /// The pseudonym/identity of the intermediary.
    pub pseudonym: String,
    /// Optional comment enclosed in parentheses.
    pub comment: Option<String>,
}

/// Typed `Via` header (RFC 7230, clause 6.3.18).
///
/// Contains a chain of intermediary pseudonyms used for forwarding loop
/// detection in distributed NGSI-LD operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via(Vec<ViaEntry>);

impl Via {
    /// Create a `Via` header with a single entry.
    pub fn with_entry(protocol_version: impl Into<String>, pseudonym: impl Into<String>) -> Via {
        Via(vec![ViaEntry {
            protocol_version: protocol_version.into(),
            pseudonym: pseudonym.into(),
            comment: None,
        }])
    }

    /// Append an entry to the Via chain, returning the modified header.
    pub fn append(mut self, protocol_version: impl Into<String>, pseudonym: impl Into<String>) -> Via {
        self.0.push(ViaEntry {
            protocol_version: protocol_version.into(),
            pseudonym: pseudonym.into(),
            comment: None,
        });
        self
    }

    /// Iterate over the pseudonyms in the Via chain.
    pub fn pseudonyms(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|e| e.pseudonym.as_str())
    }

    /// Check if the Via chain contains the given pseudonym (exact match).
    ///
    /// Used for loop detection per clause 6.3.18: compares against each
    /// discrete pseudonym, not substring match.
    pub fn contains_pseudonym(&self, identity: &str) -> bool {
        self.0.iter().any(|e| e.pseudonym == identity)
    }

    /// Check if a context source alias appears in the Via pseudonym list.
    ///
    /// Per spec clause 5.12: no registration shall match if its
    /// `contextSourceAlias` is found in the Via chain.
    pub fn has_alias(&self, alias: &str) -> bool {
        self.pseudonyms().any(|p| p == alias)
    }
}

/// Parse a single Via entry from a trimmed string like `"1.1 broker-a"` or
/// `"HTTP/1.1 proxy (Apache)"`.
fn parse_via_entry(s: &str) -> Option<ViaEntry> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split on first whitespace to get protocol_version and the rest
    let (version, rest) = trimmed.split_once(char::is_whitespace)?;
    let rest = rest.trim();

    // Check for comment in parentheses
    if let Some(paren_start) = rest.find('(') {
        let pseudonym = rest[..paren_start].trim();
        if pseudonym.is_empty() {
            return None;
        }
        let comment_end = rest.rfind(')')?;
        let comment = rest[paren_start + 1..comment_end].trim().to_string();
        Some(ViaEntry {
            protocol_version: version.to_string(),
            pseudonym: pseudonym.to_string(),
            comment: Some(comment),
        })
    } else {
        // No comment -- pseudonym may be followed by whitespace
        let pseudonym = rest.split_once(char::is_whitespace).map_or(rest, |(p, _)| p);
        if pseudonym.is_empty() {
            return None;
        }
        Some(ViaEntry {
            protocol_version: version.to_string(),
            pseudonym: pseudonym.to_string(),
            comment: None,
        })
    }
}

impl headers::Header for Via {
    fn name() -> &'static HeaderName {
        &VIA
    }

    fn decode<'i, I>(values: &mut I) -> Result<Via, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let mut entries = Vec::new();
        for value in values {
            let s = value.to_str().map_err(|_| headers::Error::invalid())?;
            for part in s.split(',') {
                if let Some(entry) = parse_via_entry(part) {
                    entries.push(entry);
                }
            }
        }
        if entries.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(Via(entries))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let s: String = self
            .0
            .iter()
            .map(|e| match &e.comment {
                Some(c) => format!("{} {} ({})", e.protocol_version, e.pseudonym, c),
                None => format!("{} {}", e.protocol_version, e.pseudonym),
            })
            .collect::<Vec<_>>()
            .join(", ");
        if let Ok(val) = HeaderValue::from_str(&s) {
            values.extend(std::iter::once(val));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use headers::Header;

    fn decode_from(raw: &str) -> Result<Via, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = std::iter::once(&val);
        Via::decode(&mut iter)
    }

    #[test]
    fn decode_single_entry() {
        let via = decode_from("1.1 broker-a").expect("should decode");
        assert_eq!(via.0.len(), 1);
        assert_eq!(via.0[0].protocol_version, "1.1");
        assert_eq!(via.0[0].pseudonym, "broker-a");
        assert_eq!(via.0[0].comment, None);
    }

    #[test]
    fn decode_multi_entry() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert_eq!(via.0.len(), 2);
        assert_eq!(via.0[0].pseudonym, "broker-a");
        assert_eq!(via.0[1].pseudonym, "broker-b");
    }

    #[test]
    fn decode_with_full_protocol() {
        let via = decode_from("HTTP/1.1 proxy").expect("should decode");
        assert_eq!(via.0[0].protocol_version, "HTTP/1.1");
        assert_eq!(via.0[0].pseudonym, "proxy");
    }

    #[test]
    fn decode_with_comment() {
        let via = decode_from("1.1 proxy (Apache)").expect("should decode");
        assert_eq!(via.0[0].pseudonym, "proxy");
        assert_eq!(via.0[0].comment, Some("Apache".to_string()));
    }

    #[test]
    fn decode_empty_rejects() {
        assert!(decode_from("").is_err());
    }

    #[test]
    fn pseudonyms_extracts_all() {
        let via = decode_from("1.1 a, 1.1 b, 1.1 c").expect("should decode");
        let pseudonyms: Vec<&str> = via.pseudonyms().collect();
        assert_eq!(pseudonyms, vec!["a", "b", "c"]);
    }

    #[test]
    fn contains_pseudonym_positive() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert!(via.contains_pseudonym("broker-a"));
    }

    #[test]
    fn contains_pseudonym_negative() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert!(!via.contains_pseudonym("broker-c"));
    }

    #[test]
    fn contains_pseudonym_no_prefix_match() {
        let via = decode_from("1.1 broker10").expect("should decode");
        assert!(!via.contains_pseudonym("broker1"));
    }

    #[test]
    fn contains_pseudonym_no_suffix_match() {
        let via = decode_from("1.1 my-broker1").expect("should decode");
        assert!(!via.contains_pseudonym("broker1"));
    }

    #[test]
    fn append_adds_to_chain() {
        let via = Via::with_entry("1.1", "broker-a").append("1.1", "broker-b");
        assert_eq!(via.0.len(), 2);
        assert_eq!(via.0[0].pseudonym, "broker-a");
        assert_eq!(via.0[1].pseudonym, "broker-b");
    }

    #[test]
    fn encode_roundtrip() {
        let original = Via::with_entry("1.1", "broker-a").append("1.1", "broker-b");
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        assert_eq!(encoded.len(), 1);

        let mut iter = encoded.iter();
        let decoded = Via::decode(&mut iter).expect("roundtrip decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn via_loop_no_header_means_no_loop() {
        // No Via header -> no loop (handled by Option<Via> being None at call site)
        let via = Via::with_entry("1.1", "broker-a");
        assert!(!via.contains_pseudonym("broker-b"));
    }

    #[test]
    fn via_loop_detected_single() {
        let via = decode_from("1.1 urn:ngsi-ld:ContextSourceIdentity:broker1").expect("should decode");
        assert!(via.contains_pseudonym("urn:ngsi-ld:ContextSourceIdentity:broker1"));
    }

    #[test]
    fn via_loop_detected_multi_hop() {
        let via = decode_from("1.1 urn:ngsi-ld:ContextSourceIdentity:broker2, 1.1 urn:ngsi-ld:ContextSourceIdentity:broker1").expect("should decode");
        assert!(via.contains_pseudonym("urn:ngsi-ld:ContextSourceIdentity:broker1"));
    }

    #[test]
    fn has_alias_matches() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert!(via.has_alias("broker-a"));
        assert!(!via.has_alias("broker-c"));
    }

    #[test]
    fn has_alias_no_substring_match() {
        let via = decode_from("1.1 broker10").expect("should decode");
        assert!(!via.has_alias("broker1"));
    }
}
