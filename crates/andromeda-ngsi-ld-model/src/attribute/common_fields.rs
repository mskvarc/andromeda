//! The members every NGSI-LD attribute family carries (ETSI GS CIM 009 clause 5.2).

use crate::attribute::instance::AttributeValue;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Members present on all NGSI-LD attribute families (ETSI GS CIM 009 clause 5.2).
///
/// Every member is declared here rather than in a nested fragment: two
/// `#[serde(flatten)]` targets on one struct both receive whatever the named
/// fields did not claim, so a nested system-fields fragment would leave
/// `createdAt`, `modifiedAt`, `instanceId` and `deletedAt` sitting in
/// `sub_attributes` as well as in their own fields. One flat struct plus one
/// flattened map is what keeps the sub-attribute map holding only
/// sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommonAttributeFields {
    /// System-generated creation timestamp.
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    /// Dataset identifier separating multi-instance attributes.
    pub dataset_id: Option<Identifier>,
    /// System-generated deletion timestamp.
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Attribute expiration timestamp.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// URI uniquely identifying an attribute instance.
    pub instance_id: Option<Identifier>,
    /// System-generated last modification timestamp.
    #[serde(default)]
    pub modified_at: Option<DateTime<Utc>>,
    /// Observation timestamp for the attribute value.
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    /// Attributes of this attribute, keyed by attribute name.
    #[serde(flatten)]
    pub sub_attributes: IndexMap<AttributeName, AttributeValue>,
}
