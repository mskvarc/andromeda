//! One lowered Query Term operand, and the target kinds it compares with.

use crate::filter::reading::target_kind::TargetKind;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_value_order::string_order::StringRelation;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

/// A lowered operand, in the value types the grammar's scalar carries.
///
/// Equality and order are over the value, so `1`, `1.0` and `1e0` are one
/// operand. A bare `URI` and a `quotedStr` holding the same text are one
/// operand too: both compare as the text they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    /// An exact number.
    Number(JsonNumber),
    /// A `quotedStr` or a bare `URI`.
    Text(String),
    /// `true` or `false`.
    Boolean(bool),
    /// A clause 4.6.3 `DateTime`, normalised to UTC.
    DateTime(DateTime<Utc>),
    /// A clause 4.6.3 `Date`.
    Date(NaiveDate),
    /// A clause 4.6.3 `Time`, normalised to UTC.
    Time(NaiveTime),
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

impl Operand {
    /// The number the operand is, `None` for any other operand.
    #[must_use]
    pub const fn number(&self) -> Option<&JsonNumber> {
        match self {
            Operand::Number(number) => Some(number),
            Operand::Text(_) | Operand::Boolean(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => None,
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
            TargetKind::String => matches!(self, Operand::Text(_)),
            TargetKind::RelationshipObject => matches!((self, comparison), (Operand::Text(_), Comparison::Equality)),
            TargetKind::Boolean => matches!(self, Operand::Boolean(_)),
            TargetKind::DateTime => matches!(self, Operand::DateTime(_)),
            TargetKind::Date => matches!(self, Operand::Date(_)),
            TargetKind::Time => matches!(self, Operand::Time(_)),
            TargetKind::Null | TargetKind::Object => false,
        }
    }
}
