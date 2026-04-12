use crate::{
    domain::attribute::{
        AttributeValue,
        common::{CommonAttributeFields, OneOrMany},
    },
    jsonld::JsonLdExpandableMacro,
};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD VocabProperty representation (ETSI GS CIM 009 clause 5.2.35).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:VocabProperty")]
pub struct VocabProperty {
    /// One or more vocabulary references.
    #[jsonld(property = "ngsi:hasVocab", custom)]
    pub vocab: OneOrMany<CompactString>,
    /// Optional JSON-LD datatype for `vocab`.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<CompactString>,
    /// Previous vocabulary reference emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousVocab", custom)]
    pub previous_vocab: Option<OneOrMany<CompactString>>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

impl Default for VocabProperty {
    fn default() -> VocabProperty {
        VocabProperty {
            vocab: OneOrMany::Many(Vec::new()),
            value_type: None,
            previous_vocab: None,
            common: CommonAttributeFields::default(),
            sub_attributes: IndexMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::attribute::{Attribute, common::OneOrMany},
        jsonld::{JsonLdExpandable, vocab},
    };

    #[test]
    fn vocab_property_round_trip_supports_single_and_many() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": ["commercial", "schema:Offer"],
            "previousVocab": "private"
        });
        let attr: Attribute = serde_json::from_value(json).expect("vocab property");
        let Attribute::VocabProperty(property) = attr else {
            panic!("expected VocabProperty");
        };
        assert!(matches!(property.vocab, OneOrMany::Many(_)));
        assert!(property.previous_vocab.is_some());
    }

    #[test]
    fn vocab_property_expanded_uses_vocab_members() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": "commercial",
            "valueType": "xsd:string"
        });
        let attr: Attribute = serde_json::from_value(json).expect("vocab property");
        let Attribute::VocabProperty(property) = attr else {
            panic!("expected VocabProperty");
        };
        let expanded = property.to_expanded();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::VOCAB.as_str()));
        assert!(obj.contains_key(vocab::VALUE_TYPE.as_str()));
    }
}
