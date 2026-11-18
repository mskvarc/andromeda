//! One lowered Query Term operand, and the target kinds it compares with.

use crate::filter::{
    reading::{target_kind::TargetKind, uri_operand::UriOperand},
    scalar::Scalar,
};
use andromeda_ngsi_ld_lexicon::time::{
    calendar_serde::{date, time_of_day},
    instant_serde::required,
    value::TimeValueForm,
};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_value_order::string_order::StringRelation;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

/// A lowered operand, in the value types the grammar's scalar carries.
///
/// Equality and order are over the value, so `1`, `1.0` and `1e0` are one
/// operand. A bare `URI` and a `quotedStr` holding the same text are one
/// operand too: both compare as the text they are, but for a URI an equality
/// compares in two spellings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operand {
    /// An exact number.
    Number(JsonNumber),
    /// A `quotedStr` or a bare `URI`.
    Text(String),
    /// A `quotedStr` or a bare `URI` after `==` or `!=` that RFC 3986 section
    /// 6 normalises to another spelling.
    Uri(UriOperand),
    /// `true` or `false`.
    Boolean(bool),
    /// A clause 4.6.3 `DateTime`, normalised to UTC.
    DateTime(#[serde(serialize_with = "required::serialize")] DateTime<Utc>),
    /// A clause 4.6.3 `Date`.
    Date(#[serde(serialize_with = "date::serialize")] NaiveDate),
    /// A clause 4.6.3 `Time`, normalised to UTC.
    Time(#[serde(serialize_with = "time_of_day::serialize")] NaiveTime),
}

/// Which operators an operand stands after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `==` and `!=`, with a value or a value list.
    Equality,
    /// The four ordering operators, and the bounds of a range.
    Order,
}

impl Comparison {
    /// The relation text is ordered by after operators of this comparison.
    ///
    /// Clause 4.9 compares text strings following RFC 8259 section 8.3, which
    /// makes equality code unit identity, and asks for the Unicode Collation
    /// Algorithm where strings are ordered, so an equality compares code
    /// points and an ordering compares under the collation.
    #[must_use]
    pub const fn string_relation(self) -> StringRelation {
        match self {
            Comparison::Equality => StringRelation::CodePoint,
            Comparison::Order => StringRelation::Collated,
        }
    }
}

/// The operand a parsed scalar is: a bare `URI` compares as the text it is,
/// exactly as a `quotedStr` holding it. Written by hand because the two text
/// productions become one operand, which no derive spells.
impl From<&Scalar> for Operand {
    fn from(scalar: &Scalar) -> Operand {
        match scalar {
            Scalar::Number(number) => Operand::Number(number.clone()),
            Scalar::Bool(value) => Operand::Boolean(*value),
            Scalar::Str(text) | Scalar::Uri(text) => Operand::Text(text.clone()),
            Scalar::DateTime(instant) => Operand::DateTime(*instant),
            Scalar::Date(date) => Operand::Date(*date),
            Scalar::Time(time) => Operand::Time(*time),
        }
    }
}

impl Operand {
    /// The text a String target is compared with, `None` for an operand that
    /// is no text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text_against(TargetKind::String)
    }

    /// The text a target of `kind` is compared with, `None` for an operand
    /// that is no text.
    #[must_use]
    pub fn text_against(&self, kind: TargetKind) -> Option<&str> {
        match self {
            Operand::Text(text) => Some(text.as_str()),
            Operand::Uri(uri) => Some(uri.against(kind)),
            Operand::Number(_) | Operand::Boolean(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => None,
        }
    }

    /// The number the operand is, `None` for any other operand.
    #[must_use]
    pub const fn number(&self) -> Option<&JsonNumber> {
        match self {
            Operand::Number(number) => Some(number),
            Operand::Text(_) | Operand::Uri(_) | Operand::Boolean(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => None,
        }
    }

    /// The clause 4.6.3 time value form the operand is written in, `None` for
    /// an operand that is no time value.
    #[must_use]
    pub const fn time_form(&self) -> Option<TimeValueForm> {
        match self {
            Operand::DateTime(_) => Some(TimeValueForm::DateTime),
            Operand::Date(_) => Some(TimeValueForm::Date),
            Operand::Time(_) => Some(TimeValueForm::Time),
            Operand::Number(_) | Operand::Text(_) | Operand::Uri(_) | Operand::Boolean(_) => None,
        }
    }

    /// Whether the operand compares with a target of `kind` after operators of
    /// `comparison`, the "same kind" of every clause 4.9 operator.
    ///
    /// Clause 4.9: "If there is no equality between the target value data type
    /// and the Query Term value data type, then it shall be considered as not
    /// matching." Text compares with a Relationship's object under equality
    /// only, since clause 4.9 bars a Relationship from "any operator different
    /// than equal or unequal", and a range is a value of those two operators
    /// whose bounds are ordered. A number compares with the numbers beyond the
    /// decimals too, which the number order places.
    #[must_use]
    pub const fn compares_with(&self, kind: TargetKind, comparison: Comparison) -> bool {
        match kind {
            TargetKind::Number | TargetKind::NotANumber | TargetKind::PositiveInfinity | TargetKind::NegativeInfinity => {
                matches!(self, Operand::Number(_))
            }
            TargetKind::String => matches!(self, Operand::Text(_) | Operand::Uri(_)),
            TargetKind::RelationshipObject => matches!((self, comparison), (Operand::Text(_) | Operand::Uri(_), Comparison::Equality)),
            TargetKind::Boolean => matches!(self, Operand::Boolean(_)),
            TargetKind::DateTime => matches!(self, Operand::DateTime(_)),
            TargetKind::Date => matches!(self, Operand::Date(_)),
            TargetKind::Time => matches!(self, Operand::Time(_)),
            TargetKind::Null | TargetKind::Object => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::reading::operand::Operand;
    use andromeda_ngsi_ld_lexicon::time::{date::NgsiLdDate, date_time::NgsiLdDateTime, time_of_day::NgsiLdTime, value::TimeValueForm};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_time_operand_names_its_form_and_no_other_operand_names_one() {
        assert_eq!(
            Operand::DateTime("2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a date-time").get()).time_form(),
            Some(TimeValueForm::DateTime)
        );
        assert_eq!(
            Operand::Date("2024-01-01".parse::<NgsiLdDate>().expect("a date").get()).time_form(),
            Some(TimeValueForm::Date)
        );
        assert_eq!(
            Operand::Time("08:00:00Z".parse::<NgsiLdTime>().expect("a time").get()).time_form(),
            Some(TimeValueForm::Time)
        );
        for operand in [Operand::Number(1.into()), Operand::Text("2024-01-01".to_owned()), Operand::Boolean(true)] {
            assert_eq!(operand.time_form(), None, "{operand:?}");
        }
    }
}
