use crate::{
    domain::ngsi_ld_urn::{AttributeListId, EntityTypeListId},
    jsonld::Expandable,
};
use serde::{Deserialize, Serialize};

/// NGSI-LD EntityTypeList representation (ETSI GS CIM 009 clause 5.2.24).
///
/// Lists all entity types known to the broker.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:EntityTypeList",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct Types {
    #[jsonld(id)]
    pub id: EntityTypeListId,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:typeList", vocab_vec)]
    pub type_list: Vec<String>,
}

/// Detailed information about an entity type (ETSI GS CIM 009 clause 5.2.26).
///
/// Provides the type name, entity count, and details of all attributes
/// associated with entities of this type.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:EntityTypeInfo",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct EntityTypeInfo {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:typeName", vocab)]
    pub type_name: String,
    #[jsonld(property = "ngsi:entityCount")]
    pub entity_count: i64,
    #[jsonld(property = "ngsi:attributeDetails", nested, vec)]
    pub attribute_details: Vec<AttributeDetail>,
}

/// Detail record for a single attribute within an EntityTypeInfo (ETSI GS CIM 009 clause 5.2.26).
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Attribute",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct AttributeDetail {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:attributeName", vocab)]
    pub attribute_name: String,
    #[jsonld(property = "ngsi:attributeTypes", vocab_vec)]
    pub attribute_types: Vec<String>,
}

/// Summary of an entity type (ETSI GS CIM 009 clause 5.2.25).
///
/// Provides the type name and all attribute names associated with entities
/// of this type.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:EntityType",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct EntityType {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:typeName", vocab)]
    pub type_name: String,
    #[jsonld(property = "ngsi:attributeNames", vocab_vec)]
    pub attribute_names: Vec<String>,
}

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
    #[jsonld(id)]
    pub id: AttributeListId,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:attributeList", vocab_vec)]
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
pub struct Attribute {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "ngsi:attributeName", vocab)]
    pub attribute_name: String,
    #[jsonld(property = "ngsi:attributeTypes", vocab_vec)]
    pub attribute_types: Vec<String>,
    #[jsonld(property = "ngsi:attributeCount")]
    pub attribute_count: Option<i64>,
    #[jsonld(property = "ngsi:typeNames", vocab_vec)]
    pub type_names: Option<Vec<String>>,
}
