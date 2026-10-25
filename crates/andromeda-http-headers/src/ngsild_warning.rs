//! Typed `NGSILD-Warning` response header (ETSI GS CIM 009 clause 6.3.17).
//!
//! Uses IANA Warning Codes to signal non-fatal issues encountered during
//! distributed operations (forwarding to context sources). An encoded header
//! lists its warnings up to a byte budget and summarises the rest per code;
//! the per-source record lives on each registration (`status`, `timesFailed`,
//! `lastFailure`, clause 5.2.9).

use crate::link::quoting::SplitAsciiUnquoted;
use http::{HeaderName, HeaderValue};
use indexmap::IndexMap;
use std::{collections::HashSet, iter};

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_WARNING: HeaderName = HeaderName::from_static("ngsild-warning");

/// Summed encoded length of the warnings a header lists before it summarises
/// the rest, which keeps the header inside a 4 KiB response-header buffer next
/// to `Link`, `NGSILD-Results-Count` and the other response headers.
const LISTED_WARNING_BYTES: usize = 2048;

/// Bytes a listed value adds beyond its own text: the `, ` that joins it to the
/// next value when a client or proxy folds the header into one line.
const VALUE_SEPARATOR_BYTES: usize = 2;

/// IANA warning codes for distributed operations (clause 6.3.17).
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display, strum::EnumDiscriminants)]
#[strum_discriminants(name(WarnCode), derive(Hash), doc = "The IANA warn-code of an [`NgsiLdWarning`], without its text.")]
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

impl NgsiLdWarning {
    /// The warning with the given code and text.
    const fn with_code(code: WarnCode, text: String) -> NgsiLdWarning {
        match code {
            WarnCode::ResponseIsStale => NgsiLdWarning::ResponseIsStale(text),
            WarnCode::RevalidationFailed => NgsiLdWarning::RevalidationFailed(text),
            WarnCode::MiscellaneousWarning => NgsiLdWarning::MiscellaneousWarning(text),
            WarnCode::MiscellaneousPersistentWarning => NgsiLdWarning::MiscellaneousPersistentWarning(text),
        }
    }
}

/// The text of the summary that stands for the warnings of one code a header
/// does not list.
#[derive(Debug, thiserror::Error)]
#[error("{count} further warnings with this code are not listed")]
struct UnlistedWarnings {
    /// Number of distinct warnings with the code that are left out.
    count: usize,
}

/// Typed `NGSILD-Warning` response header (clause 6.3.17).
///
/// Wraps multiple [`NgsiLdWarning`] values for multi-value header encoding.
#[derive(Debug, Clone)]
pub struct NgsiLdWarningHeader(Vec<NgsiLdWarning>);

impl NgsiLdWarningHeader {
    /// The `NGSILD-Warning` header for the given warnings, bounded for the wire.
    ///
    /// Exact duplicates are dropped, keeping first occurrences. The remaining
    /// warnings are listed in order while their encoded length fits 2 KiB;
    /// from the first one that does not fit, every
    /// warning is counted instead, and one summary per code with omissions
    /// follows the listed ones, in order of first omission. Clause 6.3.17
    /// requires the abnormal behaviour to be indicated with its code, not one
    /// value per source.
    #[must_use]
    pub fn new(warnings: Vec<NgsiLdWarning>) -> NgsiLdWarningHeader {
        let first_occurrences: Vec<bool> = {
            let mut seen = HashSet::with_capacity(warnings.len());
            warnings.iter().map(|warning| seen.insert(warning)).collect()
        };
        let mut budget = LISTED_WARNING_BYTES;
        let mut listed = Vec::new();
        let mut omitted: IndexMap<WarnCode, usize> = IndexMap::new();
        for warning in iter::zip(warnings, first_occurrences).filter_map(|(warning, first)| first.then_some(warning)) {
            if omitted.is_empty() {
                let cost = warning.to_string().len() + VALUE_SEPARATOR_BYTES;
                if let Some(left) = budget.checked_sub(cost) {
                    budget = left;
                    listed.push(warning);
                    continue;
                }
            }
            *omitted.entry(WarnCode::from(&warning)).or_default() += 1;
        }
        listed.extend(
            omitted
                .into_iter()
                .map(|(code, count)| NgsiLdWarning::with_code(code, UnlistedWarnings { count }.to_string())),
        );
        NgsiLdWarningHeader(listed)
    }

    /// The warnings the header carries, in wire order.
    #[must_use]
    pub fn into_warnings(self) -> Vec<NgsiLdWarning> {
        self.0
    }
}

/// Parse a single warning value like `199 - "timeout"` into an `NgsiLdWarning`.
fn parse_warning(s: &str) -> Option<NgsiLdWarning> {
    let trimmed = s.trim();
    let (code_str, rest) = trimmed.split_once(char::is_whitespace)?;
    let code: u16 = code_str.parse().ok()?;

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
            // but warning text itself may contain commas inside quotes, and an
            // RFC 9110 quoted-pair (`\"`) inside the text must not end the quote.
            for segment in SplitAsciiUnquoted::new(s, ",") {
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
                Ok(val) => values.extend(iter::once(val)),
                Err(err) => tracing::warn!(error = %err, "failed to encode NGSILD-Warning header value"),
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
    fn a_decoded_header_hands_back_its_warnings_in_wire_order() {
        let val = HeaderValue::from_str("199 - \"first\", 299 - \"second\"").expect("valid");
        let mut iter = iter::once(&val);
        let warnings = NgsiLdWarningHeader::decode(&mut iter).expect("should decode").into_warnings();
        pretty_assertions::assert_eq!(
            warnings,
            vec![
                NgsiLdWarning::MiscellaneousWarning("first".to_string()),
                NgsiLdWarning::MiscellaneousPersistentWarning("second".to_string())
            ]
        );
    }

    #[test]
    fn decode_parses_response_is_stale() {
        let val = HeaderValue::from_str("110 - \"cached\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::ResponseIsStale("cached".to_string())]);
    }

    #[test]
    fn decode_parses_revalidation_failed() {
        let val = HeaderValue::from_str("111 - \"bad\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::RevalidationFailed("bad".to_string())]);
    }

    #[test]
    fn decode_parses_miscellaneous_warning() {
        let val = HeaderValue::from_str("199 - \"timeout\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::MiscellaneousWarning("timeout".to_string())]);
    }

    #[test]
    fn decode_parses_persistent_warning() {
        let val = HeaderValue::from_str("299 - \"forbidden\"").expect("valid");
        let mut iter = iter::once(&val);
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
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode multiple");
        pretty_assertions::assert_eq!(header.0.len(), 2);
        pretty_assertions::assert_eq!(header.0[0], NgsiLdWarning::MiscellaneousWarning("timeout".to_string()));
        pretty_assertions::assert_eq!(header.0[1], NgsiLdWarning::MiscellaneousPersistentWarning("forbidden".to_string()));
    }

    #[test]
    fn decode_warning_with_comma_in_quoted_text() {
        let raw = "199 - \"timeout, retry later\"";
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.len(), 1);
        pretty_assertions::assert_eq!(header.0[0], NgsiLdWarning::MiscellaneousWarning("timeout, retry later".to_string()));
    }

    #[test]
    fn decode_warning_with_an_escaped_quote_before_a_comma_is_not_split() {
        // RFC 9110 warn-text is a quoted-string admitting quoted-pairs, so an
        // escaped quote (`\"`) must not end the quoted run and let the following
        // comma split the value into two malformed warnings; splitting goes
        // through `SplitAsciiUnquoted`, which honours the escape.
        let raw = r#"110 - "a\", b""#;
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode as one warning");
        pretty_assertions::assert_eq!(header.0.len(), 1);
    }

    fn unreachable_source(index: usize) -> NgsiLdWarning {
        NgsiLdWarning::MiscellaneousWarning(format!(
            "context source urn:ngsi-ld:ContextSourceRegistration:{index} did not respond: connection refused"
        ))
    }

    fn encoded(header: &NgsiLdWarningHeader) -> Vec<String> {
        let mut values = Vec::new();
        header.encode(&mut values);
        values.iter().map(|value| value.to_str().expect("valid str").to_owned()).collect()
    }

    fn listed_bytes(values: &[String]) -> usize {
        values.iter().map(|value| value.len() + VALUE_SEPARATOR_BYTES).sum()
    }

    #[test]
    fn warnings_within_the_budget_are_all_listed_in_order() {
        let warnings = vec![
            NgsiLdWarning::MiscellaneousWarning("context source s1 did not respond".to_owned()),
            NgsiLdWarning::MiscellaneousPersistentWarning("context source s2 returned 403".to_owned()),
            NgsiLdWarning::ResponseIsStale("context source s3 answered from cache".to_owned()),
        ];
        let header = NgsiLdWarningHeader::new(warnings.clone()); // clone: the expectation outlives the consumed input
        pretty_assertions::assert_eq!(header.into_warnings(), warnings);
    }

    #[test]
    fn exact_duplicates_are_listed_once() {
        let header = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::MiscellaneousWarning("a".to_owned()),
            NgsiLdWarning::MiscellaneousPersistentWarning("b".to_owned()),
            NgsiLdWarning::MiscellaneousWarning("a".to_owned()),
        ]);
        pretty_assertions::assert_eq!(encoded(&header), ["199 - \"a\"", "299 - \"b\""]);
    }

    #[test]
    fn four_thousand_unreachable_sources_fit_the_budget_with_one_summary() {
        let warnings: Vec<NgsiLdWarning> = (0..4000).map(unreachable_source).collect();
        let values = encoded(&NgsiLdWarningHeader::new(warnings.clone())); // clone: the prefix check reads the input again
        let (summary, listed) = values.split_last().expect("a summary");
        assert!(listed_bytes(listed) <= LISTED_WARNING_BYTES);
        assert!(listed.iter().zip(&warnings).all(|(value, warning)| *value == warning.to_string()));
        pretty_assertions::assert_eq!(
            summary.as_str(),
            format!("199 - \"{} further warnings with this code are not listed\"", 4000 - listed.len())
        );
        pretty_assertions::assert_eq!(values.len(), listed.len() + 1);
    }

    #[test]
    fn each_code_with_omitted_warnings_gets_its_own_summary() {
        let text = |index: usize| format!("context source urn:ngsi-ld:ContextSourceRegistration:{index:04} failed its forwarded query");
        let warnings: Vec<NgsiLdWarning> = (0..100)
            .map(|index| NgsiLdWarning::MiscellaneousWarning(text(index)))
            .chain((100..200).map(|index| NgsiLdWarning::MiscellaneousPersistentWarning(text(index))))
            .collect();
        let values = encoded(&NgsiLdWarningHeader::new(warnings));
        let [listed @ .., first, second] = values.as_slice() else {
            panic!("expected two summaries, got {values:?}");
        };
        let count = |summary: &str, code: &str| -> usize {
            summary
                .strip_prefix(&format!("{code} - \""))
                .and_then(|rest| rest.strip_suffix(" further warnings with this code are not listed\""))
                .and_then(|count| count.parse().ok())
                .expect("a summary with a count")
        };
        pretty_assertions::assert_eq!(count(first, "199") + count(second, "299") + listed.len(), 200);
    }

    #[test]
    fn a_code_seen_only_after_the_budget_is_spent_still_appears() {
        let warnings: Vec<NgsiLdWarning> = (0..200)
            .map(unreachable_source)
            .chain(iter::once(NgsiLdWarning::RevalidationFailed("context source s answered garbage".to_owned())))
            .collect();
        let values = encoded(&NgsiLdWarningHeader::new(warnings));
        assert!(values.iter().any(|value| value == "111 - \"1 further warnings with this code are not listed\""));
    }

    #[test]
    fn a_first_warning_larger_than_the_budget_is_summarised_not_dropped() {
        let header = NgsiLdWarningHeader::new(vec![NgsiLdWarning::MiscellaneousPersistentWarning("x".repeat(3000))]);
        pretty_assertions::assert_eq!(encoded(&header), ["299 - \"1 further warnings with this code are not listed\""]);
    }

    #[test]
    fn summaries_decode_with_their_codes() {
        let warnings: Vec<NgsiLdWarning> = (0..100)
            .map(unreachable_source)
            .chain((0..100).map(|index| NgsiLdWarning::MiscellaneousPersistentWarning(format!("context source {index} returned HTTP 500"))))
            .collect();
        let mut values = Vec::new();
        NgsiLdWarningHeader::new(warnings).encode(&mut values);
        let decoded = NgsiLdWarningHeader::decode(&mut values.iter()).expect("should decode").into_warnings();
        let [.., NgsiLdWarning::MiscellaneousWarning(_), NgsiLdWarning::MiscellaneousPersistentWarning(_)] = decoded.as_slice() else {
            panic!("expected a 199 then a 299 summary, got {decoded:?}");
        };
    }

    #[test]
    fn decoding_does_not_bound() {
        let combined = (0..300).map(|index| unreachable_source(index).to_string()).collect::<Vec<_>>().join(", ");
        let value = HeaderValue::from_str(&combined).expect("valid header value");
        let decoded = NgsiLdWarningHeader::decode(&mut iter::once(&value)).expect("should decode");
        pretty_assertions::assert_eq!(decoded.into_warnings().len(), 300);
    }
}
