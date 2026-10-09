//! One `current_attributes` row: a top-level attribute instance with its
//! sub-attribute tree.

use crate::current_attribute::{stored_json::StoredJson, stored_value::StoredValue, sub_attribute::SubAttributeElement};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};

/// The members of one attribute instance a row and an element both hold.
#[derive(Debug, PartialEq)]
pub struct InstanceMembers {
    /// `attribute_names` key of the attribute.
    pub attribute_name_key: AttributeNameKey,
    /// The `datasetId`, absent on the default instance (clause 4.5.5).
    pub dataset_id: Option<Identifier>,
    /// The attribute family.
    pub attribute_kind: AttributeKind,
    /// The value.
    pub value: StoredValue,
    /// `entity_type_names` key of the `valueType`.
    pub value_type_key: Option<EntityTypeNameKey>,
    /// `entity_type_names` keys of the `objectType` list, in written order.
    pub object_type_keys: Vec<EntityTypeNameKey>,
    /// Catalog identity of the `unitCode`.
    pub unit_code_id: Option<UnitCodeId>,
    /// `observedAt`.
    pub observed_at: Option<DateTime<Utc>>,
    /// `expiresAt` (clause 4.22).
    pub expires_at: Option<DateTime<Utc>>,
    /// The clause 4.5.2.2 seals, in their stored JSON.
    pub proof_seals: Option<StoredJson>,
}

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
