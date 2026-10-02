//! The `VocabProperty` attribute family (ETSI GS CIM 009 clause 5.2.35).

use crate::attribute::{common_fields::CommonAttributeFields, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD `VocabProperty` representation (ETSI GS CIM 009 clause 5.2.35).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabProperty {
    /// One or more vocabulary references.
    pub vocab: OneOrMany<NgsiLdReference>,
    /// Optional JSON-LD datatype for `vocab`.
    pub value_type: Option<NgsiLdReference>,
    /// Previous vocabulary reference emitted in notifications when `showChanges` is active.
    pub previous_vocab: Option<OneOrMany<NgsiLdReference>>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, one_or_many::OneOrMany};

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
    fn the_vocab_property_default_matches_the_field_by_field_literal() {
        use crate::attribute::vocab_property::VocabProperty;
        let actual = VocabProperty::default();
        pretty_assertions::assert_eq!(actual.vocab, OneOrMany::Many(Vec::new()));
        assert!(actual.value_type.is_none() && actual.previous_vocab.is_none());
        assert!(actual.common.sub_attributes.is_empty());
        assert!(actual.common.dataset_id.is_none() && actual.common.observed_at.is_none() && actual.common.expires_at.is_none());
    }
}
