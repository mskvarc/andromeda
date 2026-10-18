//! The single RFC 3339 rendering Andromeda applies to every timestamp it emits
//! from an instant it holds.
//!
//! Chrono's own `Serialize` emits sub-second precision that varies with the
//! stored value in ways NGSI-LD does not want. Andromeda renders one form
//! through [`sub_second`]: UTC, the literal `Z` designator, and exactly the
//! sub-second precision the instant carries and no more -- a whole second as a
//! bare `Z`, a millisecond instant as `.sss`, a microsecond one as `.ssssss`.
//!
//! This one rendering serves every instant the broker emits: the broker-minted
//! `createdAt`, `modifiedAt`, `deletedAt`, `expiresAt` and a subscription's
//! `lastNotification`, and the temporal-representation members rebuilt from
//! stored instants (`observedAt`, `DateTime` values, aggregation bucket
//! bounds). The reason it drops the fraction from a whole second rather than
//! zero-padding to six digits is conformance: the ETSI test suite writes an
//! `observedAt` or `expiresAt` such as `2020-08-01T12:03:00Z` and requires the
//! temporal retrieval to return that exact string, and the spec's own Annex C
//! examples spell a whole-second instant with no fraction. A fixed six-digit
//! rendering (`.000000Z`) fails those exact-match cases. Clause 4.6.3 admits
//! "one or more fractional digits, up to a maximum of six", so both spellings
//! name the same instant, and the minimal one is what round-trips.
//!
//! This is not applied to a `DateTime` value a *client* supplied on the Core
//! Entity API: clause 4.6.4 makes the broker "preserve the representation of
//! the content of the values ... and return the original content", so that
//! value is stored and served as the verbatim string the client wrote (see
//! `andromeda_ngsi_ld_model::attribute::value`), never re-rendered here. The
//! temporal representation carries no such string -- it stores the instant in a
//! microsecond `TIMESTAMPTZ` column -- so it renders through [`sub_second`]
//! like any other broker-held instant. The serde adapters below are that same
//! rendering reached through `#[serde(with = …)]`.

use crate::fractional_seconds::{FractionalSecondsError, accept_comma_decimal_separator, reject_excess_fractional_digits};
use chrono::{DateTime, ParseError, SecondsFormat, SubsecRound, Utc};
use serde::Deserialize;
use std::borrow::Cow;
use thiserror::Error;

/// The fractional digits clause 4.6.3 admits at most, and the resolution of a
/// `TIMESTAMPTZ`.
const MICROSECOND_DIGITS: u16 = 6;

/// Render a UTC timestamp as RFC 3339, keeping exactly the sub-second precision
/// it carries: a whole second as `2024-06-15T12:30:00Z`, a millisecond instant
/// as `...:00.750Z`, a microsecond one as `...:00.123456Z`.
///
/// [`SecondsFormat::AutoSi`] prints the fraction in the smallest SI group -- 0,
/// 3 or 6 digits -- that represents the value without loss, and never a numeric
/// offset. This is what makes an `observedAt` or `expiresAt` round-trip to the
/// spelling the client sent for the whole-second and millisecond cases the
/// conformance suite exercises, while still carrying the microsecond resolution
/// clause 4.6.3 caps a `DateTime` at and PostgreSQL `TIMESTAMPTZ` stores.
///
/// An instant finer than a microsecond -- a Linux clock reads nanoseconds --
/// is rendered at the microsecond the store keeps it at, discarding the
/// remainder the way the `TIMESTAMPTZ` column does, so a response written
/// before the instant was stored and one read back after agree, and no
/// rendering ever carries the nine digits clause 4.6.3 forbids.
#[must_use]
pub fn sub_second(dt: DateTime<Utc>) -> String {
    dt.trunc_subsecs(MICROSECOND_DIGITS).to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// Why a client-supplied RFC 3339 instant is unusable.
#[derive(Debug, Error)]
pub enum InstantError {
    /// The fractional-seconds part breaks clause 4.6.3: more than one comma
    /// separator, or more than six fractional digits.
    #[error("instant {0}")]
    Fraction(#[from] FractionalSecondsError),
    /// The text is not an RFC 3339 date-time.
    #[error("not an RFC 3339 date-time: {0}")]
    Malformed(#[source] ParseError),
}

/// A client-supplied instant: the instant it names and the text it is kept as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInstant<'a> {
    /// The text with the clause 4.6.3 compatibility comma rewritten to the
    /// decimal point, which is the spelling the broker stores and renders.
    pub canonical: Cow<'a, str>,
    /// The instant the text names.
    pub instant: DateTime<Utc>,
}

/// Read an RFC 3339 instant a client supplied in a request.
///
/// Clause 4.6.3: "In requests, also a comma instead of a decimal point may be
/// used as separator for compatibility reasons." This is the one place the
/// comma is accepted for an instant that may carry a numeric offset
/// (`observedAt`, `expiresAt`, `timeAt`); the `DateTime` and `Time` values of
/// clause 4.6.3 share the same rewrite through
/// [`accept_comma_decimal_separator`]. The same clause caps the fraction at
/// "a maximum of six" digits, so a finer instant is refused rather than cut to
/// the microsecond: the broker never substitutes an instant for the one written.
///
/// # Errors
///
/// Returns [`InstantError::Fraction`] for more than one comma or more than six
/// fractional digits, and [`InstantError::Malformed`] when the rewritten text
/// is not RFC 3339.
pub fn parse_client_instant(text: &str) -> Result<ClientInstant<'_>, InstantError> {
    let canonical = accept_comma_decimal_separator(text)?;
    reject_excess_fractional_digits(&canonical)?;
    let instant = DateTime::parse_from_rfc3339(&canonical).map_err(InstantError::Malformed)?.with_timezone(&Utc);
    Ok(ClientInstant { canonical, instant })
}

/// An instant a request carries, read as [`parse_client_instant`] reads it.
///
/// A decoder that reads a member into a typed value names this type so the
/// clause 4.6.3 comma separator is accepted there as everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct RequestInstant(#[serde(with = "datetime")] DateTime<Utc>);

impl RequestInstant {
    /// The instant.
    #[must_use]
    pub const fn get(self) -> DateTime<Utc> {
        self.0
    }
}

/// Serialize and deserialize a required `DateTime<Utc>` as an RFC 3339 string.
pub mod datetime {
    use crate::rfc3339::{parse_client_instant, sub_second};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    /// Render the timestamp as an RFC 3339 string, keeping its sub-second precision.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&sub_second(*dt))
    }

    /// Parse an RFC 3339 string into a UTC timestamp.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when the value is not a string or does not
    /// parse as RFC 3339.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_client_instant(&s).map(|parsed| parsed.instant).map_err(Error::custom)
    }
}

/// Serialize and deserialize an optional `DateTime<Utc>` as an RFC 3339 string.
pub mod optional_datetime {
    use crate::rfc3339::{parse_client_instant, sub_second};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    /// Render the timestamp when present, and JSON `null` when absent.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(opt: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(dt) => serializer.serialize_some(&sub_second(*dt)),
            None => serializer.serialize_none(),
        }
    }

    /// Parse an optional RFC 3339 string into a UTC timestamp.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when a value is present but does not parse
    /// as RFC 3339.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => parse_client_instant(&s).map(|parsed| Some(parsed.instant)).map_err(Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fractional_seconds::FractionalSecondsError,
        rfc3339::{InstantError, parse_client_instant, sub_second},
    };
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestRequired {
        #[serde(with = "super::datetime")]
        ts: DateTime<Utc>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestOptional {
        #[serde(with = "super::optional_datetime", skip_serializing_if = "Option::is_none", default)]
        ts: Option<DateTime<Utc>>,
    }

    #[test]
    fn a_whole_second_renders_without_a_fraction() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        pretty_assertions::assert_eq!(sub_second(dt), "2024-06-15T12:30:00Z");
    }

    #[test]
    fn a_millisecond_instant_renders_three_fractional_digits() {
        let dt = Utc.with_ymd_and_hms(2026, 3, 1, 10, 0, 0).unwrap() + chrono::Duration::milliseconds(123);
        pretty_assertions::assert_eq!(sub_second(dt), "2026-03-01T10:00:00.123Z");
    }

    #[test]
    fn a_microsecond_instant_renders_at_full_six_digits() {
        let dt = Utc.with_ymd_and_hms(2026, 3, 1, 10, 0, 0).unwrap() + chrono::Duration::microseconds(123_456);
        pretty_assertions::assert_eq!(sub_second(dt), "2026-03-01T10:00:00.123456Z");
    }

    /// Clause 4.6.3 admits six fractional digits at most, and a nanosecond clock
    /// reading is stored at the microsecond, so both spell the same instant.
    #[test]
    fn a_nanosecond_instant_renders_at_the_microsecond_it_is_stored_at() {
        let dt = Utc.with_ymd_and_hms(2026, 10, 6, 13, 29, 28).unwrap() + chrono::Duration::nanoseconds(90_386);
        pretty_assertions::assert_eq!(sub_second(dt), "2026-10-06T13:29:28.000090Z");
    }

    #[test]
    fn a_nanosecond_instant_within_a_millisecond_renders_as_that_millisecond() {
        let dt = Utc.with_ymd_and_hms(2026, 10, 6, 13, 29, 28).unwrap() + chrono::Duration::nanoseconds(123_000_999);
        pretty_assertions::assert_eq!(sub_second(dt), "2026-10-06T13:29:28.123Z");
    }

    #[test]
    fn the_serde_adapters_render_exactly_what_the_function_does() {
        // Two readings of one format is how they drift; this pins them to one.
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap() + chrono::Duration::milliseconds(750);
        let expected = serde_json::Value::String(sub_second(dt));

        let required = serde_json::to_value(TestRequired { ts: dt }).unwrap();
        pretty_assertions::assert_eq!(required["ts"], expected);

        let optional = serde_json::to_value(TestOptional { ts: Some(dt) }).unwrap();
        pretty_assertions::assert_eq!(optional["ts"], expected);
    }

    #[test]
    fn rfc3339_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestRequired { ts: dt };
        let json = serde_json::to_string(&val).unwrap();
        assert!(json.contains("2024-06-15"));
        let parsed: TestRequired = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, dt);
    }

    #[test]
    fn rfc3339_option_some_round_trip() {
        let dt = Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap();
        let val = TestOptional { ts: Some(dt) };
        let json = serde_json::to_string(&val).unwrap();
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, Some(dt));
    }

    #[test]
    fn rfc3339_option_none_round_trip() {
        let val = TestOptional { ts: None };
        let json = serde_json::to_string(&val).unwrap();
        assert!(!json.contains("ts"));
        let parsed: TestOptional = serde_json::from_str(&json).unwrap();
        pretty_assertions::assert_eq!(parsed.ts, None);
    }

    /// Clause 4.6.3: a comma separator reads as the decimal point, with an
    /// offset too, and the canonical text carries the point.
    #[test]
    fn a_comma_fraction_reads_as_a_decimal_fraction() {
        let parsed = parse_client_instant("2024-01-01T01:00:00,25+01:00").expect("a comma fraction");
        pretty_assertions::assert_eq!(parsed.canonical, "2024-01-01T01:00:00.25+01:00");
        pretty_assertions::assert_eq!(
            parsed.instant,
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("instant") + chrono::Duration::milliseconds(250)
        );
    }

    /// Clause 4.6.3 admits "one or more fractional digits, up to a maximum of
    /// six".
    #[test]
    fn six_fractional_digits_are_the_accepted_maximum() {
        let parsed = parse_client_instant("2026-03-04T05:06:07.123456Z").expect("six fractional digits");
        pretty_assertions::assert_eq!(parsed.canonical, "2026-03-04T05:06:07.123456Z");
        pretty_assertions::assert_eq!(
            parsed.instant,
            Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).single().expect("instant") + chrono::Duration::microseconds(123_456)
        );
    }

    #[test]
    fn seven_fractional_digits_are_refused_rather_than_truncated() {
        let error = parse_client_instant("2026-03-04T05:06:07.1234567Z").expect_err("seven digits exceed clause 4.6.3");
        assert!(matches!(error, InstantError::Fraction(FractionalSecondsError::TooManyDigits)));
    }

    #[test]
    fn nine_fractional_digits_are_refused_rather_than_truncated() {
        let error = parse_client_instant("2026-03-04T05:06:07.123456789Z").expect_err("nine digits exceed clause 4.6.3");
        assert!(matches!(error, InstantError::Fraction(FractionalSecondsError::TooManyDigits)));
    }

    #[test]
    fn seven_fractional_digits_after_a_comma_and_before_an_offset_are_refused() {
        let error = parse_client_instant("2026-03-04T06:06:07,1234567+01:00").expect_err("seven digits exceed clause 4.6.3");
        assert!(matches!(error, InstantError::Fraction(FractionalSecondsError::TooManyDigits)));
    }

    #[test]
    fn the_serde_adapters_refuse_more_than_six_fractional_digits() {
        assert!(serde_json::from_str::<TestRequired>(r#"{"ts": "2026-03-04T05:06:07.123456789Z"}"#).is_err());
        assert!(serde_json::from_str::<TestOptional>(r#"{"ts": "2026-03-04T05:06:07.1234567Z"}"#).is_err());
    }

    #[test]
    fn two_commas_and_non_instants_are_refused() {
        assert!(parse_client_instant("2024-01-01T00:00:00,5,5Z").is_err());
        assert!(parse_client_instant("not an instant").is_err());
    }
}
