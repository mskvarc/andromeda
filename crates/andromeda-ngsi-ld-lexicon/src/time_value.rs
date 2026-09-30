//! The three clause 4.6.3 time value forms, and the single classification of a literal into one of them.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 enumerates the data types "for
//! representing Values" and names three of them by their lexical form alone: a
//! "`DateTime` string for encoding a timestamp", a "Date string for encoding a
//! calendar date" and a "Time string for encoding a local time expressed in
//! UTC". Each is a JSON string, and each is fully pinned by the clause — all
//! components present with "reduced representations are not permitted", the
//! literal `Z` designator and no other zone expression, and a fraction of "one
//! or more fractional digits, up to a maximum of six".
//!
//! Because the representation of those three data types *is* the string, which
//! data type a value has can only be read off the string. Clause 4.8 NOTE 3
//! confirms a Property may hold one — "User-defined Properties whose value is a
//! time value (Date, `DateTime` or Time) are defined as Property, not as
//! `TemporalProperty`" — and annex C.6 calls the `valueType` member and the
//! JSON-LD `@value`/`@type` pair mechanisms for saying so explicitly, not the
//! only way a value can be one.
//!
//! Clause 4.9 then depends on that reading twice over: its `ComparableValue`
//! production admits a bare `dateTime`, `date` and `time` operand, it requires
//! that "when comparing dates or times, the order relation considered shall be
//! a temporal one", and it makes every operator non-matching when "there is no
//! equality between the target value data type and the Query Term value data
//! type". A broker that classified a stored value by one rule and a `q` operand
//! by another would satisfy that last sentence by never matching anything, so
//! the classification lives here and every side of the comparison reads it from
//! this one module.

use crate::{
    ngsi_ld_date::{NgsiLdDate, NgsiLdDateError},
    ngsi_ld_date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
    ngsi_ld_time::{NgsiLdTime, NgsiLdTimeError},
};
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// The width of the clause 4.6.3 `DateTime` skeleton `YYYY-MM-DDThh:mm:ss`, before its mandatory `Z`.
const DATE_TIME_SKELETON_WIDTH: usize = 19;

/// The one width of the clause 4.6.3 `Date` form `YYYY-MM-DD`.
const DATE_WIDTH: usize = 10;

/// The width of the clause 4.6.3 `Time` skeleton `hh:mm:ss`, before its mandatory `Z`.
const TIME_SKELETON_WIDTH: usize = 8;

/// Whether a character may appear anywhere in a clause 4.6.3 time value.
///
/// The alphabet is deliberately wider than the conformant forms: `+` and `-`
/// admit the numeric zone offset the clause forbids, and `,` admits the decimal
/// separator it allows "in requests" only. A literal spelled with either is
/// recognised as a time value *written wrongly*, which is what lets a `q`
/// operand carrying one be refused as malformed rather than silently compared
/// as text. What the alphabet excludes is free-form text: a space, or any
/// letter other than the `T` and `Z` designators, means the literal is not an
/// attempt at a time value at all.
const fn is_time_value_char(character: char) -> bool {
    matches!(character, '0'..='9' | '-' | ':' | '.' | ',' | '+' | 'T' | 'Z')
}

/// The calendar-date components of the `Date` and `DateTime` forms, `YYYY-MM-DD`.
macro_rules! date_components {
    () => {
        "[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])"
    };
}

/// The time-of-day components of the `Time` and `DateTime` forms: `hh:mm:ss`,
/// the optional fraction in either separator, and the mandatory `Z`.
macro_rules! time_components {
    () => {
        "([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]([.,][0-9]{1,6})?Z"
    };
}

/// The whole-literal pattern of the `DateTime` form.
const DATE_TIME_PATTERN: &str = concat!("^", date_components!(), "T", time_components!(), "$");

/// The whole-literal pattern of the `Date` form.
const DATE_PATTERN: &str = concat!("^", date_components!(), "$");

/// The whole-literal pattern of the `Time` form.
const TIME_PATTERN: &str = concat!("^", time_components!(), "$");

/// Which of the three clause 4.6.3 time value forms a literal is written in.
///
/// This is the shape of the literal, decided before it is parsed. A literal that
/// has a form and does not satisfy it is a malformed value of that form, which
/// is a different thing from a string that was never one — and the two callers
/// of this module part company on exactly that distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeValueForm {
    /// The `DateTime` form, `YYYY-MM-DDThh:mm:ssZ`.
    DateTime,
    /// The `Date` form, `YYYY-MM-DD`.
    Date,
    /// The `Time` form, `hh:mm:ssZ`.
    Time,
}

impl TimeValueForm {
    /// The form a literal is written in, or `None` when it is written in none.
    ///
    /// The three forms cannot collide: the `Date` and `DateTime` forms open with
    /// a four-digit year followed by `-`, and only the longer one carries the
    /// `T` that starts a time of day, while a `Time` opens with the two hour
    /// digits followed by `:`.
    #[must_use]
    pub fn of(literal: &str) -> Option<TimeValueForm> {
        if !literal.chars().all(is_time_value_char) {
            return None;
        }
        // Every admitted character is ASCII, so a byte index is a character index.
        let bytes = literal.as_bytes();
        if bytes.len() >= DATE_TIME_SKELETON_WIDTH && bytes.get(4) == Some(&b'-') && bytes.get(10) == Some(&b'T') {
            Some(TimeValueForm::DateTime)
        } else if bytes.len() == DATE_WIDTH && bytes.get(4) == Some(&b'-') && bytes.get(7) == Some(&b'-') {
            Some(TimeValueForm::Date)
        } else if bytes.len() >= TIME_SKELETON_WIDTH && bytes.get(2) == Some(&b':') && bytes.get(5) == Some(&b':') {
            Some(TimeValueForm::Time)
        } else {
            None
        }
    }

    /// The form a JSON-LD `@type` IRI declares, or `None` when it declares none.
    ///
    /// Annex C.6 shows a reified Property naming its value's datatype through
    /// the `@value`/`@type` pair or the `valueType` member, both of which reach
    /// an expanded value node carrying one of these three IRIs.
    #[must_use]
    pub fn declared_by(type_iri: &str) -> Option<TimeValueForm> {
        if type_iri == expanded_classes::DATE_TIME.as_str() {
            Some(TimeValueForm::DateTime)
        } else if type_iri == expanded_classes::DATE.as_str() {
            Some(TimeValueForm::Date)
        } else if type_iri == expanded_classes::TIME.as_str() {
            Some(TimeValueForm::Time)
        } else {
            None
        }
    }

    /// The POSIX regular expression matching exactly the spellings clause 4.6.3
    /// admits for this form, anchored at both ends.
    ///
    /// The pattern is this module's classification written for a reader that
    /// cannot run [`TimeValueForm::of`] followed by a parse — a storage backend
    /// deciding a stored literal's data type inside a statement. Every component
    /// the clause pins is pinned here: all components present, because "reduced
    /// representations are not permitted"; the literal `Z` and no other zone
    /// expression; a fraction of "one or more fractional digits, up to a maximum
    /// of six", written with either separator, since "in requests, also a comma
    /// instead of a decimal point may be used".
    ///
    /// The ranges cover every rule a regular expression can carry, which leaves
    /// exactly one literal the pattern admits and the parsers refuse: a day the
    /// calendar does not have, such as `2024-02-30`. A reader that classifies by
    /// this pattern places such a value in its lexical form's bucket rather than
    /// under String.
    #[must_use]
    pub const fn lexical_pattern(self) -> &'static str {
        match self {
            TimeValueForm::DateTime => DATE_TIME_PATTERN,
            TimeValueForm::Date => DATE_PATTERN,
            TimeValueForm::Time => TIME_PATTERN,
        }
    }

    /// Parse a literal as this form.
    ///
    /// # Errors
    ///
    /// Returns the clause 4.6.3 refusal of the form's own lexical type when the
    /// literal does not satisfy it.
    pub fn parse(self, literal: &str) -> Result<TimeValue, TimeValueError> {
        match self {
            TimeValueForm::DateTime => Ok(TimeValue::DateTime(literal.parse::<NgsiLdDateTime>()?)),
            TimeValueForm::Date => Ok(TimeValue::Date(literal.parse::<NgsiLdDate>()?)),
            TimeValueForm::Time => Ok(TimeValue::Time(literal.parse::<NgsiLdTime>()?)),
        }
    }
}

/// A parsed clause 4.6.3 time value.
///
/// [`Display`] renders each variant in the canonical form its lexical type
/// defines, which is the single rendering the broker stores, orders and
/// compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum TimeValue {
    /// A `DateTime`: an instant.
    #[display("{_0}")]
    DateTime(NgsiLdDateTime),
    /// A `Date`: a calendar date.
    #[display("{_0}")]
    Date(NgsiLdDate),
    /// A `Time`: a time of day in UTC.
    #[display("{_0}")]
    Time(NgsiLdTime),
}

impl TimeValue {
    /// The time value one JSON string literal carries, if it carries one.
    ///
    /// `declared` is the JSON-LD `@type` an expanded value node states. The two
    /// inputs are read in the order clause 4.6.3 and annex C.6 put them in:
    ///
    /// * a declared form is what the client said the value is, so the literal is
    ///   held to it and one that does not satisfy it is an error — refusing is
    ///   the only way `2026-4-2` declared as a `Date` is not silently stored as
    ///   a spelling that sorts nowhere near its neighbours;
    /// * with no declared form, the clause 4.6.3 lexical forms decide, because
    ///   the representation of each of those three data types *is* the string.
    ///   A literal that resembles one without satisfying it is not an error
    ///   here: clause 4.6.3 admits "all the JSON native data types" too, so the
    ///   value is simply the JSON string it is written as, and `None` says so.
    #[must_use]
    pub fn admitted(declared: Option<&str>, literal: &str) -> Option<Result<TimeValue, TimeValueError>> {
        match declared.and_then(TimeValueForm::declared_by) {
            Some(form) => Some(form.parse(literal)),
            None => TimeValueForm::of(literal).and_then(|form| form.parse(literal).ok()).map(Ok),
        }
    }
}

// Parsing is `TimeValueForm::of` followed by that form's own parser, which is
// what a caller holding only text wants; the two steps stay separate because a
// caller that has a declared form skips the first.
impl FromStr for TimeValue {
    type Err = TimeValueError;

    fn from_str(literal: &str) -> Result<TimeValue, TimeValueError> {
        let Some(form) = TimeValueForm::of(literal) else {
            return Err(TimeValueError::NoTimeValueForm);
        };
        form.parse(literal)
    }
}

/// Ways a literal fails to be a clause 4.6.3 time value.
#[derive(Debug, Error)]
pub enum TimeValueError {
    /// The literal is written in the `DateTime` form and is not one.
    #[error(transparent)]
    DateTime(#[from] NgsiLdDateTimeError),
    /// The literal is written in the `Date` form and is not one.
    #[error(transparent)]
    Date(#[from] NgsiLdDateError),
    /// The literal is written in the `Time` form and is not one.
    #[error(transparent)]
    Time(#[from] NgsiLdTimeError),
    /// The literal is written in none of the three forms (ETSI GS CIM 009 clause 4.6.3).
    #[error("value is written in no NGSI-LD time value form")]
    NoTimeValueForm,
}

#[cfg(test)]
mod tests {
    use crate::time_value::{TimeValue, TimeValueError, TimeValueForm};
    use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;

    /// Clause 4.6.3 fixes one lexical form per data type, so each conformant
    /// spelling is classified as the type whose form it is written in.
    #[test]
    fn each_conformant_spelling_is_classified_as_its_own_form() {
        for (literal, form) in [
            ("2024-01-01T00:00:00Z", TimeValueForm::DateTime),
            ("2024-01-01T00:00:00.250000Z", TimeValueForm::DateTime),
            ("2024-01-01", TimeValueForm::Date),
            ("12:00:00Z", TimeValueForm::Time),
            ("12:00:00.5Z", TimeValueForm::Time),
        ] {
            pretty_assertions::assert_eq!(TimeValueForm::of(literal), Some(form), "{literal}");
        }
    }

    /// Free-form text is not an attempt at a time value, whatever punctuation it
    /// happens to carry. A `q` operand carrying such a literal is a `quotedStr`
    /// under clause 4.9's grammar and has to compare as the text it is.
    #[test]
    fn free_form_text_carrying_temporal_punctuation_is_written_in_no_form() {
        for literal in [
            "12:00:00 in the morning",
            "ab:cd:ef",
            "urn:ngsi-ld:Vehicle:A4567",
            "atrium",
            "2024-01-01T00:00:00Z and later",
            "",
        ] {
            pretty_assertions::assert_eq!(TimeValueForm::of(literal), None, "{literal}");
        }
    }

    /// A spelling clause 4.6.3 forbids is still written in the form it fails —
    /// a numeric offset in place of the mandatory `Z`, a fraction beyond six
    /// digits, a reduced representation — which is what lets it be reported as
    /// malformed rather than compared as text.
    #[test]
    fn a_forbidden_spelling_keeps_the_form_it_fails() {
        for (literal, form) in [
            ("2024-01-01T01:00:00+01:00", TimeValueForm::DateTime),
            ("2024-01-01T00:00:00.1234567Z", TimeValueForm::DateTime),
            ("2024-01-01T00:00:00", TimeValueForm::DateTime),
            ("13:00:00+01:00", TimeValueForm::Time),
            ("12:00:00", TimeValueForm::Time),
        ] {
            pretty_assertions::assert_eq!(TimeValueForm::of(literal), Some(form), "{literal}");
            assert!(literal.parse::<TimeValue>().is_err(), "{literal} must not parse");
        }
    }

    /// Annex C.6: the `@value`/`@type` pair is one mechanism for declaring the
    /// datatype of a reified Property's value.
    #[test]
    fn each_time_value_class_iri_declares_its_own_form() {
        pretty_assertions::assert_eq!(TimeValueForm::declared_by(expanded_classes::DATE_TIME.as_str()), Some(TimeValueForm::DateTime));
        pretty_assertions::assert_eq!(TimeValueForm::declared_by(expanded_classes::DATE.as_str()), Some(TimeValueForm::Date));
        pretty_assertions::assert_eq!(TimeValueForm::declared_by(expanded_classes::TIME.as_str()), Some(TimeValueForm::Time));
        pretty_assertions::assert_eq!(TimeValueForm::declared_by(expanded_classes::PROPERTY.as_str()), None);
    }

    /// A declared form is what the client said the value is, so a literal that
    /// does not satisfy it is refused rather than demoted to a string.
    #[test]
    fn a_declared_form_refuses_a_literal_that_does_not_satisfy_it() {
        let refusal = TimeValue::admitted(Some(expanded_classes::TIME.as_str()), "13:00:00+01:00").expect("a declared form always answers");
        assert!(matches!(refusal, Err(TimeValueError::Time(_))), "got {refusal:?}");
    }

    /// With nothing declared, a literal that merely resembles a time value is
    /// the JSON string it is written as: clause 4.6.3 admits "all the JSON
    /// native data types" alongside the three lexical forms.
    #[test]
    fn an_undeclared_literal_that_resembles_a_form_without_satisfying_it_is_a_plain_string() {
        assert!(TimeValue::admitted(None, "13:00:00+01:00").is_none());
        assert!(TimeValue::admitted(None, "2024-01-01T00:00:00.1234567Z").is_none());
        assert!(TimeValue::admitted(None, "atrium").is_none());
    }

    /// The undeclared reading is what makes a client's timestamp findable: the
    /// value it wrote is a `DateTime`, and it renders in the canonical form of
    /// its lexical type.
    #[test]
    fn an_undeclared_conformant_literal_is_the_time_value_it_is_written_as() {
        let admitted = TimeValue::admitted(None, "2024-01-01T00:00:00.250000Z")
            .expect("a conformant DateTime is a time value")
            .expect("and it parses");
        pretty_assertions::assert_eq!(admitted.to_string(), "2024-01-01T00:00:00.250000Z");

        let time = TimeValue::admitted(None, "12:00:00Z").expect("a conformant Time").expect("and it parses");
        pretty_assertions::assert_eq!(time.to_string(), "12:00:00.000000Z");

        let date = TimeValue::admitted(None, "2024-01-01").expect("a conformant Date").expect("and it parses");
        pretty_assertions::assert_eq!(date.to_string(), "2024-01-01");
    }

    /// Clause 4.6.3: "In requests, also a comma instead of a decimal point may
    /// be used as separator for compatibility reasons."
    #[test]
    fn the_comma_compatibility_separator_names_the_same_value_as_the_decimal_point() {
        pretty_assertions::assert_eq!(
            "12:00:00,5Z".parse::<TimeValue>().expect("the comma form").to_string(),
            "12:00:00.5Z".parse::<TimeValue>().expect("the decimal point form").to_string()
        );
        pretty_assertions::assert_eq!(
            "2024-01-01T00:00:00,25Z".parse::<TimeValue>().expect("the comma form").to_string(),
            "2024-01-01T00:00:00.25Z".parse::<TimeValue>().expect("the decimal point form").to_string()
        );
    }

    /// The pattern is the classification written for a reader that cannot run
    /// the parser, so it has to accept exactly the literals the parser accepts.
    /// The one documented exception is a day the calendar does not have, which
    /// no regular expression can exclude.
    #[test]
    fn each_lexical_pattern_accepts_exactly_the_literals_its_form_parses() {
        let literals = [
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00.250000Z",
            "2024-01-01T00:00:00,25Z",
            "2024-01-01T00:00:00.1234567Z",
            "2024-01-01T01:00:00+01:00",
            "2024-01-01T00:00:00",
            "2024-01-01T24:00:00Z",
            "2024-13-01T00:00:00Z",
            "2024-01-01",
            "2024-1-1",
            "2024-01-32",
            "12:00:00Z",
            "12:00:00.5Z",
            "12:00:00,5Z",
            "12:00:00",
            "13:00:00+01:00",
            "24:00:00Z",
            "12:60:00Z",
            "atrium",
            "12:00:00 in the morning",
            "urn:ngsi-ld:Vehicle:A4567",
            "",
        ];
        for form in [TimeValueForm::DateTime, TimeValueForm::Date, TimeValueForm::Time] {
            let pattern = regex::Regex::new(form.lexical_pattern()).expect("a valid regular expression");
            for literal in literals {
                let parses = form.parse(literal).is_ok();
                pretty_assertions::assert_eq!(pattern.is_match(literal), parses, "{form:?} pattern disagrees on {literal}");
            }
        }
    }

    /// The one literal a regular expression cannot separate: February has no
    /// thirtieth day, so the pattern accepts what the parser refuses.
    #[test]
    fn a_calendar_day_that_does_not_exist_is_the_patterns_single_documented_exception() {
        let pattern = regex::Regex::new(TimeValueForm::Date.lexical_pattern()).expect("a valid regular expression");
        assert!(pattern.is_match("2024-02-30"), "the pattern cannot count the days of a month");
        assert!(TimeValueForm::Date.parse("2024-02-30").is_err(), "the parser can");
    }

    #[test]
    fn a_literal_in_no_form_reports_that_rather_than_a_lexical_refusal() {
        let error = "atrium".parse::<TimeValue>().expect_err("not a time value");
        assert!(matches!(error, TimeValueError::NoTimeValueForm), "got {error:?}");
    }
}
