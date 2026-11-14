//! One `current_attributes` row: a top-level attribute instance with its
//! sub-attribute tree.

use andromeda_postgresql_attribute_instance::{stored_members::InstanceMembers, sub_attribute::SubAttributeElement};
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
