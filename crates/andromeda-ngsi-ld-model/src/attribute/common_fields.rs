//! The members every NGSI-LD attribute family carries (ETSI GS CIM 009 clause 5.2).

use crate::attribute::instance::AttributeValue;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::Serialize;
use serde_with::skip_serializing_none;

/// Members present on all NGSI-LD attribute families (ETSI GS CIM 009 clause 5.2).
///
/// Every member is written flat beside the family's own, the sub-attributes
/// among them.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonAttributeFields {
    /// System-generated creation timestamp.
    pub created_at: Option<DateTime<Utc>>,
    /// Dataset identifier separating multi-instance attributes.
    pub dataset_id: Option<Identifier>,
    /// System-generated deletion timestamp.
    pub deleted_at: Option<DateTime<Utc>>,
    /// Attribute expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// URI uniquely identifying an attribute instance.
    pub instance_id: Option<Identifier>,
    /// System-generated last modification timestamp.
    pub modified_at: Option<DateTime<Utc>>,
    /// Observation timestamp for the attribute value.
    pub observed_at: Option<DateTime<Utc>>,
    /// Attributes of this attribute, keyed by attribute name.
    #[serde(flatten)]
    pub sub_attributes: IndexMap<AttributeName, AttributeValue>,
}
