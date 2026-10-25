//! The attribute discovery resources (ETSI GS CIM 009 clauses 5.2.27 and 5.2.28).

use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    constant_string,
    entity_type_name::EntityTypeName,
    ngsi_ld_urn::AttributeListId,
};
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(AttributeListType, "AttributeList");
constant_string!(AttributeResourceType, "Attribute");

/// NGSI-LD `AttributeList` representation (ETSI GS CIM 009 clause 5.2.27).
///
/// Lists all attributes known to the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeList {
    /// Generated identifier of this list resource.
    pub id: AttributeListId,
    /// Fixed `AttributeList` discriminator.
    #[serde(rename = "type")]
    pub type_: AttributeListType,
    /// Names of every attribute the broker holds, compacted against the
    /// request's context where one applies.
    pub attribute_list: Vec<AttributeName>,
}

/// Detailed information about a single attribute (ETSI GS CIM 009 clause 5.2.28).
///
/// Provides the attribute name, types, usage count, and associated entity
/// type names.
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct AttributeInfo {
    /// Full URI of the attribute name.
    ///
    /// Table 5.2.28-1 restricts the member to the "Full URI of attribute
    /// name", so it is never shortened, whatever the request's `@context`
    /// declares for the same name.
    pub id: IriBuf,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    #[builder(default = Default::default())]
    pub type_: AttributeResourceType,
    /// Compacted name of the attribute.
    pub attribute_name: AttributeName,
    /// Attribute families this attribute has been seen as.
    ///
    /// Table 5.2.28-1 gives the member cardinality 0..1, so a record that
    /// names none omits it rather than carrying an empty array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default = Default::default())]
    pub attribute_types: Vec<AttributeType>,
    /// Table 5.2.28-1 `attributeCount`: the number of attribute instances with
    /// this name, each `datasetId` instance counted (clause 4.5.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub attribute_count: Option<u64>,
    /// Entity types on which this attribute appears.
    ///
    /// Table 5.2.28-1 gives the member cardinality 0..1, so a record that
    /// names none omits it rather than carrying an empty array.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default = Default::default())]
    pub type_names: Vec<EntityTypeName>,
}

/// Detail record for a single attribute within an `EntityTypeInfo` (ETSI GS CIM 009 clause 5.2.26).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeDetail {
    /// Full URI of the attribute name.
    ///
    /// Table 5.2.28-1 restricts the member to the "Full URI of attribute
    /// name", so it is never shortened.
    pub id: IriBuf,
    /// Fixed `Attribute` discriminator.
    #[serde(rename = "type")]
    pub type_: AttributeResourceType,
    /// Compacted name of the attribute.
    pub attribute_name: AttributeName,
    /// Attribute families this attribute has been seen as.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attribute_types: Vec<AttributeType>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
        pretty_assertions::assert_eq!(info.type_names, vec!["Vehicle".parse::<EntityTypeName>().expect("valid type name")]);
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
