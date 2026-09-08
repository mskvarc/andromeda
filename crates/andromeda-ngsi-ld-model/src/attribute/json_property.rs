//! The `JsonProperty` attribute family (ETSI GS CIM 009 clause 5.2.38).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use jsonld::Expandable;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD `JsonProperty` representation (ETSI GS CIM 009 clause 5.2.38).
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
    pub value_type: Option<NgsiLdReference>,
    /// Previous JSON payload emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousJson", coerce = "@json")]
    pub previous_json: Option<Value>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    #[jsonld(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use jsonld::Expandable;
    use serde_json::Value;

    #[test]
    fn json_property_round_trip_supports_previous_json() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": { "key": "value" },
            "previousJson": { "key": "old" }
        });
        let attr: Attribute = serde_json::from_value(json).expect("json property");
        let Attribute::JsonProperty(property) = attr else {
            panic!("expected JsonProperty");
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
            panic!("expected JsonProperty");
        };
        let expanded = property.expand::<serde_json::Value>();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(expanded_properties::PREVIOUS_JSON.as_str()));
    }

    #[test]
    fn the_json_property_default_matches_the_field_by_field_literal() {
        use crate::attribute::json_property::JsonProperty;
        let actual = JsonProperty::default();
        pretty_assertions::assert_eq!(actual.json, Value::Null);
        assert!(actual.value_type.is_none() && actual.previous_json.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
