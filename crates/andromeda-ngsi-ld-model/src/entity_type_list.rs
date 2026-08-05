//! The entity-type discovery resources (ETSI GS CIM 009 clauses 5.2.24 to 5.2.26).

use crate::attribute_list::AttributeDetail;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, constant_string, entity_type_name::EntityTypeName, ngsi_ld_urn::EntityTypeListId};
use iri_rs::IriBuf;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(EntityTypeListType, "EntityTypeList");
constant_string!(EntityTypeType, "EntityType");
constant_string!(EntityTypeInfoType, "EntityTypeInfo");

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
    pub type_: EntityTypeListType,
    /// Names of every entity type the broker holds, compacted against the
    /// request's context where one applies.
    #[jsonld(property = "ngsi:typeList", coerce = "@id", vec)]
    pub type_list: Vec<EntityTypeName>,
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
    pub id: IriBuf,
    /// Fixed `EntityType` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: EntityTypeType,
    /// Compacted name of the entity type.
    #[jsonld(property = "ngsi:typeName", coerce = "@id")]
    pub type_name: EntityTypeName,
    /// Names of every attribute seen on entities of this type.
    #[jsonld(property = "ngsi:attributeNames", coerce = "@id", vec)]
    pub attribute_names: Vec<AttributeName>,
}

/// Detailed information about an entity type (ETSI GS CIM 009 clause 5.2.26).
///
/// Provides the type name, entity count, and details of all attributes
/// associated with entities of this type.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:EntityTypeInfo",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct EntityTypeInfo {
    /// Expanded IRI of the entity type.
    #[jsonld(id)]
    pub id: IriBuf,
    /// Fixed `EntityTypeInfo` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    #[builder(default)]
    pub type_: EntityTypeInfoType,
    /// Compacted name of the entity type.
    #[jsonld(property = "ngsi:typeName", coerce = "@id")]
    pub type_name: EntityTypeName,
    /// Number of entities currently held for this type.
    #[jsonld(property = "ngsi:entityCount")]
    pub entity_count: i64,
    /// One record per attribute seen on entities of this type.
    #[jsonld(property = "ngsi:attributeDetails", nested, vec)]
    #[builder(default)]
    pub attribute_details: Vec<AttributeDetail>,
}

#[cfg(test)]
mod tests {
    use crate::entity_type_list::{EntityType, EntityTypeInfo, EntityTypeInfoType, EntityTypeList, EntityTypeListType};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, ngsi_ld_urn::EntityTypeListId};
    use serde_json::json;

    #[test]
    fn an_entity_type_list_serialises_its_fixed_type_token() {
        let list = EntityTypeList {
            id: "urn:ngsi-ld:EntityTypeList:1".parse::<EntityTypeListId>().expect("valid list id"),
            type_: EntityTypeListType,
            type_list: Vec::new(),
        };
        let rendered = serde_json::to_value(&list).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("EntityTypeList"));
    }

    #[test]
    fn an_entity_type_list_rejects_any_other_type_token() {
        let error = serde_json::from_value::<EntityTypeList>(json!({
            "id": "urn:ngsi-ld:EntityTypeList:1",
            "type": "AttributeList",
            "typeList": []
        }))
        .expect_err("a foreign discriminator is rejected");
        assert!(error.to_string().contains("invalid value"), "{error}");
    }

    #[test]
    fn an_entity_type_carries_typed_attribute_names() {
        let entity_type: EntityType = serde_json::from_value(json!({
            "id": "https://example.com/Vehicle",
            "type": "EntityType",
            "typeName": "Vehicle",
            "attributeNames": ["https://example.com/speed"]
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(entity_type.type_name, "Vehicle".parse::<EntityTypeName>().expect("valid type name"));
        pretty_assertions::assert_eq!(
            entity_type.attribute_names,
            vec!["https://example.com/speed".parse::<AttributeName>().expect("valid IRI")]
        );
    }

    #[test]
    fn an_entity_type_rejects_a_name_outside_the_clause_four_six_two_grammar() {
        assert!(
            serde_json::from_value::<EntityType>(json!({
                "id": "https://example.com/Vehicle",
                "type": "EntityType",
                "typeName": "Vehicle",
                "attributeNames": [""]
            }))
            .is_err()
        );
    }

    #[test]
    fn an_entity_type_info_carries_its_fixed_type_token_and_entity_count() {
        let info: EntityTypeInfo = serde_json::from_value(json!({
            "id": "https://example.com/Vehicle",
            "type": "EntityTypeInfo",
            "typeName": "Vehicle",
            "entityCount": 7,
            "attributeDetails": []
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(info.type_, EntityTypeInfoType);
        pretty_assertions::assert_eq!(info.entity_count, 7);
    }
}
