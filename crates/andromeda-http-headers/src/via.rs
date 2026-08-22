//! Typed `Via` header per RFC 9110 clause 7.6.3 (ETSI GS CIM 009 clause 6.3.18).
//!
//! The Via header field indicates the presence of intermediate protocols
//! and recipients between the user agent and the server. Used for loop
//! detection in distributed NGSI-LD forwarding.

use crate::received_protocol::ReceivedProtocol;
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use http::{HeaderName, HeaderValue};
use std::iter;
use thiserror::Error;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static VIA: HeaderName = HeaderName::from_static("via");

/// Why a single `Via` element could not be read.
///
/// Loop detection depends on every hop's pseudonym reaching
/// [`Via::contains_pseudonym`], so a parse that cannot recover a pseudonym is a
/// hard failure the header decode refuses rather than silently drops.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ViaParseError {
    /// The element was empty.
    #[error("Via element is empty")]
    Empty,
    /// The element carried no token that could name an intermediary, or the
    /// token it carried was not a usable [`ContextSourcePseudonym`].
    #[error("Via element names no intermediary")]
    MissingPseudonym,
}

/// A single entry in a Via header chain.
///
/// Each entry represents an intermediate protocol hop in the form
/// `"protocol-version pseudonym (optional comment)"`. The protocol is optional
/// because a non-conformant element with a lone token still names an
/// intermediary and is kept for loop detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaEntry {
    /// The received-protocol of the hop (e.g. `1.1` or `HTTP/1.1`), absent when
    /// the element carried only a bare pseudonym.
    pub protocol: Option<ReceivedProtocol>,
    /// The pseudonym/identity of the intermediary.
    pub pseudonym: ContextSourcePseudonym,
    /// Optional comment enclosed in parentheses. Free-form per RFC 9110, so it
    /// stays unstructured text.
    pub comment: Option<String>,
}

/// Typed `Via` header (RFC 9110 clause 7.6.3, ETSI GS CIM 009 clause 6.3.18).
///
/// Contains a chain of intermediary pseudonyms used for forwarding loop
/// detection in distributed NGSI-LD operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via(Vec<ViaEntry>);

impl Via {
    /// Create a `Via` header with a single entry.
    pub fn with_entry(protocol: ReceivedProtocol, pseudonym: ContextSourcePseudonym) -> Via {
        Via(vec![ViaEntry {
            protocol: Some(protocol),
            pseudonym,
            comment: None,
        }])
    }

    /// Append an entry to the Via chain, returning the modified header.
    pub fn append(mut self, protocol: ReceivedProtocol, pseudonym: ContextSourcePseudonym) -> Via {
        self.0.push(ViaEntry {
            protocol: Some(protocol),
            pseudonym,
            comment: None,
        });
        self
    }

    /// Check if the Via chain contains the given pseudonym (exact match).
    ///
    /// Used for loop detection per clause 6.3.18: compares against each
    /// discrete pseudonym, not substring match.
    pub fn contains_pseudonym(&self, identity: &ContextSourcePseudonym) -> bool {
        self.0.iter().any(|entry| &entry.pseudonym == identity)
    }

    /// The pseudonym of every hop in the chain, in the order they were
    /// traversed.
    ///
    /// Clause 5.12 excludes from a registration match every context source the
    /// listing of previously encountered sources names; this is that listing.
    pub fn pseudonyms(&self) -> impl Iterator<Item = &ContextSourcePseudonym> {
        self.0.iter().map(|entry| &entry.pseudonym)
    }
}

/// Parse a single Via element like `"1.1 broker-a"` or `"HTTP/1.1 proxy (Apache)"`.
///
/// Recovers the pseudonym as robustly as the loop-detection purpose demands: an
/// unterminated comment does not discard the element (the pseudonym before `(`
/// is still read), and a lone token with no protocol version is kept as the
/// pseudonym rather than dropped. An element that yields no pseudonym at all is
/// the only hard failure, so a hop naming this broker can never be silently
/// lost the way `Option`-dropping an unparseable element allowed.
fn parse_via_entry(s: &str) -> Result<ViaEntry, ViaParseError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(ViaParseError::Empty);
    }

    // Separate an optional trailing comment. A missing `)` is tolerated: the
    // pseudonym sits before the `(`, so a malformed comment must not cost the hop.
    let (head, comment) = match trimmed.split_once('(') {
        Some((head, tail)) => {
            let comment = tail.strip_suffix(')').unwrap_or(tail).trim();
            (head.trim(), (!comment.is_empty()).then(|| comment.to_string()))
        }
        None => (trimmed, None),
    };

    // received-protocol RWS received-by: the pseudonym is the second
    // whitespace-separated token. A single token has no protocol version, which
    // is non-conformant but still names an intermediary, so it is taken as the
    // pseudonym rather than discarded (fail closed for loop detection).
    let mut tokens = head.split_whitespace();
    let first = tokens.next().ok_or(ViaParseError::MissingPseudonym)?;
    let (protocol, pseudonym) = match tokens.next() {
        // A first token that does not parse as a received-protocol is dropped
        // rather than costing the hop; only the pseudonym is load-bearing.
        Some(second) => (
            ReceivedProtocol::new(first).ok(),
            ContextSourcePseudonym::new(second).map_err(|_| ViaParseError::MissingPseudonym)?,
        ),
        None => (None, ContextSourcePseudonym::new(first).map_err(|_| ViaParseError::MissingPseudonym)?),
    };

    Ok(ViaEntry { protocol, pseudonym, comment })
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
                // An empty element between commas is a benign artefact of the
                // RFC 9110 `#`-list production and is skipped; every other
                // element must parse, so a malformed hop fails the decode rather
                // than being silently dropped.
                if part.trim().is_empty() {
                    continue;
                }
                entries.push(parse_via_entry(part).map_err(|_| headers::Error::invalid())?);
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
            .map(|entry| {
                let received = match &entry.protocol {
                    Some(protocol) => format!("{protocol} {}", entry.pseudonym),
                    None => entry.pseudonym.to_string(),
                };
                match &entry.comment {
                    Some(comment) => format!("{received} ({comment})"),
                    None => received,
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        match HeaderValue::from_str(&s) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Via header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{received_protocol::ReceivedProtocol, via::Via};
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use headers::Header;
    use http::HeaderValue;
    use std::iter;

    /// A valid received-protocol, for building outgoing entries in tests.
    fn protocol(value: &str) -> ReceivedProtocol {
        ReceivedProtocol::new(value).expect("valid received-protocol")
    }

    /// A valid pseudonym, for building outgoing entries in tests.
    fn pseudonym(value: &str) -> ContextSourcePseudonym {
        ContextSourcePseudonym::new(value).expect("valid pseudonym")
    }

    fn decode_from(raw: &str) -> Result<Via, headers::Error> {
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        Via::decode(&mut iter)
    }

    #[test]
    fn decode_single_entry() {
        let via = decode_from("1.1 broker-a").expect("should decode");
        pretty_assertions::assert_eq!(via.0.len(), 1);
        pretty_assertions::assert_eq!(via.0[0].protocol.as_ref().map(ReceivedProtocol::as_str), Some("1.1"));
        pretty_assertions::assert_eq!(via.0[0].pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(via.0[0].comment, None);
    }

    #[test]
    fn decode_multi_entry() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        pretty_assertions::assert_eq!(via.0.len(), 2);
        pretty_assertions::assert_eq!(via.0[0].pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(via.0[1].pseudonym.as_str(), "broker-b");
    }

    #[test]
    fn decode_with_full_protocol() {
        let via = decode_from("HTTP/1.1 proxy").expect("should decode");
        pretty_assertions::assert_eq!(via.0[0].protocol.as_ref().map(ReceivedProtocol::as_str), Some("HTTP/1.1"));
        pretty_assertions::assert_eq!(via.0[0].pseudonym.as_str(), "proxy");
    }

    #[test]
    fn decode_with_comment() {
        let via = decode_from("1.1 proxy (Apache)").expect("should decode");
        pretty_assertions::assert_eq!(via.0[0].pseudonym.as_str(), "proxy");
        pretty_assertions::assert_eq!(via.0[0].comment, Some("Apache".to_string()));
    }

    #[test]
    fn decode_empty_rejects() {
        assert!(decode_from("").is_err());
    }

    #[test]
    fn a_hop_with_a_malformed_comment_is_preserved_not_silently_dropped() {
        // Loop detection reads every hop's pseudonym; an element this parser
        // cannot fully read (here, an unterminated comment) must still
        // contribute its pseudonym, or a cycle back through this broker could
        // slip past `contains_pseudonym`.
        let via = decode_from("1.1 broker-a, 1.1 broker-b (unterminated").expect("should decode");
        assert!(via.contains_pseudonym(&pseudonym("broker-a")));
        assert!(
            via.contains_pseudonym(&pseudonym("broker-b")),
            "a hop with a malformed comment must not be lost"
        );
    }

    #[test]
    fn a_version_less_token_is_kept_as_a_pseudonym() {
        // A non-conformant element with no protocol version still names an
        // intermediary, so it is kept rather than dropped -- fail closed, so
        // loop detection can still match it.
        let via = decode_from("brokerX").expect("should decode");
        pretty_assertions::assert_eq!(via.0[0].protocol, None);
        assert!(via.contains_pseudonym(&pseudonym("brokerX")));
    }

    #[test]
    fn an_empty_element_between_commas_is_skipped_but_the_rest_survive() {
        let via = decode_from("1.1 broker-a, , 1.1 broker-b").expect("should decode");
        assert!(via.contains_pseudonym(&pseudonym("broker-a")));
        assert!(via.contains_pseudonym(&pseudonym("broker-b")));
    }

    #[test]
    fn pseudonyms_extracts_all() {
        let via = decode_from("1.1 a, 1.1 b, 1.1 c").expect("should decode");
        let pseudonyms: Vec<&str> = via.pseudonyms().map(ContextSourcePseudonym::as_str).collect();
        pretty_assertions::assert_eq!(pseudonyms, vec!["a", "b", "c"]);
    }

    #[test]
    fn contains_pseudonym_positive() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert!(via.contains_pseudonym(&pseudonym("broker-a")));
    }

    #[test]
    fn contains_pseudonym_negative() {
        let via = decode_from("1.1 broker-a, 1.1 broker-b").expect("should decode");
        assert!(!via.contains_pseudonym(&pseudonym("broker-c")));
    }

    #[test]
    fn contains_pseudonym_no_prefix_match() {
        let via = decode_from("1.1 broker10").expect("should decode");
        assert!(!via.contains_pseudonym(&pseudonym("broker1")));
    }

    #[test]
    fn contains_pseudonym_no_suffix_match() {
        let via = decode_from("1.1 my-broker1").expect("should decode");
        assert!(!via.contains_pseudonym(&pseudonym("broker1")));
    }

    #[test]
    fn append_adds_to_chain() {
        let via = Via::with_entry(protocol("1.1"), pseudonym("broker-a")).append(protocol("1.1"), pseudonym("broker-b"));
        pretty_assertions::assert_eq!(via.0.len(), 2);
        pretty_assertions::assert_eq!(via.0[0].pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(via.0[1].pseudonym.as_str(), "broker-b");
    }

    #[test]
    fn encode_roundtrip() {
        let original = Via::with_entry(protocol("1.1"), pseudonym("broker-a")).append(protocol("1.1"), pseudonym("broker-b"));
        let mut encoded = Vec::new();
        original.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);

        let mut iter = encoded.iter();
        let decoded = Via::decode(&mut iter).expect("roundtrip decode");
        pretty_assertions::assert_eq!(original, decoded);
    }

    #[test]
    fn via_loop_no_header_means_no_loop() {
        // No Via header -> no loop (handled by Option<Via> being None at call site)
        let via = Via::with_entry(protocol("1.1"), pseudonym("broker-a"));
        assert!(!via.contains_pseudonym(&pseudonym("broker-b")));
    }

    #[test]
    fn via_loop_detected_single() {
        let via = decode_from("1.1 urn:ngsi-ld:ContextSourceIdentity:broker1").expect("should decode");
        assert!(via.contains_pseudonym(&pseudonym("urn:ngsi-ld:ContextSourceIdentity:broker1")));
    }

    #[test]
    fn via_loop_detected_multi_hop() {
        let via = decode_from("1.1 urn:ngsi-ld:ContextSourceIdentity:broker2, 1.1 urn:ngsi-ld:ContextSourceIdentity:broker1").expect("should decode");
        assert!(via.contains_pseudonym(&pseudonym("urn:ngsi-ld:ContextSourceIdentity:broker1")));
    }
}
