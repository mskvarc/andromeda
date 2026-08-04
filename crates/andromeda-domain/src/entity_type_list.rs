//! The entity-type discovery resources (ETSI GS CIM 009 clauses 5.2.24 to 5.2.26).

use crate::{attribute_list::AttributeDetail, ngsi_ld_urn::EntityTypeListId};
use jsonld::Expandable;
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
pub struct EntityTypeList {
    /// Generated identifier of this list resource.
    #[jsonld(id)]
    pub id: EntityTypeListId,
    /// Fixed `EntityTypeList` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Expanded IRIs of every entity type the broker holds.
    #[jsonld(property = "ngsi:typeList", coerce = "@id", vec)]
    pub type_list: Vec<String>,
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
    /// Expanded IRI of the entity type.
    #[jsonld(id)]
    pub id: String,
    /// Fixed `EntityType` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Compacted name of the entity type.
    #[jsonld(property = "ngsi:typeName", coerce = "@id")]
    pub type_name: String,
    /// Names of every attribute seen on entities of this type.
    #[jsonld(property = "ngsi:attributeNames", coerce = "@id", vec)]
    pub attribute_names: Vec<String>,
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
    /// Expanded IRI of the entity type.
    #[jsonld(id)]
    pub id: String,
    /// Fixed `EntityTypeInfo` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: String,
    /// Compacted name of the entity type.
    #[jsonld(property = "ngsi:typeName", coerce = "@id")]
    pub type_name: String,
    /// Number of entities currently held for this type.
    #[jsonld(property = "ngsi:entityCount")]
    pub entity_count: i64,
    /// One record per attribute seen on entities of this type.
    #[jsonld(property = "ngsi:attributeDetails", nested, vec)]
    pub attribute_details: Vec<AttributeDetail>,
}
