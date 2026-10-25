//! The three clause 4.6.3 time value data types, and the one rule that decides when a value is one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.3 enumerates `DateTime`, `Date` and
//! `Time` among the data types "for representing Values" and pins the string
//! form of each completely: all components present with "reduced
//! representations are not permitted", the literal `Z` designator and no other
//! zone expression, and a fraction of "one or more fractional digits, up to a
//! maximum of six".
//!
//! A value has one of those data types only when it is declared as one (annex
//! C.6): by its JSON-LD `@type`, which a user `@context` may also coerce, or
//! by the `valueType` of the Property it belongs to (clause 4.5.2). A string
//! declared as neither is a string, whatever it looks like, so it is held,
//! served and compared as the text it is. Clause 4.9 compares a target only
//! with an operand of its own data type ("if there is no equality between the
//! target value data type and the Query Term value data type ... not
//! matching"), and its `quotedStr` is a String.
//!
//! The lexical form decides in one place only: a bare `q` operand, which the
//! clause 4.9 `ComparableValue` grammar admits as a `dateTime`, a `date` or a
//! `time` by its shape ([`TimeValueForm::of`]).

use crate::{
    datatype::primitive_literal::PrimitiveLiteral,
    reference::NgsiLdReference,
    time::{
        date::{NgsiLdDate, NgsiLdDateError},
        date_time::{NgsiLdDateTime, NgsiLdDateTimeError},
        rendering::{date_text, instant_text, time_of_day_text},
        time_of_day::{NgsiLdTime, NgsiLdTimeError},
    },
};
use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;
use derive_more::Display;
use iri_rs::Iri;
use thiserror::Error;

/// The width of the clause 4.6.3 `DateTime` skeleton `YYYY-MM-DDThh:mm:ss`, before its mandatory `Z`.
const DATE_TIME_SKELETON_WIDTH: usize = 19;

/// The one width of the clause 4.6.3 `Date` form `YYYY-MM-DD`.
const DATE_WIDTH: usize = 10;

/// The width of the clause 4.6.3 `Time` skeleton `hh:mm:ss`, before its mandatory `Z`.
const TIME_SKELETON_WIDTH: usize = 8;

/// Whether a character may appear anywhere in a bare clause 4.6.3 time operand.
///
/// The alphabet is wider than the conformant forms: `+` and `-` admit the
/// numeric zone offset the clause forbids, and `,` the decimal separator it
/// allows "in requests". An operand spelled with either is a time value
/// written wrongly, which is refused as malformed rather than compared as
/// text. A space, or any letter other than the `T` and `Z` designators, means
/// the operand is not an attempt at a time value at all.
const fn is_time_value_char(character: char) -> bool {
    matches!(character, '0'..='9' | '-' | ':' | '.' | ',' | '+' | 'T' | 'Z')
}

/// One of the three clause 4.6.3 time value data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeValueForm {
    /// `DateTime`, `YYYY-MM-DDThh:mm:ssZ`.
    DateTime,
    /// `Date`, `YYYY-MM-DD`.
    Date,
    /// `Time`, `hh:mm:ssZ`.
    Time,
}

impl TimeValueForm {
    /// The form a bare clause 4.9 `q` operand is written in, or `None` when it
    /// is written in none.
    ///
    /// The three forms cannot collide: the `Date` and `DateTime` forms open with
    /// a four-digit year followed by `-`, and only the longer one carries the
    /// `T` that starts a time of day, while a `Time` opens with the two hour
    /// digits followed by `:`.
    #[must_use]
    pub fn of(operand: &str) -> Option<TimeValueForm> {
        if !operand.chars().all(is_time_value_char) {
            return None;
        }
        // Every admitted character is ASCII, so a byte index is a character index.
        let bytes = operand.as_bytes();
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

    /// The form a datatype IRI names, or `None` when it names none of the three.
    #[must_use]
    pub fn named_by(datatype: &str) -> Option<TimeValueForm> {
        [TimeValueForm::DateTime, TimeValueForm::Date, TimeValueForm::Time]
            .into_iter()
            .find(|form| form.iri().as_str() == datatype)
    }

    /// The datatype IRI of this form in the NGSI-LD core vocabulary.
    #[must_use]
    pub const fn iri(self) -> Iri<&'static str> {
        match self {
            TimeValueForm::DateTime => expanded_classes::DATE_TIME,
            TimeValueForm::Date => expanded_classes::DATE,
            TimeValueForm::Time => expanded_classes::TIME,
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

/// What one literal is declared as: its own datatype and the `valueType` of
/// the attribute it is a value of.
///
/// `valueType` declares the datatype of every primitive member of the value it
/// sits beside (clause 4.5.2): the single value, each value of a multi-valued
/// Property, each item of a `ListProperty`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimeValueDeclaration<'a> {
    /// The literal's JSON-LD `@type`, expanded.
    pub datatype: Option<&'a NgsiLdReference>,
    /// The `valueType` of the attribute, expanded.
    pub value_type: Option<&'a NgsiLdReference>,
}

impl<'a> TimeValueDeclaration<'a> {
    /// The declaration of a literal carrying `datatype` in an attribute whose
    /// `valueType` is `value_type`.
    #[must_use]
    pub const fn new(datatype: Option<&'a NgsiLdReference>, value_type: Option<&'a NgsiLdReference>) -> TimeValueDeclaration<'a> {
        TimeValueDeclaration { datatype, value_type }
    }

    /// The time value form the declaration names, `None` when it names none.
    ///
    /// # Errors
    ///
    /// Returns [`TimeValueError::ConflictingDeclarations`] when the datatype and
    /// the `valueType` are two different datatypes and one of them is a time
    /// value form: a literal has one datatype.
    pub fn form(self) -> Result<Option<TimeValueForm>, TimeValueError> {
        let named = |reference: Option<&NgsiLdReference>| reference.and_then(|reference| TimeValueForm::named_by(reference.as_str()));
        let (by_datatype, by_value_type) = (named(self.datatype), named(self.value_type));
        match (self.datatype, self.value_type) {
            (Some(datatype), Some(value_type)) if datatype != value_type && (by_datatype.is_some() || by_value_type.is_some()) => {
                Err(TimeValueError::ConflictingDeclarations)
            }
            (Some(_) | None, Some(_) | None) => Ok(by_datatype.or(by_value_type)),
        }
    }
}

/// A parsed clause 4.6.3 time value.
///
/// [`Display`] renders the one spelling the broker gives each value it serves:
/// an instant and a time of day in UTC with the sub-second precision they carry
/// in groups of three digits, a date in its only spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
pub enum TimeValue {
    /// A `DateTime`: an instant.
    #[display("{}", instant_text(_0.get()))]
    DateTime(NgsiLdDateTime),
    /// A `Date`: a calendar date.
    #[display("{}", date_text(_0.get()))]
    Date(NgsiLdDate),
    /// A `Time`: a time of day in UTC.
    #[display("{}", time_of_day_text(_0.get()))]
    Time(NgsiLdTime),
}

impl TimeValue {
    /// The time value `literal` is declared as, `None` when it is declared as
    /// none and is therefore the plain JSON value it is.
    ///
    /// A declared form is what the client said the value is, so a literal that
    /// does not satisfy it is an error, never a string.
    #[must_use]
    pub fn declared(declaration: TimeValueDeclaration<'_>, literal: PrimitiveLiteral<'_>) -> Option<Result<TimeValue, TimeValueError>> {
        let form = match declaration.form() {
            Ok(form) => form?,
            Err(conflict) => return Some(Err(conflict)),
        };
        Some(match literal {
            PrimitiveLiteral::String(text) => form.parse(text),
            PrimitiveLiteral::Number(_) | PrimitiveLiteral::Boolean(_) => Err(TimeValueError::NotAString),
        })
    }

    /// The form this value has.
    #[must_use]
    pub const fn form(&self) -> TimeValueForm {
        match self {
            TimeValue::DateTime(_) => TimeValueForm::DateTime,
            TimeValue::Date(_) => TimeValueForm::Date,
            TimeValue::Time(_) => TimeValueForm::Time,
        }
    }
}

/// Ways a declared literal fails to be a clause 4.6.3 time value.
#[derive(Debug, Error)]
pub enum TimeValueError {
    /// The literal is declared a `DateTime` and is not one.
    #[error(transparent)]
    DateTime(#[from] NgsiLdDateTimeError),
    /// The literal is declared a `Date` and is not one.
    #[error(transparent)]
    Date(#[from] NgsiLdDateError),
    /// The literal is declared a `Time` and is not one.
    #[error(transparent)]
    Time(#[from] NgsiLdTimeError),
    /// A time datatype is declared on a JSON number or boolean; clause 4.6.3
    /// writes each of the three as a string.
    #[error("an NGSI-LD time value is a string")]
    NotAString,
    /// The literal's datatype and the attribute's `valueType` name two
    /// different datatypes.
    #[error("the value's datatype and the attribute's valueType disagree")]
    ConflictingDeclarations,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        datatype::primitive_literal::PrimitiveLiteral,
        reference::NgsiLdReference,
        time::{
            lexical::LexicalError,
            time_of_day::NgsiLdTimeError,
            value::{TimeValue, TimeValueDeclaration, TimeValueError, TimeValueForm},
        },
    };
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_vocabulary::expanded::classes as expanded_classes;

    fn reference(form: TimeValueForm) -> NgsiLdReference {
        form.iri().as_str().parse().expect("a datatype IRI")
    }

    fn declared(datatype: Option<&NgsiLdReference>, value_type: Option<&NgsiLdReference>, text: &str) -> Option<Result<TimeValue, TimeValueError>> {
        TimeValue::declared(TimeValueDeclaration::new(datatype, value_type), PrimitiveLiteral::String(text))
    }

    #[test]
    fn a_literal_typed_date_time_is_a_date_time() {
        let date_time = reference(TimeValueForm::DateTime);
        let value = declared(Some(&date_time), None, "2024-01-01T00:00:00.5Z")
            .expect("declared")
            .expect("a DateTime");
        assert!(matches!(value, TimeValue::DateTime(_)), "got {value:?}");
        pretty_assertions::assert_eq!(value.to_string(), "2024-01-01T00:00:00.500Z");
    }

    /// Annex C.6: a value is a time value only when it is declared as one.
    #[test]
    fn an_undeclared_string_in_date_time_form_is_no_time_value() {
        assert!(declared(None, None, "2024-01-01T00:00:00Z").is_none());
        assert!(declared(None, None, "12:00:00Z").is_none());
        assert!(declared(None, None, "2024-01-01").is_none());
    }

    #[test]
    fn a_value_type_declares_the_time_value_of_a_plain_literal() {
        let time = reference(TimeValueForm::Time);
        let value = declared(None, Some(&time), "12:00:00Z").expect("declared").expect("a Time");
        assert!(matches!(value, TimeValue::Time(_)), "got {value:?}");
        pretty_assertions::assert_eq!(value.to_string(), "12:00:00Z");
    }

    #[test]
    fn a_datatype_and_a_value_type_naming_two_forms_conflict() {
        let (date, time) = (reference(TimeValueForm::Date), reference(TimeValueForm::Time));
        let refusal = declared(Some(&date), Some(&time), "2024-01-01").expect("declared");
        assert!(matches!(refusal, Err(TimeValueError::ConflictingDeclarations)), "got {refusal:?}");
    }

    #[test]
    fn a_non_time_datatype_beside_a_time_value_type_conflicts() {
        let string: NgsiLdReference = "http://www.w3.org/2001/XMLSchema#string".parse().expect("a datatype IRI");
        let date_time = reference(TimeValueForm::DateTime);
        let refusal = declared(Some(&string), Some(&date_time), "2024-01-01T00:00:00Z").expect("declared");
        assert!(matches!(refusal, Err(TimeValueError::ConflictingDeclarations)), "got {refusal:?}");
    }

    #[test]
    fn a_datatype_and_a_value_type_naming_one_form_agree() {
        let date_time = reference(TimeValueForm::DateTime);
        let value = declared(Some(&date_time), Some(&date_time), "2024-01-01T00:00:00Z").expect("declared");
        assert!(matches!(value, Ok(TimeValue::DateTime(_))), "got {value:?}");
    }

    /// A declared form is what the client said the value is, so a literal that
    /// does not satisfy it is refused rather than demoted to a string.
    #[test]
    fn a_declared_form_refuses_a_literal_that_does_not_satisfy_it() {
        let time = reference(TimeValueForm::Time);
        let refusal = declared(Some(&time), None, "yesterday").expect("declared");
        assert!(
            matches!(refusal, Err(TimeValueError::Time(NgsiLdTimeError::Lexical(LexicalError::Malformed)))),
            "got {refusal:?}"
        );
    }

    #[test]
    fn a_time_datatype_on_a_number_is_not_a_string() {
        let date_time = reference(TimeValueForm::DateTime);
        let number: JsonNumber = serde_json::from_str("123").expect("a number");
        let refusal = TimeValue::declared(TimeValueDeclaration::new(Some(&date_time), None), PrimitiveLiteral::Number(&number)).expect("declared");
        assert!(matches!(refusal, Err(TimeValueError::NotAString)), "got {refusal:?}");
        let refusal = TimeValue::declared(TimeValueDeclaration::new(Some(&date_time), None), PrimitiveLiteral::Boolean(true)).expect("declared");
        assert!(matches!(refusal, Err(TimeValueError::NotAString)), "got {refusal:?}");
    }

    #[test]
    fn each_time_value_class_iri_names_its_own_form() {
        for form in [TimeValueForm::DateTime, TimeValueForm::Date, TimeValueForm::Time] {
            pretty_assertions::assert_eq!(TimeValueForm::named_by(form.iri().as_str()), Some(form));
        }
        pretty_assertions::assert_eq!(TimeValueForm::named_by(expanded_classes::PROPERTY.as_str()), None);
    }

    /// The canonical spelling of each form: no fraction, three or six digits,
    /// and `Z`; a date in its only spelling.
    #[test]
    fn each_value_displays_in_its_one_served_spelling() {
        for (form, written, served) in [
            (TimeValueForm::DateTime, "2024-01-01T00:00:00,5Z", "2024-01-01T00:00:00.500Z"),
            (TimeValueForm::DateTime, "2024-01-01T00:00:00.000Z", "2024-01-01T00:00:00Z"),
            (TimeValueForm::DateTime, "2024-01-01T00:00:00.250000Z", "2024-01-01T00:00:00.250Z"),
            (TimeValueForm::Time, "12:00:00.000Z", "12:00:00Z"),
            (TimeValueForm::Time, "12:00:00.5Z", "12:00:00.500Z"),
            (TimeValueForm::Time, "12:00:00.123456Z", "12:00:00.123456Z"),
            (TimeValueForm::Date, "2024-01-01", "2024-01-01"),
        ] {
            pretty_assertions::assert_eq!(form.parse(written).expect("a time value").to_string(), served, "{written}");
        }
    }

    #[test]
    fn each_bare_operand_form_is_classified_by_its_shape() {
        for (operand, form) in [
            ("2024-01-01T00:00:00Z", Some(TimeValueForm::DateTime)),
            ("2024-01-01T01:00:00+01:00", Some(TimeValueForm::DateTime)),
            ("2024-01-01", Some(TimeValueForm::Date)),
            ("12:00:00.5Z", Some(TimeValueForm::Time)),
            ("13:00:00+01:00", Some(TimeValueForm::Time)),
            ("12:00:00 in the morning", None),
            ("urn:ngsi-ld:Vehicle:A4567", None),
            ("atrium", None),
            ("", None),
        ] {
            pretty_assertions::assert_eq!(TimeValueForm::of(operand), form, "{operand}");
        }
    }
}
