use crate::domain::attribute::{AttributeValue, common::CommonAttributeFields};
use compact_str::CompactString;
use indexmap::IndexMap;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD JsonProperty representation (ETSI GS CIM 009 clause 5.2.38).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, Expandable)]
#[serde(rename_all = "camelCase")]
#[jsonld(
    type = "ngsi:JsonProperty",
    prefix(ngsi = "https://uri.etsi.org/ngsi-ld/", xsd = "http://www.w3.org/2001/XMLSchema#")
)]
pub struct JsonProperty {
    /// Raw non-expandable JSON payload.
    #[jsonld(property = "ngsi:hasJSON", coerce = "@json")]
    pub json: Value,
    /// Optional JSON-LD datatype for `json`.
    #[jsonld(property = "ngsi:hasValueType", coerce = "@id")]
    pub value_type: Option<CompactString>,
    /// Previous JSON payload emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousJson", coerce = "@json")]
    pub previous_json: Option<Value>,
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::domain::{attribute::Attribute, vocab};
    use jsonld::Expandable;

    #[test]
    fn json_property_round_trip_supports_previous_json() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": { "key": "value" },
            "previousJson": { "key": "old" }
        });
        let attr: Attribute = serde_json::from_value(json).expect("json property");
        let Attribute::JsonProperty(property) = attr else {
            unreachable!("expected JsonProperty");
        };
        assert!(property.previous_json.is_some());
    }

    #[test]
    fn json_property_expanded_includes_previous_json_member() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": { "key": "value" },
            "previousJson": { "key": "old" }
        });
        let attr: Attribute = serde_json::from_value(json).expect("json property");
        let Attribute::JsonProperty(property) = attr else {
            unreachable!("expected JsonProperty");
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::properties::PREVIOUS_JSON.as_str()));
    }

    #[test]
    fn the_json_property_default_matches_the_field_by_field_literal() {
        use crate::domain::attribute::JsonProperty;
        let actual = JsonProperty::default();
        pretty_assertions::assert_eq!(actual.json, serde_json::Value::Null);
        assert!(actual.value_type.is_none() && actual.previous_json.is_none());
        assert!(actual.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
