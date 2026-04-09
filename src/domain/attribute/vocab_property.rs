use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandable,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabProperty {
    pub vocab: Value,
    pub previous_vocab: Option<Value>,
    #[serde(default)]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

/// Manual impl because `vocab` can be a string (single term) or array (multiple
/// terms) at runtime, requiring dynamic dispatch for correct `@id` wrapping.
impl JsonLdExpandable for VocabProperty {
    fn to_expanded(&self) -> Value {
        let mut obj = serde_json::Map::new();

        obj.insert("@type".to_string(), serde_json::json!(["https://uri.etsi.org/ngsi-ld/VocabProperty"]));

        // vocab can be string -> single @id, or array -> multiple @id entries
        let has_vocab = match &self.vocab {
            Value::String(s) => {
                obj.insert("https://uri.etsi.org/ngsi-ld/hasVocab".to_string(), serde_json::json!([{"@id": s}]));
                true
            }
            Value::Array(arr) => {
                let ids: Vec<Value> = arr.iter().filter_map(|v| v.as_str().map(|s| serde_json::json!({"@id": s}))).collect();
                obj.insert("https://uri.etsi.org/ngsi-ld/hasVocab".to_string(), Value::Array(ids));
                true
            }
            Value::Null => false,
            _other => {
                obj.insert("https://uri.etsi.org/ngsi-ld/hasVocab".to_string(), serde_json::json!([{"@value": self.vocab}]));
                true
            }
        };

        let _ = has_vocab;

        if let Some(ref observed_at) = self.observed_at {
            obj.insert(
                "https://uri.etsi.org/ngsi-ld/observedAt".to_string(),
                serde_json::json!([{"@value": observed_at, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]),
            );
        }

        if let Some(ref expires_at) = self.expires_at {
            obj.insert(
                "https://uri.etsi.org/ngsi-ld/expiresAt".to_string(),
                serde_json::json!([{"@value": expires_at, "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]),
            );
        }

        if let Some(ref dataset_id) = self.dataset_id {
            obj.insert(
                "https://uri.etsi.org/ngsi-ld/datasetId".to_string(),
                serde_json::json!([{"@id": dataset_id.to_string()}]),
            );
        }

        for (key, val) in &self.sub_attributes {
            obj.insert(key.to_string(), JsonLdExpandable::to_expanded(val));
        }

        Value::Object(obj)
    }
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
