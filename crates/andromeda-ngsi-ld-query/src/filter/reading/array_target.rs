//! The arrays a clause 4.9 target can be.

use strum::EnumIter;

/// A target holding several values, by what clause 4.9 makes of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum ArrayTarget {
    /// Several values that are together the target value: an array Property,
    /// a `valueList`. "Is included in the target value, if the latter is an
    /// array."
    ValueArray,
    /// Several objects that are together the target object: a Relationship
    /// naming several, a `ListRelationship`.
    ObjectArray,
    /// A languageMap, or one language of it, any one of whose strings is the
    /// target value: "a value from any of the key-value pairs held within the
    /// languageMap".
    AnyOneValue,
}
