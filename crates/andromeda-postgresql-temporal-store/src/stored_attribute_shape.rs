//! The family and the value shape one `attribute_history` row was written
//! with, as the pair clause 4.5.19.1's tables are indexed by.
//!
//! Two stored codes rather than one: Tables 4.5.19.1-1 and 4.5.19.1-2 give one
//! column per value datatype and govern the Property families, while Table
//! 4.5.19.1-3 has a single column keyed on the family alone and governs
//! Relationships (and, through clause 4.5.22.1's "specialized type of
//! Relationship", `ListRelationship` with them). Reading a shape without the
//! family it was written under answers out of the wrong table.

use andromeda_postgresql_codes::{attribute_kind::AttributeKind, recorded_shape::RecordedShape};

/// One combination of stored attribute family and refined value shape.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct StoredAttributeShape {
    /// The family code the row recorded (clause 5.2.4).
    pub(crate) kind: AttributeKind,
    /// The refined value shape the row's value column carries.
    pub(crate) shape: RecordedShape,
}
