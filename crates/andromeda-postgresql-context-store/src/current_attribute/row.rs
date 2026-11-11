//! One `current_attributes` row: a top-level attribute instance with its
//! sub-attribute tree.

use andromeda_postgresql_attribute_instance::{stored_members::InstanceMembers, stored_value::StoredValue, sub_attribute::SubAttributeElement};
use andromeda_postgresql_schema::{current::CurrentAttributes, string_value_column::StringValueColumn, uri_value_column::UriValueColumn};
use chrono::{DateTime, Utc};

/// One row of `current_attributes`, for one Entity.
#[derive(Debug, PartialEq)]
pub struct CurrentAttributeRow {
    /// The written position of the instance among the Entity's attribute
    /// instances, from 0: attributes in order, each attribute's datasets in
    /// order.
    pub ordinal: i32,
    /// The instance's members.
    pub members: InstanceMembers,
    /// `createdAt` (clause 4.8).
    pub created_at: DateTime<Utc>,
    /// `modifiedAt` (clause 4.8).
    pub modified_at: DateTime<Utc>,
    /// The instance's sub-attributes in pre-order: every element before its
    /// children, siblings and dataset instances in written order.
    pub sub_attributes: Vec<SubAttributeElement>,
}

/// The `current_attributes` column a row holds `value` in: a String or a URI
/// past its B-tree bound moves to its overflow column. `None` for a
/// data-integrity seal, which is stored only as a sub-attribute (clause
/// 4.5.2.2), so no row holds one.
#[must_use]
pub fn row_column(value: &StoredValue) -> Option<CurrentAttributes> {
    Some(match value {
        StoredValue::Number { .. } | StoredValue::DeclaredNumber { .. } => CurrentAttributes::ValueNumber,
        StoredValue::String { text, .. } => StringValueColumn::of(text).column(),
        StoredValue::Boolean { .. } | StoredValue::DeclaredBoolean { .. } => CurrentAttributes::ValueBoolean,
        StoredValue::DateTime { .. } => CurrentAttributes::ValueDateTime,
        StoredValue::Date { .. } | StoredValue::Time { .. } => CurrentAttributes::ValueString,
        StoredValue::Geometry { .. } => CurrentAttributes::ValueGeometry,
        StoredValue::Object(_) => CurrentAttributes::ValueObject,
        StoredValue::Uri(uri) => UriValueColumn::of(uri.as_str()).column(),
        StoredValue::Proof(_) => return None,
    })
}
