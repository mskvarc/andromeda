//! The `JsonProperty` attribute family (ETSI GS CIM 009 clause 5.2.38).

use crate::attribute::common_fields::CommonAttributeFields;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use serde::Serialize;
use serde_with::skip_serializing_none;

/// NGSI-LD `JsonProperty` representation (ETSI GS CIM 009 clause 5.2.38).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonProperty {
    /// Raw JSON carried as data, never interpreted as JSON-LD (clause
    /// 4.5.24.2).
    pub json: JsonLiteral,
    /// Optional JSON-LD datatype for `json`.
    pub value_type: Option<CoercedTerm>,
    /// Previous JSON payload emitted in notifications when `showChanges` is active.
    pub previous_json: Option<JsonLiteral>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;

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
}
