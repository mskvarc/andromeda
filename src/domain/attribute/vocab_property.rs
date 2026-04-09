use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

/// NGSI-LD VocabProperty representation (ETSI GS CIM 009 clause 5.2.35).
///
/// A VocabProperty represents an attribute whose value is a vocabulary term
/// (compact IRI or full IRI). Contains the vocab value, optional previous
/// value, metadata (observedAt, datasetId), and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:VocabProperty")]
pub struct VocabProperty {
    #[jsonld(property = "ngsi:hasVocab", vocab_polymorphic)]
    pub vocab: Value,
    #[jsonld(skip)]
    pub previous_vocab: Option<Value>,
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", typed_value, datatype = "ngsi:DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", typed_value, datatype = "ngsi:DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "ngsi:datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn vocab_property_expanded_string_vocab_produces_id() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": "categoryA"
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("vocab_property");
        let crate::domain::attribute::Attribute::VocabProperty(v) = &attr else {
            panic!("expected VocabProperty");
        };
        let expanded = v.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/VocabProperty"]));
        assert_eq!(obj["https://uri.etsi.org/ngsi-ld/hasVocab"], serde_json::json!([{"@id": "categoryA"}]));
    }

    #[test]
    fn vocab_property_expanded_array_vocab_produces_id_array() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": ["catA", "catB"]
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("vocab_property");
        let crate::domain::attribute::Attribute::VocabProperty(v) = &attr else {
            panic!("expected VocabProperty");
        };
        let expanded = v.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(
            obj["https://uri.etsi.org/ngsi-ld/hasVocab"],
            serde_json::json!([{"@id": "catA"}, {"@id": "catB"}])
        );
    }

    #[test]
    fn vocab_property_expanded_includes_metadata() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": "val",
            "observedAt": "2024-01-01T00:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:1"
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("vocab_property");
        let crate::domain::attribute::Attribute::VocabProperty(v) = &attr else {
            panic!("expected VocabProperty");
        };
        let expanded = v.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/observedAt"));
        assert!(obj.contains_key("https://uri.etsi.org/ngsi-ld/datasetId"));
    }
}
