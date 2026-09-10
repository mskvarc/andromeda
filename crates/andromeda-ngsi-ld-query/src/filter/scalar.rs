//! Comparison operands of a `q` predicate (clause 4.9).

use crate::{error::QueryError, filter::quoting::Quoting};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_date_time::NgsiLdDateTime, ngsi_ld_time::NgsiLdTime, time_value::TimeValueForm};
use andromeda_ngsi_ld_number::JsonNumber;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

/// The right-hand side of a comparison predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareValue {
    /// A single scalar value (e.g. `speed>50`).
    Scalar(Scalar),
    /// A range of two values (e.g. `speed==10..20`).
    Range {
        /// The lower bound of the range (inclusive).
        min: Scalar,
        /// The upper bound of the range (inclusive).
        max: Scalar,
    },
    /// A list of values (e.g. `status==\"active\",\"pending\"`).
    ValueList(Vec<Scalar>),
}

/// A scalar value in a `q` expression.
///
/// The three temporal variants carry parsed values rather than the client's
/// text. Clause 4.9's grammar defines `dateTime`, `date` and `time` as values
/// "as mandated by clause 4.6.3", so an operand is admitted here on exactly the
/// terms the write path admits one: the literal `Z` designator and no other
/// zone expression, at most six fractional-second digits, and a comma accepted
/// in requests as a decimal separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// An exact numeric value.
    Number(JsonNumber),
    /// A boolean value (`true` or `false`).
    Bool(bool),
    /// A quoted string value.
    Str(String),
    /// An RFC 3339 `date-time`, normalised to UTC.
    DateTime(DateTime<Utc>),
    /// An RFC 3339 `full-date`.
    Date(NaiveDate),
    /// An RFC 3339 `full-time`, normalised to UTC.
    Time(NaiveTime),
}

impl Scalar {
    /// Classify a literal operand by the clause 4.6.3 form it is written in and
    /// by whether the client quoted it.
    ///
    /// Clause 4.9's grammar gives an operand no type tag — `ComparableValue =
    /// Number / quotedStr / dateTime / date / time` — so the literal's own
    /// lexical form is what says which domain it belongs to, and the same
    /// classifier reads it that
    /// [`TimeValueForm`](andromeda_ngsi_ld_lexicon::time_value::TimeValueForm)
    /// classifies a stored value with. Clause 4.9 makes every operator
    /// non-matching when "there is no equality between the target value data
    /// type and the Query Term value data type", which two classifiers could
    /// only ever satisfy by never matching.
    ///
    /// The quotes decide what a literal that is *written in* one of those forms
    /// and does not satisfy it means, because the grammar puts them inside the
    /// production: `quotedStr` is the IETF RFC 8259 `String` production together
    /// with its quotes, while `dateTime`, `date` and `time` are unquoted
    /// alternatives, as clause 4.9's own EXAMPLE 8 writes them. So a quoted
    /// literal is derivable whatever it contains, and refusing one would refuse
    /// a well-formed Query Term — while a bare one that satisfies no unquoted
    /// production is derivable from nothing at all and is a malformed request.
    ///
    /// A quoted literal that *does* satisfy a clause 4.6.3 form is classified
    /// temporally, which is what keeps a stored timestamp findable in either
    /// spelling and matches clause 4.9's EXAMPLE 3,
    /// `?q=isParked=="urn:ngsi-ld:OffStreetParking:Downtown1"`, where a quoted
    /// URI has to match a Relationship object rather than being held apart from
    /// one because of its quotes.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::QInvalidDateTime`], [`QueryError::QInvalidDate`] or
    /// [`QueryError::QInvalidTime`] when a bare literal is written in that
    /// clause 4.6.3 form but is not a well-formed value of it.
    pub fn from_literal(raw: &str, quoting: Quoting) -> Result<Scalar, QueryError> {
        let Some(form) = TimeValueForm::of(raw) else {
            return Ok(Scalar::Str(raw.to_string()));
        };
        match (form, quoting) {
            (TimeValueForm::DateTime, Quoting::Bare) => Ok(Scalar::DateTime(parse_date_time(raw)?)),
            (TimeValueForm::Date, Quoting::Bare) => Ok(Scalar::Date(parse_date(raw)?)),
            (TimeValueForm::Time, Quoting::Bare) => Ok(Scalar::Time(parse_time(raw)?)),
            (TimeValueForm::DateTime, Quoting::Quoted) => Ok(parse_date_time(raw).map_or_else(|_| Scalar::Str(raw.to_string()), Scalar::DateTime)),
            (TimeValueForm::Date, Quoting::Quoted) => Ok(parse_date(raw).map_or_else(|_| Scalar::Str(raw.to_string()), Scalar::Date)),
            (TimeValueForm::Time, Quoting::Quoted) => Ok(parse_time(raw).map_or_else(|_| Scalar::Str(raw.to_string()), Scalar::Time)),
        }
    }

    /// The operand's canonical text form.
    ///
    /// One rendering serves both the clause 4.9 serialisation and every backend
    /// binding, so a filter that round-trips through the query string and a
    /// filter that reaches a store carry the same literal by construction.
    ///
    /// A temporal operand renders in the canonical clause 4.6.3 form of its own
    /// lexical type, fraction included. The fraction is not decoration: the
    /// binding a `valueList`, a `languageMap` and a compound value are probed
    /// through compares this text against the literal the client stored, and
    /// clause 4.9 names the `valueList` array as a target value, so a rendering
    /// at whole seconds turned `readings=="2024-01-01T00:00:00.250Z"` into a
    /// filter that could never match the value it names.
    #[must_use]
    pub fn literal_text(&self) -> String {
        match self {
            Scalar::Number(number) => number.to_string(),
            Scalar::Bool(value) => value.to_string(),
            // clone: callers own the rendered literal
            Scalar::Str(value) => value.clone(),
            Scalar::DateTime(value) => value.format(DATE_TIME_FORMAT).to_string(),
            Scalar::Date(value) => value.to_string(),
            Scalar::Time(value) => value.format(TIME_FORMAT).to_string(),
        }
    }
}

/// Parse a clause 4.6.3 `DateTime` operand.
///
/// Returns the instant rather than a [`Scalar`] wrapping it: a caller that
/// needs the value back out of the variant it was just placed in has to prove
/// the variant, and there is nothing to prove here.
///
/// # Errors
///
/// Returns [`QueryError::QInvalidDateTime`] when `raw` is not a clause 4.6.3
/// `DateTime` -- which includes a numeric zone offset in place of the mandatory
/// `Z`, and a fraction longer than six digits.
pub fn parse_date_time(raw: &str) -> Result<DateTime<Utc>, QueryError> {
    raw.parse::<NgsiLdDateTime>()
        .map(NgsiLdDateTime::get)
        .map_err(|source| QueryError::QInvalidDateTime {
            value: raw.to_string(),
            source,
        })
}

/// Parse a clause 4.6.3 `Date` operand.
///
/// # Errors
///
/// Returns [`QueryError::QInvalidDate`] when `raw` is not a clause 4.6.3
/// `Date`, whose components "shall appear in the string; reduced
/// representations are not permitted".
pub fn parse_date(raw: &str) -> Result<NaiveDate, QueryError> {
    raw.parse::<NgsiLdDate>().map(NgsiLdDate::get).map_err(|source| QueryError::QInvalidDate {
        value: raw.to_string(),
        source,
    })
}

/// Parse a clause 4.6.3 `Time` operand.
///
/// # Errors
///
/// Returns [`QueryError::QInvalidTime`] when `raw` is not a clause 4.6.3
/// `Time` -- which includes one written without the mandatory `Z` designator,
/// and one carrying a numeric offset, since the clause states that the string
/// "shall not contain expressions of the difference between local time and UTC".
pub fn parse_time(raw: &str) -> Result<NaiveTime, QueryError> {
    raw.parse::<NgsiLdTime>().map(NgsiLdTime::get).map_err(|source| QueryError::QInvalidTime {
        value: raw.to_string(),
        source,
    })
}

/// Canonical rendering of a UTC instant: seconds always, fractional seconds
/// only when they are non-zero, and an explicit `Z`.
const DATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.fZ";

/// Canonical rendering of a UTC time of day: seconds always, fractional
/// seconds only when they are non-zero, and an explicit `Z`.
const TIME_FORMAT: &str = "%H:%M:%S%.fZ";

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            quoting::Quoting,
            scalar::{Scalar, parse_date, parse_date_time, parse_time},
        },
    };

    fn date_time(raw: &str) -> Scalar {
        Scalar::DateTime(parse_date_time(raw).expect("valid date-time"))
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a date-time operand is an RFC 3339 date-time, so it is
        // resolved to an instant here rather than re-read on every rendering.
        let parsed = parse_date_time("2017-12-24T12:00:00Z").expect("valid date-time");
        assert_eq!(parsed.to_rfc3339_opts(chrono::SecondsFormat::Secs, true), "2017-12-24T12:00:00Z");
    }

    #[test]
    fn an_unparseable_date_time_operand_is_rejected() {
        let error = parse_date_time("2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDateTime { .. }), "got {error:?}");
    }

    #[test]
    fn an_unparseable_date_operand_is_rejected() {
        let error = parse_date("2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDate { .. }), "got {error:?}");
    }

    #[test]
    fn an_unparseable_time_operand_is_rejected() {
        let error = parse_time("25:99:99Z").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidTime { .. }), "got {error:?}");
    }

    /// Clause 4.9's grammar defines `dateTime` and `time` as values "as
    /// mandated by clause 4.6.3", and clause 4.6.3 requires the literal `Z`
    /// designator for `DateTime` and states that a `Time` string "shall not
    /// contain expressions of the difference between local time and UTC". A
    /// filter operand carrying a numeric offset was previously resolved to UTC
    /// and compared, so a spelling the write path refuses could still be
    /// queried with.
    #[test]
    fn an_offset_bearing_operand_is_rejected_rather_than_normalised() {
        let date_time_error = parse_date_time("2017-12-24T13:00:00+01:00").expect_err("a numeric offset is forbidden");
        assert!(matches!(date_time_error, QueryError::QInvalidDateTime { .. }), "got {date_time_error:?}");

        let time_error = parse_time("13:00:00+01:00").expect_err("a numeric offset is forbidden");
        assert!(matches!(time_error, QueryError::QInvalidTime { .. }), "got {time_error:?}");
    }

    /// Clause 4.6.3, for both `DateTime` and `Time`: "In requests, also a comma
    /// instead of a decimal point may be used as separator for compatibility
    /// reasons." A `q` operand is request text, so the compatibility form has to
    /// reach the same operand the decimal point does.
    #[test]
    fn a_comma_decimal_separator_names_the_same_operand_as_a_decimal_point() {
        assert_eq!(date_time("2024-01-01T00:00:00,250Z"), date_time("2024-01-01T00:00:00.250Z"));
        assert_eq!(
            parse_time("12:00:00,250Z").expect("the comma compatibility form"),
            parse_time("12:00:00.250Z").expect("the canonical form")
        );
    }

    /// Clause 4.6.3 caps the fraction at "one or more fractional digits, up to
    /// a maximum of six".
    #[test]
    fn a_fraction_beyond_six_digits_is_rejected() {
        let date_time_error = parse_date_time("2024-01-01T00:00:00.1234567Z").expect_err("seven digits exceeds the maximum");
        assert!(matches!(date_time_error, QueryError::QInvalidDateTime { .. }), "got {date_time_error:?}");

        let time_error = parse_time("12:00:00.1234567Z").expect_err("seven digits exceeds the maximum");
        assert!(matches!(time_error, QueryError::QInvalidTime { .. }), "got {time_error:?}");
    }

    /// A literal that satisfies a clause 4.6.3 form is that value in either
    /// spelling. Quoting is the escaping device EXAMPLE 11's `"Overstay 60
    /// minutes"` needs, not a declaration that the value is a String — EXAMPLE 3
    /// writes a URI inside quotes and requires it to match a Relationship
    /// object — so a stored timestamp is findable by the timestamp that wrote
    /// it however the client spelled the operand.
    #[test]
    fn the_quoted_and_unquoted_spellings_of_a_conformant_operand_classify_alike() {
        for literal in ["2024-01-01T00:00:00Z", "2024-01-01", "12:00:00Z"] {
            let bare = Scalar::from_literal(literal, Quoting::Bare).expect("a conformant operand");
            let quoted = Scalar::from_literal(literal, Quoting::Quoted).expect("a conformant operand");
            assert!(
                matches!(bare, Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)),
                "{literal} classified as {bare:?}"
            );
            pretty_assertions::assert_eq!(bare, quoted, "{literal}");
        }
    }

    /// Clause 4.9's `quotedStr` is the IETF RFC 8259 `String` production
    /// *including its quotes*, so a quoted literal is derivable whatever it
    /// contains: a value written in a clause 4.6.3 form that it does not satisfy
    /// is the text it is, and comparing it as text is what lets a client find
    /// the value the write path stored for it.
    #[test]
    fn a_quoted_literal_that_satisfies_no_form_is_the_text_it_is() {
        for literal in [
            "2024-01-01T01:00:00+01:00",
            "2024-01-01T00:00:00.1234567Z",
            "12:34:5678",
            "1234-56-78",
            "0000-00-00",
        ] {
            let scalar = Scalar::from_literal(literal, Quoting::Quoted).expect("a quotedStr is always derivable");
            pretty_assertions::assert_eq!(scalar, Scalar::Str(literal.to_owned()), "{literal}");
        }
    }

    /// The same literals written bare derive from no production of clause 4.9's
    /// grammar: `quotedStr` carries the quotes they lack, and each is written in
    /// a `dateTime`, `date` or `time` form it does not satisfy. A request the
    /// grammar cannot derive is a malformed one.
    #[test]
    fn a_bare_literal_that_satisfies_no_form_is_a_malformed_operand() {
        for (literal, is_expected) in [
            ("2024-01-01T01:00:00+01:00", matches_date_time as fn(&QueryError) -> bool),
            ("2024-01-01T00:00:00.1234567Z", matches_date_time),
            ("12:34:5678", matches_time),
            ("1234-56-78", matches_date),
            ("0000-00-00", matches_date),
        ] {
            let error = Scalar::from_literal(literal, Quoting::Bare).expect_err("no production derives it");
            assert!(is_expected(&error), "{literal}: got {error:?}");
        }
    }

    /// Whether a refusal is the `dateTime` one.
    fn matches_date_time(error: &QueryError) -> bool {
        matches!(error, QueryError::QInvalidDateTime { .. })
    }

    /// Whether a refusal is the `date` one.
    fn matches_date(error: &QueryError) -> bool {
        matches!(error, QueryError::QInvalidDate { .. })
    }

    /// Whether a refusal is the `time` one.
    fn matches_time(error: &QueryError) -> bool {
        matches!(error, QueryError::QInvalidTime { .. })
    }

    /// Clause 4.9's `quotedStr` is a JSON string, so free-form text carrying
    /// colons is compared as the text it is in either spelling. Classifying an
    /// operand by three byte positions alone made `note=="12:00:00 in the
    /// morning"` a malformed `time` operand and answered a conformant filter
    /// with `BadRequestData`.
    #[test]
    fn free_form_text_is_a_string_operand_however_much_punctuation_it_carries() {
        for literal in ["12:00:00 in the morning", "ab:cd:ef", "urn:ngsi-ld:Vehicle:A4567", "1999-2000"] {
            for quoting in [Quoting::Bare, Quoting::Quoted] {
                let scalar = Scalar::from_literal(literal, quoting).expect("free-form text is never malformed");
                pretty_assertions::assert_eq!(scalar, Scalar::Str(literal.to_owned()), "{literal} {quoting:?}");
            }
        }
    }

    /// The canonical clause 4.6.3 rendering carries the fraction the operand
    /// names. The JSONB containment probe a `valueList` and a `languageMap` are
    /// answered through binds this text against the literal the client stored,
    /// so an operand rendered at whole seconds named a value no stored list
    /// holds.
    #[test]
    fn a_fractional_operand_renders_the_fraction_it_carries() {
        assert_eq!(date_time("2024-01-01T00:00:00.250Z").literal_text(), "2024-01-01T00:00:00.250Z");
        assert_eq!(date_time("2024-01-01T00:00:00Z").literal_text(), "2024-01-01T00:00:00Z");
        assert_eq!(date_time("2024-01-01T00:00:00.123456Z").literal_text(), "2024-01-01T00:00:00.123456Z");
        assert_eq!(
            Scalar::Time(parse_time("12:00:00.250Z").expect("a fractional time of day")).literal_text(),
            "12:00:00.250Z"
        );
        assert_eq!(
            Scalar::Time(parse_time("12:00:00Z").expect("a whole-second time of day")).literal_text(),
            "12:00:00Z"
        );
    }
}
