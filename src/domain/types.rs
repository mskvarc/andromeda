use crate::jsonld::JsonLdExpandableMacro;
use serde::{Deserialize, Serialize};
use urn::Urn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Types {
    pub id: Urn,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "typeList")]
    pub type_list: Vec<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeList {
    pub id: Urn,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "attributeList")]
    pub attribute_list: Vec<String>,
}

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
