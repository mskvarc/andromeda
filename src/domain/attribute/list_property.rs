use crate::domain::attribute::{
    AttributeValue,
    common::{CommonAttributeFields, NgsiLdValue},
};
use compact_str::CompactString;
use indexmap::IndexMap;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD ListProperty representation (ETSI GS CIM 009 clause 5.2.36).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:ListProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct ListProperty {
    #[jsonld(property = "ngsi:hasValueList", container = "list")]
    pub value_list: Vec<NgsiLdValue>,
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<CompactString>,
    #[jsonld(property = "ngsi:hasPreviousValueList", container = "list")]
    pub previous_value_list: Option<Vec<NgsiLdValue>>,
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for ListProperty {
    fn default() -> ListProperty {
        ListProperty {
            value_list: Vec::new(),
            value_type: None,
            previous_value_list: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use jsonld::Expandable;

    #[test]
    fn list_property_expanded_has_list_container() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "previousValueList": [0, 1, 2]
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("list_property");
        let crate::domain::attribute::Attribute::ListProperty(lp) = &attr else {
            unreachable!("expected ListProperty");
        };
        let expanded = lp.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("expanded is object");

        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/ListProperty"]));
        let has_value_list = &obj["https://uri.etsi.org/ngsi-ld/hasValueList"];
        pretty_assertions::assert_eq!(has_value_list, &serde_json::json!([{"@list": [1, 2, 3]}]));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/hasPreviousValueList"));
    }
}
