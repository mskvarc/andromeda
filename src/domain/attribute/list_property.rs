use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD ListProperty representation (ETSI GS CIM 009 clause 5.2.36).
///
/// A ListProperty represents an attribute whose value is an ordered list
/// of JSON values. Contains the value list, optional metadata
/// (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/ListProperty")]
pub struct ListProperty {
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/hasValueList", list)]
    pub value_list: Vec<Value>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/observedAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(
        property = "https://uri.etsi.org/ngsi-ld/expiresAt",
        typed_value,
        datatype = "https://uri.etsi.org/ngsi-ld/DateTime"
    )]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn list_property_expanded_has_list_container() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3]
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("list_property");
        let crate::domain::attribute::Attribute::ListProperty(lp) = &attr else {
            panic!("expected ListProperty");
        };
        let expanded = lp.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/ListProperty"]));
        let has_value_list = &obj["https://uri.etsi.org/ngsi-ld/hasValueList"];
        assert_eq!(has_value_list, &serde_json::json!([{"@list": [1, 2, 3]}]));
    }
}
