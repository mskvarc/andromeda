//! Typed `NGSILD-Warning` response header (ETSI GS CIM 009 clause 6.3.17).
//!
//! Uses IANA Warning Codes to signal non-fatal issues encountered during
//! distributed operations (forwarding to context sources). An encoded header
//! lists its warnings up to a byte budget and summarises the rest per code;
//! the per-source record lives on each registration (`status`, `timesFailed`,
//! `lastFailure`, clause 5.2.9). Each value's text is bounded and quoted by
//! [`WarnText`], whether this broker wrote it or relays it from a source.

use crate::{link::quoting::SplitAsciiUnquoted, warn_text::WarnText};
use http::{HeaderName, HeaderValue};
use indexmap::IndexMap;
use std::{collections::HashSet, fmt::Display, iter};

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_WARNING: HeaderName = HeaderName::from_static("ngsild-warning");

/// Summed encoded length of the warnings a header lists before it summarises
/// the rest, which keeps the header inside a 4 KiB response-header buffer next
/// to `Link`, `NGSILD-Results-Count` and the other response headers.
const LISTED_WARNING_BYTES: usize = 2048;

/// Bytes a listed value adds beyond its own text: the `, ` that joins it to the
/// next value when a client or proxy folds the header into one line.
const VALUE_SEPARATOR_BYTES: usize = 2;

/// The IANA warn-codes of Table 6.3.17-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString)]
pub enum WarnCode {
    /// 110: cached data was reused instead of asking the registration endpoint.
    #[strum(serialize = "110")]
    ResponseIsStale,
    /// 111: a response arrived in time, but its payload cannot be used.
    #[strum(serialize = "111")]
    RevalidationFailed,
    /// 199: no response arrived in time, or a registration loop was detected.
    #[strum(serialize = "199")]
    MiscellaneousWarning,
    /// 299: the registration endpoint answered with an error response.
    #[strum(serialize = "299")]
    MiscellaneousPersistentWarning,
}

/// One `NGSILD-Warning` value: a warn-code and its text, with `-` as the
/// warn-agent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
#[display("{code} - \"{}\"", text.quoted())]
pub struct NgsiLdWarning {
    /// What kind of abnormal behaviour the warning reports.
    code: WarnCode,
    /// What happened, in words.
    text: WarnText,
}

impl NgsiLdWarning {
    /// The warning with `code`, reporting `text`.
    #[must_use]
    pub fn new(code: WarnCode, text: impl Display) -> NgsiLdWarning {
        NgsiLdWarning {
            code,
            text: WarnText::new(text),
        }
    }

    /// The warn-code.
    #[must_use]
    pub const fn code(&self) -> WarnCode {
        self.code
    }

    /// The warn-text.
    #[must_use]
    pub const fn text(&self) -> &WarnText {
        &self.text
    }

    /// The warn-code and the warn-text, taken apart.
    #[must_use]
    pub fn into_parts(self) -> (WarnCode, WarnText) {
        (self.code, self.text)
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
            *omitted.entry(warning.code).or_default() += 1;
        }
        listed.extend(omitted.into_iter().map(|(code, count)| NgsiLdWarning::new(code, UnlistedWarnings { count })));
        NgsiLdWarningHeader(listed)
    }

    /// The warnings the header carries, in wire order.
    #[must_use]
    pub fn into_warnings(self) -> Vec<NgsiLdWarning> {
        self.0
    }
}

/// Parse a single warning value like `199 - "timeout"` into an `NgsiLdWarning`,
/// its text bounded as any other warning's.
fn parse_warning(s: &str) -> Option<NgsiLdWarning> {
    let (code, rest) = s.trim().split_once(char::is_whitespace)?;
    let code = code.parse().ok()?;
    let start = rest.find('"')? + 1;
    let end = rest.rfind('"')?;
    if start >= end {
        return None;
    }
    Some(NgsiLdWarning {
        code,
        text: WarnText::from_quoted(&rest[start..end]),
    })
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
    use crate::{
        ngsild_warning::{LISTED_WARNING_BYTES, NgsiLdWarning, NgsiLdWarningHeader, VALUE_SEPARATOR_BYTES, WarnCode},
        warn_text::WARN_TEXT_BYTES,
    };
    use headers::Header;
    use http::HeaderValue;
    use std::iter;

    #[test]
    fn encode_single_warning() {
        let header = NgsiLdWarningHeader::new(vec![NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "msg".to_string())]);
        let mut encoded = Vec::new();
        header.encode(&mut encoded);
        pretty_assertions::assert_eq!(encoded.len(), 1);
        pretty_assertions::assert_eq!(encoded[0].to_str().expect("valid str"), "199 - \"msg\"");
    }

    #[test]
    fn encode_multiple_warnings() {
        let header = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached".to_string()),
            NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "forbidden".to_string()),
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
                NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "first".to_string()),
                NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "second".to_string())
            ]
        );
    }

    #[test]
    fn decode_parses_response_is_stale() {
        let val = HeaderValue::from_str("110 - \"cached\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached".to_string())]);
    }

    #[test]
    fn decode_parses_revalidation_failed() {
        let val = HeaderValue::from_str("111 - \"bad\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.as_slice(), &[NgsiLdWarning::new(WarnCode::RevalidationFailed, "bad".to_string())]);
    }

    #[test]
    fn decode_parses_miscellaneous_warning() {
        let val = HeaderValue::from_str("199 - \"timeout\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(
            header.0.as_slice(),
            &[NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string())]
        );
    }

    #[test]
    fn decode_parses_persistent_warning() {
        let val = HeaderValue::from_str("299 - \"forbidden\"").expect("valid");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(
            header.0.as_slice(),
            &[NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "forbidden".to_string())]
        );
    }

    #[test]
    fn roundtrip_all_variants() {
        let original = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::new(WarnCode::ResponseIsStale, "stale".to_string()),
            NgsiLdWarning::new(WarnCode::RevalidationFailed, "invalid".to_string()),
            NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string()),
            NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "error".to_string()),
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
        let w = NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "199 - \"timeout\"");
    }

    #[test]
    fn warning_header_format_persistent() {
        let w = NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "403 Forbidden".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "299 - \"403 Forbidden\"");
    }

    #[test]
    fn warning_header_format_response_is_stale() {
        let w = NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached data".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "110 - \"cached data\"");
    }

    #[test]
    fn warning_header_format_revalidation_failed() {
        let w = NgsiLdWarning::new(WarnCode::RevalidationFailed, "invalid JSON".to_string());
        pretty_assertions::assert_eq!(w.to_string(), "111 - \"invalid JSON\"");
    }

    #[test]
    fn decode_comma_separated_warnings_in_single_header_value() {
        let combined = "199 - \"timeout\", 299 - \"forbidden\"";
        let val = HeaderValue::from_str(combined).expect("valid header value");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode multiple");
        pretty_assertions::assert_eq!(header.0.len(), 2);
        pretty_assertions::assert_eq!(header.0[0], NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout".to_string()));
        pretty_assertions::assert_eq!(
            header.0[1],
            NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "forbidden".to_string())
        );
    }

    #[test]
    fn decode_warning_with_comma_in_quoted_text() {
        let raw = "199 - \"timeout, retry later\"";
        let val = HeaderValue::from_str(raw).expect("valid header value");
        let mut iter = iter::once(&val);
        let header = NgsiLdWarningHeader::decode(&mut iter).expect("should decode");
        pretty_assertions::assert_eq!(header.0.len(), 1);
        pretty_assertions::assert_eq!(
            header.0[0],
            NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout, retry later".to_string())
        );
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
        NgsiLdWarning::new(
            WarnCode::MiscellaneousWarning,
            format!("context source urn:ngsi-ld:ContextSourceRegistration:{index} did not respond: connection refused"),
        )
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
            NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "context source s1 did not respond".to_owned()),
            NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "context source s2 returned 403".to_owned()),
            NgsiLdWarning::new(WarnCode::ResponseIsStale, "context source s3 answered from cache".to_owned()),
        ];
        let header = NgsiLdWarningHeader::new(warnings.clone()); // clone: the expectation outlives the consumed input
        pretty_assertions::assert_eq!(header.into_warnings(), warnings);
    }

    #[test]
    fn exact_duplicates_are_listed_once() {
        let header = NgsiLdWarningHeader::new(vec![
            NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "a".to_owned()),
            NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "b".to_owned()),
            NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "a".to_owned()),
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
            .map(|index| NgsiLdWarning::new(WarnCode::MiscellaneousWarning, text(index)))
            .chain((100..200).map(|index| NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, text(index))))
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
            .chain(iter::once(NgsiLdWarning::new(
                WarnCode::RevalidationFailed,
                "context source s answered garbage".to_owned(),
            )))
            .collect();
        let values = encoded(&NgsiLdWarningHeader::new(warnings));
        assert!(values.iter().any(|value| value == "111 - \"1 further warnings with this code are not listed\""));
    }

    #[test]
    fn a_warning_longer_than_the_budget_is_listed_cut_to_the_text_bound() {
        let header = NgsiLdWarningHeader::new(vec![NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "x".repeat(3000))]);
        let [value] = encoded(&header).try_into().expect("one listed warning");
        pretty_assertions::assert_eq!(value, format!("299 - \"{}...\"", "x".repeat(WARN_TEXT_BYTES - 3)));
    }

    #[test]
    fn summaries_decode_with_their_codes() {
        let warnings: Vec<NgsiLdWarning> = (0..100)
            .map(unreachable_source)
            .chain((0..100).map(|index| NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, format!("context source {index} returned HTTP 500"))))
            .collect();
        let mut values = Vec::new();
        NgsiLdWarningHeader::new(warnings).encode(&mut values);
        let decoded = NgsiLdWarningHeader::decode(&mut values.iter()).expect("should decode").into_warnings();
        let codes: Vec<WarnCode> = decoded.iter().map(NgsiLdWarning::code).collect();
        assert!(
            codes.ends_with(&[WarnCode::MiscellaneousWarning, WarnCode::MiscellaneousPersistentWarning]),
            "expected a 199 then a 299 summary, got {decoded:?}"
        );
    }

    #[test]
    fn decoding_does_not_bound() {
        let combined = (0..300).map(|index| unreachable_source(index).to_string()).collect::<Vec<_>>().join(", ");
        let value = HeaderValue::from_str(&combined).expect("valid header value");
        let decoded = NgsiLdWarningHeader::decode(&mut iter::once(&value)).expect("should decode");
        pretty_assertions::assert_eq!(decoded.into_warnings().len(), 300);
    }

    /// Clause 6.3.17 relays a source's own warnings; one whose text runs past
    /// the bound is relayed cut, not whole.
    #[test]
    fn a_relayed_warning_is_cut_to_the_text_bound() {
        let relayed = HeaderValue::from_str(&format!("299 - \"{}\"", "x".repeat(147_000))).expect("valid header value");
        let [warning] = NgsiLdWarningHeader::decode(&mut iter::once(&relayed))
            .expect("should decode")
            .into_warnings()
            .try_into()
            .expect("one warning");
        pretty_assertions::assert_eq!(warning.code(), WarnCode::MiscellaneousPersistentWarning);
        pretty_assertions::assert_eq!(warning.text().quoted().to_string().len(), WARN_TEXT_BYTES);
        pretty_assertions::assert_eq!(warning.to_string().len(), WARN_TEXT_BYTES + "299 - \"\"".len());
    }

    #[test]
    fn a_text_with_quotes_and_non_ascii_encodes_as_a_header_value_and_decodes_back() {
        let warning = NgsiLdWarning::new(WarnCode::RevalidationFailed, "source in Zürich said \"no\", twice");
        let values = encoded(&NgsiLdWarningHeader::new(vec![warning]));
        pretty_assertions::assert_eq!(values, [r#"111 - "source in Z%C3%BCrich said \"no\", twice""#]);
        let value = HeaderValue::from_str(&values[0]).expect("valid header value");
        let decoded = NgsiLdWarningHeader::decode(&mut iter::once(&value)).expect("should decode").into_warnings();
        pretty_assertions::assert_eq!(
            decoded,
            [NgsiLdWarning::new(WarnCode::RevalidationFailed, "source in Z%C3%BCrich said \"no\", twice")]
        );
    }

    #[test]
    fn an_unknown_warn_code_is_not_read() {
        let value = HeaderValue::from_str("214 - \"transformed\", 199 - \"timeout\"").expect("valid header value");
        let decoded = NgsiLdWarningHeader::decode(&mut iter::once(&value)).expect("should decode").into_warnings();
        pretty_assertions::assert_eq!(decoded, [NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout")]);
    }
}
