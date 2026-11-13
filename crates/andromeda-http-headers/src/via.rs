//! Typed `Via` header per RFC 9110 section 7.6.3 (ETSI GS CIM 009 clause 6.3.18).
//!
//! The Via header field indicates the presence of intermediate protocols
//! and recipients between the user agent and the server. Used for loop
//! detection in distributed NGSI-LD forwarding.
//!
//! A field value is read as octets, because the comment that may close each
//! element admits `obs-text`. The list is split on the commas that stand
//! outside a comment, and each element is read on its own: one that names no
//! intermediary is dropped, and the hops beside it, on its line and on every
//! other `Via` line, still list the sources the request passed.

use crate::{
    received_protocol::ReceivedProtocol,
    via_comment::{ViaComment, comment_len},
};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use http::{HeaderName, HeaderValue};
use std::{iter, str};
use thiserror::Error;

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static VIA: HeaderName = HeaderName::from_static("via");

/// Why a single `Via` element could not be read.
///
/// The decode drops such an element alone: loop detection reads the pseudonym
/// of every other hop, so one unreadable element must not cost the chain.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ViaParseError {
    /// The element was empty.
    #[error("via element is empty")]
    Empty,
    /// The element carried no token that could name an intermediary, or the
    /// token it carried was not a usable [`ContextSourcePseudonym`].
    #[error("via element names no intermediary")]
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
    /// The comment the element closed with, absent when it carried none or one
    /// that is not a balanced comment.
    pub comment: Option<ViaComment>,
}

impl ViaEntry {
    /// Read one element like `"1.1 broker-a"` or `"HTTP/1.1 proxy (Apache)"`.
    ///
    /// Recovers the pseudonym as robustly as the loop-detection purpose demands:
    /// a malformed comment costs only the comment, since the pseudonym stands
    /// before it, and a lone token with no protocol version is kept as the
    /// pseudonym rather than dropped.
    ///
    /// # Errors
    ///
    /// Returns [`ViaParseError::Empty`] for an element of whitespace only, and
    /// [`ViaParseError::MissingPseudonym`] when no usable pseudonym precedes
    /// the comment.
    fn parse(element: &[u8]) -> Result<ViaEntry, ViaParseError> {
        let element = element.trim_ascii();
        if element.is_empty() {
            return Err(ViaParseError::Empty);
        }
        // Neither a received-protocol nor a pseudonym admits `(`, so the first
        // one opens the comment.
        let (head, comment) = match element.iter().position(|octet| *octet == b'(') {
            Some(start) => {
                let (head, comment) = element.split_at(start);
                (head, ViaComment::new(comment).ok())
            }
            None => (element, None),
        };
        let head = str::from_utf8(head).map_err(|_| ViaParseError::MissingPseudonym)?;

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

    /// The element as it is written into the list.
    fn written(&self) -> Vec<u8> {
        let mut written = Vec::new();
        if let Some(protocol) = &self.protocol {
            written.extend_from_slice(protocol.as_str().as_bytes());
            written.push(b' ');
        }
        written.extend_from_slice(self.pseudonym.as_str().as_bytes());
        if let Some(comment) = &self.comment {
            written.push(b' ');
            written.extend_from_slice(comment.as_bytes());
        }
        written
    }
}

/// The elements of one `Via` field line (RFC 9110 section 5.6.1), split on
/// the commas that stand outside a comment.
struct ViaElements<'a> {
    /// What is left of the line, `None` once its last element is out.
    rest: Option<&'a [u8]>,
}

impl<'a> Iterator for ViaElements<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let line = self.rest.take()?;
        let mut position = 0;
        while let Some(octet) = line.get(position) {
            match octet {
                b',' => {
                    let (element, rest) = line.split_at(position);
                    self.rest = rest.get(1..);
                    return Some(element);
                }
                // A comment that never closes runs to the end of the line, so
                // no comma inside it ends the element.
                b'(' => position += line.get(position..).and_then(comment_len).unwrap_or(line.len() - position),
                _other => position += 1,
            }
        }
        Some(line)
    }
}

/// Typed `Via` header (RFC 9110 section 7.6.3, ETSI GS CIM 009 clause 6.3.18).
///
/// Contains a chain of intermediary pseudonyms used for forwarding loop
/// detection in distributed NGSI-LD operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Via(Vec<ViaEntry>);

impl Via {
    /// Create a `Via` header with a single entry.
    #[must_use]
    pub fn with_entry(protocol: ReceivedProtocol, pseudonym: ContextSourcePseudonym) -> Via {
        Via(vec![ViaEntry {
            protocol: Some(protocol),
            pseudonym,
            comment: None,
        }])
    }

    /// Append an entry to the Via chain, returning the modified header.
    #[must_use]
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
    #[must_use]
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

/// The hop `element` reads as, or `None` when it names no intermediary.
fn readable_hop(element: &[u8]) -> Option<ViaEntry> {
    match ViaEntry::parse(element) {
        Ok(entry) => Some(entry),
        // An empty element between commas is a benign artefact of the RFC 9110
        // `#`-list production.
        Err(ViaParseError::Empty) => None,
        Err(error) => {
            tracing::debug!(%error, "a Via element is dropped, and the hops beside it are kept");
            None
        }
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
        let entries: Vec<ViaEntry> = values
            .flat_map(|value| ViaElements { rest: Some(value.as_bytes()) })
            .filter_map(readable_hop)
            .collect();
        if entries.is_empty() {
            return Err(headers::Error::invalid());
        }
        Ok(Via(entries))
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        let line = self.0.iter().map(ViaEntry::written).collect::<Vec<_>>().join(b", ".as_slice());
        match HeaderValue::from_bytes(&line) {
            Ok(val) => values.extend(iter::once(val)),
            Err(err) => tracing::warn!(error = %err, "failed to encode Via header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{received_protocol::ReceivedProtocol, via::Via, via_comment::ViaComment};
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

    /// Decode the `Via` field lines `lines`, each given as octets.
    fn decode_lines(lines: &[&[u8]]) -> Result<Via, headers::Error> {
        let values: Vec<HeaderValue> = lines.iter().map(|line| HeaderValue::from_bytes(line).expect("valid field value")).collect();
        Via::decode(&mut values.iter())
    }

    /// The pseudonyms of `via`, in order.
    fn pseudonyms_of(via: &Via) -> Vec<&str> {
        via.pseudonyms().map(ContextSourcePseudonym::as_str).collect()
    }

    /// The comment of each hop of `via`, as written.
    fn comments_of(via: &Via) -> Vec<Option<&[u8]>> {
        via.0.iter().map(|entry| entry.comment.as_ref().map(ViaComment::as_bytes)).collect()
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
        pretty_assertions::assert_eq!(comments_of(&via), vec![Some(b"(Apache)".as_slice())]);
    }

    #[test]
    fn decode_empty_rejects() {
        assert!(decode_from("").is_err());
    }

    /// RFC 9110 section 5.6.5 lets a comment hold a comma and a nested comment,
    /// so neither ends the hop.
    #[test]
    fn a_comma_or_a_nested_comment_inside_a_comment_keeps_the_hop_whole() {
        let via = decode_from("1.1 broker-a (proxy, (zone 2)), 1.1 broker-b").expect("should decode");
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["broker-a", "broker-b"]);
        pretty_assertions::assert_eq!(comments_of(&via), vec![Some(b"(proxy, (zone 2))".as_slice()), None]);
    }

    /// `ctext` admits `obs-text`, so octets beyond ASCII in a comment cost
    /// neither the hop nor the header.
    #[test]
    fn a_comment_holding_octets_beyond_ascii_is_read() {
        let via = decode_lines(&[b"1.1 broker-a (Z\xc3\xbcrich proxy), 1.1 broker-b (\xff)"]).expect("should decode");
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["broker-a", "broker-b"]);
        pretty_assertions::assert_eq!(comments_of(&via), vec![Some(b"(Z\xc3\xbcrich proxy)".as_slice()), Some(b"(\xff)".as_slice())]);
    }

    /// A quoted pair's second octet is data, so `\)` does not close the
    /// comment and the comma after it stays inside.
    #[test]
    fn a_comment_holding_a_quoted_pair_is_one_comment() {
        let via = decode_from(r"1.1 broker-a (a \), b), 1.1 broker-b").expect("should decode");
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["broker-a", "broker-b"]);
        pretty_assertions::assert_eq!(comments_of(&via), vec![Some(br"(a \), b)".as_slice()), None]);
    }

    #[test]
    fn a_hop_naming_no_intermediary_is_dropped_alone() {
        let via = decode_lines(&[b"1.1 broker-a, (no pseudonym), \xff\xfe broker-x", b"(only a comment)", b"1.1 broker-b"]).expect("should decode");
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["broker-a", "broker-b"]);
    }

    #[test]
    fn a_header_naming_no_intermediary_at_all_does_not_decode() {
        assert!(decode_lines(&[b"(only a comment)", b" , "]).is_err());
    }

    /// Commas inside a comment that never closes split nothing, so what
    /// follows belongs to the comment, and the malformed comment costs only
    /// itself.
    #[test]
    fn a_comment_that_never_closes_runs_to_the_end_of_its_line() {
        let via = decode_lines(&[b"1.1 broker-a (open, 1.1 broker-x", b"1.1 broker-b"]).expect("should decode");
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["broker-a", "broker-b"]);
        pretty_assertions::assert_eq!(comments_of(&via), vec![None, None]);
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
        // intermediary, so it is kept rather than dropped, and loop detection
        // can still match it.
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
        pretty_assertions::assert_eq!(pseudonyms_of(&via), vec!["a", "b", "c"]);
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

    /// The chain a forward carries keeps every received hop as written, its
    /// comment included, and the hop this broker adds carries none.
    #[test]
    fn an_extended_chain_keeps_received_comments_and_adds_a_bare_hop() {
        let received = decode_lines(&[b"1.1 broker-a (Z\xc3\xbcrich proxy, (zone 2))"]).expect("should decode");
        let mut encoded = Vec::new();
        received.append(protocol("1.1"), pseudonym("broker-b")).encode(&mut encoded);
        pretty_assertions::assert_eq!(
            encoded.iter().map(HeaderValue::as_bytes).collect::<Vec<_>>(),
            vec![b"1.1 broker-a (Z\xc3\xbcrich proxy, (zone 2)), 1.1 broker-b".as_slice()]
        );
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
