//! The attribute discovery resources (ETSI GS CIM 009 clauses 5.2.27 and 5.2.28).

use crate::ngsi_ld_urn::AttributeListId;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};

/// NGSI-LD AttributeList representation (ETSI GS CIM 009 clause 5.2.27).
///
/// Lists all attributes known to the broker.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:AttributeList",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct AttributeList {
    /// Generated identifier of this list resource.
    #[jsonld(id)]
    pub id: AttributeListId,
    /// Fixed `AttributeList` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Expanded IRIs of every attribute the broker holds.
    #[jsonld(property = "ngsi:attributeList", coerce = "@id", vec)]
    pub attribute_list: Vec<String>,
}

/// Detailed information about a single attribute (ETSI GS CIM 009 clause 5.2.28).
///
/// Provides the attribute name, types, usage count, and associated entity
/// type names.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Attribute",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct AttributeInfo {
    /// Expanded IRI of the attribute.
    #[jsonld(id)]
    pub id: String,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Compacted name of the attribute.
    #[jsonld(property = "ngsi:attributeName", coerce = "@id")]
    pub attribute_name: String,
    /// Attribute families this attribute has been seen as.
    #[jsonld(property = "ngsi:attributeTypes", coerce = "@id", vec)]
    pub attribute_types: Vec<String>,
    /// Number of entity instances carrying this attribute.
    #[jsonld(property = "ngsi:attributeCount")]
    pub attribute_count: Option<i64>,
    /// Entity types on which this attribute appears.
    #[jsonld(property = "ngsi:typeNames", coerce = "@id", vec)]
    pub type_names: Option<Vec<String>>,
}

/// Detail record for a single attribute within an EntityTypeInfo (ETSI GS CIM 009 clause 5.2.26).
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Attribute",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct AttributeDetail {
    /// Expanded IRI of the attribute.
    #[jsonld(id)]
    pub id: String,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Compacted name of the attribute.
    #[jsonld(property = "ngsi:attributeName", coerce = "@id")]
    pub attribute_name: String,
    /// Attribute families this attribute has been seen as.
    #[jsonld(property = "ngsi:attributeTypes", coerce = "@id", vec)]
    pub attribute_types: Vec<String>,
}
