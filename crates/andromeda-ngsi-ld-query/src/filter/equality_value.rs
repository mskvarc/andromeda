//! The value of a clause 4.9 equality Query Term (`CompEqualityValue`): one scalar, a range or a value list.

use crate::filter::scalar::Scalar;

/// What an equality Query Term compares an attribute with: `==` and `!=` take a scalar, a range or a value list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EqualityValue {
    /// A single scalar value (e.g. `speed==50`).
    Scalar(Scalar),
    /// A range of two values (e.g. `speed==10..20`).
    Range {
        /// The lower bound of the range (inclusive).
        min: Scalar,
        /// The upper bound of the range (inclusive).
        max: Scalar,
    },
    /// A list of values (e.g. `status=="active","pending"`).
    ValueList(Vec<Scalar>),
}
