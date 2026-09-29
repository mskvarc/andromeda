//! The members every NGSI-LD attribute family carries (ETSI GS CIM 009 clause 5.2).

use crate::attribute::instance::AttributeValue;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use jsonld::Expandable;
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(fragment, prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#"))]
pub struct CommonAttributeFields {
    /// System-generated creation timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:createdAt", coerce = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub created_at: Option<DateTime<Utc>>,
    /// Dataset identifier separating multi-instance attributes.
    #[jsonld(property = "ngsi:datasetId", coerce = "@id")]
    pub dataset_id: Option<Identifier>,
    /// System-generated deletion timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:deletedAt", coerce = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub deleted_at: Option<DateTime<Utc>>,
    /// Attribute expiration timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", coerce = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    /// URI uniquely identifying an attribute instance.
    #[jsonld(property = "ngsi:instanceId", coerce = "@id")]
    pub instance_id: Option<Identifier>,
    /// System-generated last modification timestamp.
    #[serde(default)]
    #[jsonld(property = "ngsi:modifiedAt", coerce = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub modified_at: Option<DateTime<Utc>>,
    /// Observation timestamp for the attribute value.
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", coerce = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    /// Attributes of this attribute, keyed by attribute name.
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<AttributeName, AttributeValue>,
}
