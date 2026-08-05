//! The attribute discovery resources (ETSI GS CIM 009 clauses 5.2.27 and 5.2.28).

use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    constant_string,
    entity_type_name::EntityTypeName,
    ngsi_ld_urn::AttributeListId,
};
use iri_rs::IriBuf;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(AttributeListType, "AttributeList");
constant_string!(AttributeResourceType, "Attribute");

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
    pub type_: AttributeListType,
    /// Names of every attribute the broker holds, compacted against the
    /// request's context where one applies.
    #[jsonld(property = "ngsi:attributeList", coerce = "@id", vec)]
    pub attribute_list: Vec<AttributeName>,
}

/// Detailed information about a single attribute (ETSI GS CIM 009 clause 5.2.28).
///
/// Provides the attribute name, types, usage count, and associated entity
/// type names.
#[derive(Debug, Clone, Serialize, Deserialize, Expandable, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:Attribute",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct AttributeInfo {
    /// Expanded IRI of the attribute.
    #[jsonld(id)]
    pub id: IriBuf,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    #[builder(default)]
    pub type_: AttributeResourceType,
    /// Compacted name of the attribute.
    #[jsonld(property = "ngsi:attributeName", coerce = "@id")]
    pub attribute_name: AttributeName,
    /// Attribute families this attribute has been seen as.
    #[jsonld(property = "ngsi:attributeTypes", coerce = "@id", vec)]
    #[builder(default)]
    pub attribute_types: Vec<AttributeType>,
    /// Number of entity instances carrying this attribute.
    #[jsonld(property = "ngsi:attributeCount")]
    #[builder(default)]
    pub attribute_count: Option<i64>,
    /// Entity types on which this attribute appears.
    #[jsonld(property = "ngsi:typeNames", coerce = "@id", vec)]
    #[builder(default)]
    pub type_names: Option<Vec<EntityTypeName>>,
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
    pub id: IriBuf,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    #[jsonld(skip)]
    pub type_: AttributeResourceType,
    /// Compacted name of the attribute.
    #[jsonld(property = "ngsi:attributeName", coerce = "@id")]
    pub attribute_name: AttributeName,
    /// Attribute families this attribute has been seen as.
    #[jsonld(property = "ngsi:attributeTypes", coerce = "@id", vec)]
    pub attribute_types: Vec<AttributeType>,
}

#[cfg(test)]
mod tests {
    use crate::attribute_list::{AttributeDetail, AttributeInfo, AttributeList, AttributeListType, AttributeResourceType};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        ngsi_ld_urn::AttributeListId,
    };
    use serde_json::json;

    #[test]
    fn an_attribute_list_serialises_its_fixed_type_token() {
        let list = AttributeList {
            id: "urn:ngsi-ld:AttributeList:1".parse::<AttributeListId>().expect("valid list id"),
            type_: AttributeListType,
            attribute_list: Vec::new(),
        };
        let rendered = serde_json::to_value(&list).expect("serialises");
        pretty_assertions::assert_eq!(rendered["type"], json!("AttributeList"));
    }

    #[test]
    fn an_attribute_list_rejects_any_other_type_token() {
        let error = serde_json::from_value::<AttributeList>(json!({
            "id": "urn:ngsi-ld:AttributeList:1",
            "type": "EntityTypeList",
            "attributeList": []
        }))
        .expect_err("a foreign discriminator is rejected");
        assert!(error.to_string().contains("invalid value"), "{error}");
    }

    #[test]
    fn an_attribute_list_carries_compacted_attribute_names() {
        let list: AttributeList = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:AttributeList:1",
            "type": "AttributeList",
            "attributeList": ["speed", "https://example.com/temperature"]
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(
            list.attribute_list,
            vec![
                "speed".parse::<AttributeName>().expect("valid name"),
                "https://example.com/temperature".parse::<AttributeName>().expect("valid IRI"),
            ]
        );
    }

    #[test]
    fn an_attribute_info_carries_typed_attribute_types() {
        let info: AttributeInfo = serde_json::from_value(json!({
            "id": "https://example.com/speed",
            "type": "Attribute",
            "attributeName": "speed",
            "attributeTypes": ["Property", "Relationship"],
            "typeNames": ["Vehicle"]
        }))
        .expect("parses");
        pretty_assertions::assert_eq!(info.type_, AttributeResourceType);
        pretty_assertions::assert_eq!(info.attribute_types, vec![AttributeType::Property, AttributeType::Relationship]);
        pretty_assertions::assert_eq!(info.type_names, Some(vec!["Vehicle".parse::<EntityTypeName>().expect("valid type name")]));
    }

    #[test]
    fn an_attribute_info_rejects_an_unknown_attribute_type() {
        assert!(
            serde_json::from_value::<AttributeInfo>(json!({
                "id": "https://example.com/speed",
                "type": "Attribute",
                "attributeName": "speed",
                "attributeTypes": ["Teleporter"]
            }))
            .is_err()
        );
    }

    #[test]
    fn an_attribute_detail_rejects_a_name_outside_the_clause_four_six_two_grammar() {
        assert!(
            serde_json::from_value::<AttributeDetail>(json!({
                "id": "https://example.com/speed",
                "type": "Attribute",
                "attributeName": "",
                "attributeTypes": ["Property"]
            }))
            .is_err()
        );
    }
}
