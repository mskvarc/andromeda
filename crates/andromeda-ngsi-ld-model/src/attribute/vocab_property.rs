//! The `VocabProperty` attribute family (ETSI GS CIM 009 clause 5.2.35).

use crate::attribute::{common_fields::CommonAttributeFields, one_or_many::OneOrMany};
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use serde::Serialize;
use serde_with::skip_serializing_none;

/// NGSI-LD `VocabProperty` representation (ETSI GS CIM 009 clause 5.2.35).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabProperty {
    /// One or more vocabulary values, as written: any string the `@context`
    /// can coerce to a URI (Table 5.2.35-1).
    pub vocab: OneOrMany<CoercedTerm>,
    /// Optional JSON-LD datatype for `vocab`, as written (Table 5.2.35-1).
    pub value_type: Option<CoercedTerm>,
    /// Previous vocabulary values emitted in notifications when `showChanges` is active.
    pub previous_vocab: Option<OneOrMany<CoercedTerm>>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

#[cfg(test)]
mod tests {
    use crate::attribute::{instance::Attribute, one_or_many::OneOrMany};
    use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;

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
    fn a_hyphenated_vocab_term_decodes_as_a_single_value_and_as_an_array() {
        let single: Attribute = serde_json::from_value(serde_json::json!({"type": "VocabProperty", "vocab": "non-commercial"})).expect("single vocab");
        let Attribute::VocabProperty(single) = single else {
            panic!("expected VocabProperty");
        };
        let terms: Vec<&str> = single.vocab.iter().map(CoercedTerm::as_str).collect();
        pretty_assertions::assert_eq!(terms, vec!["non-commercial"]);
        assert!(matches!(single.vocab, OneOrMany::One(_)));

        let many: Attribute =
            serde_json::from_value(serde_json::json!({"type": "VocabProperty", "vocab": ["non-commercial", "commercial"], "previousVocab": "non-commercial"}))
                .expect("array vocab");
        let Attribute::VocabProperty(many) = many else {
            panic!("expected VocabProperty");
        };
        let terms: Vec<&str> = many.vocab.iter().map(CoercedTerm::as_str).collect();
        pretty_assertions::assert_eq!(terms, vec!["non-commercial", "commercial"]);
        assert!(many.previous_vocab.is_some());
    }

    #[test]
    fn a_vocab_value_holding_whitespace_is_refused() {
        assert!(serde_json::from_value::<Attribute>(serde_json::json!({"type": "VocabProperty", "vocab": "non commercial"})).is_err());
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
