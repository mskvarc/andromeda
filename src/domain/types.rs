use crate::jsonld::JsonLdExpandableMacro;
use serde::{Deserialize, Serialize};
use urn::Urn;

/// NGSI-LD EntityTypeList representation (ETSI GS CIM 009 clause 5.2.24).
///
/// Lists all entity types known to the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Types {
    pub id: Urn,
    #[serde(rename = "type")]
    pub type_: String,
    pub type_list: Vec<String>,
}

/// Detailed information about an entity type (ETSI GS CIM 009 clause 5.2.26).
///
/// Provides the type name, entity count, and details of all attributes
/// associated with entities of this type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/EntityTypeInfo")]
pub struct EntityTypeInfo {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/typeName", vocab)]
    pub type_name: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/entityCount")]
    pub entity_count: i64,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeDetails", nested, vec)]
    pub attribute_details: Vec<AttributeDetail>,
}

/// Detail record for a single attribute within an EntityTypeInfo (ETSI GS CIM 009 clause 5.2.26).
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/Attribute")]
pub struct AttributeDetail {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeName", vocab)]
    pub attribute_name: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeTypes", vocab_vec)]
    pub attribute_types: Vec<String>,
}

/// Summary of an entity type (ETSI GS CIM 009 clause 5.2.25).
///
/// Provides the type name and all attribute names associated with entities
/// of this type.
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/EntityType")]
pub struct EntityType {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/typeName", vocab)]
    pub type_name: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeNames", vocab_vec)]
    pub attribute_names: Vec<String>,
}

/// NGSI-LD AttributeList representation (ETSI GS CIM 009 clause 5.2.27).
///
/// Lists all attributes known to the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeList {
    pub id: Urn,
    #[serde(rename = "type")]
    pub type_: String,
    pub attribute_list: Vec<String>,
}

/// Detailed information about a single attribute (ETSI GS CIM 009 clause 5.2.28).
///
/// Provides the attribute name, types, usage count, and associated entity
/// type names.
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/Attribute")]
pub struct Attribute {
    #[jsonld(id)]
    pub id: String,
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeName", vocab)]
    pub attribute_name: String,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeTypes", vocab_vec)]
    pub attribute_types: Vec<String>,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeCount")]
    pub attribute_count: Option<i64>,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/typeNames", vocab_vec)]
    pub type_names: Option<Vec<String>>,
}
