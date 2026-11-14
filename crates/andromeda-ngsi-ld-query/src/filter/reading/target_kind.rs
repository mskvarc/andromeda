//! The kinds of value a clause 4.9 target can be.

use andromeda_ngsi_ld_number::declared_number::NonFinite;
use strum::EnumIter;

/// The kind of one target value, the data type clause 4.9 compares a Query
/// Term operand by.
///
/// A `VocabProperty` term is a String once expanded (clause 4.9, "the target
/// value shall be expanded according to the @context"), so no kind of its own
/// is needed. A Relationship's object is kept apart from a String because
/// clause 4.9 bars it from "any operator different than equal or unequal".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum TargetKind {
    /// An exact decimal (clause 4.6.3).
    Number,
    /// Not-a-number, a value of the `xsd:float` and `xsd:double` value spaces
    /// beyond the decimals.
    NotANumber,
    /// Positive infinity of the `xsd:float` and `xsd:double` value spaces.
    PositiveInfinity,
    /// Negative infinity of the `xsd:float` and `xsd:double` value spaces.
    NegativeInfinity,
    /// A String, the lexical form of an IRI a Property value names among them.
    String,
    /// The object of a Relationship or a `ListRelationship`, a URI.
    RelationshipObject,
    /// A Boolean.
    Boolean,
    /// A value declared as a clause 4.6.3 `DateTime`, or an instant an
    /// instance member holds.
    DateTime,
    /// A value declared as a clause 4.6.3 `Date`.
    Date,
    /// A value declared as a clause 4.6.3 `Time`.
    Time,
    /// The JSON `null` a raw JSON value holds.
    Null,
    /// A JSON object a raw JSON value holds.
    Object,
}

impl TargetKind {
    /// The number beyond the decimals a target of this kind is, `None` for
    /// every other kind.
    #[must_use]
    pub const fn non_finite(self) -> Option<NonFinite> {
        match self {
            TargetKind::NotANumber => Some(NonFinite::NotANumber),
            TargetKind::PositiveInfinity => Some(NonFinite::PositiveInfinity),
            TargetKind::NegativeInfinity => Some(NonFinite::NegativeInfinity),
            TargetKind::Number
            | TargetKind::String
            | TargetKind::RelationshipObject
            | TargetKind::Boolean
            | TargetKind::DateTime
            | TargetKind::Date
            | TargetKind::Time
            | TargetKind::Null
            | TargetKind::Object => None,
        }
    }
}
