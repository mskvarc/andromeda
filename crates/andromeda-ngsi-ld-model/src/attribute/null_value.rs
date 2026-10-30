//! Whether an attribute instance holds its family's NGSI-LD Null in place of
//! its value (ETSI GS CIM 009 clause 4.5.0).
//!
//! The Null is never data (clause 5.5.4), so an instance holding it is a
//! deletion: the object form clause 5.8.6 reports a deleted attribute in when
//! a `datasetId`, the system attributes or a previous value are reported with
//! it, and clause 4.5.7 records a deleted instance in.

use crate::attribute::{
    geo_property_value::GeoPropertyValue,
    instance::Attribute,
    language_property::{LanguageMapKey, LanguageMapValue},
    one_or_many::OneOrMany,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_representation::compact_node::CompactNode;
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::is_ngsi_ld_null;

impl Attribute {
    /// Whether the instance's value member is the NGSI-LD Null its family is
    /// deleted by: the Null itself under `value`, `object`, `vocab` and
    /// `json`, the array of the one Null under `valueList` and `objectList`,
    /// and the language map holding it under `@none` alone.
    #[must_use]
    pub fn holds_ngsi_ld_null(&self) -> bool {
        match self {
            Attribute::Property(property) => is_null_literal(property.value.literal()),
            Attribute::GeoProperty(geo_property) => geo_property.value == GeoPropertyValue::Null,
            Attribute::Relationship(relationship) => is_only_null(&relationship.object, |uri| uri.as_str()),
            Attribute::VocabProperty(vocab_property) => is_only_null(&vocab_property.vocab, |term| term.as_str()),
            Attribute::JsonProperty(json_property) => is_null_literal(&json_property.json),
            Attribute::LanguageProperty(language_property) => {
                language_property.language_map.len() == 1
                    && matches!(
                        language_property.language_map.get(&LanguageMapKey::None),
                        Some(LanguageMapValue::Single(text)) if is_ngsi_ld_null(text)
                    )
            }
            Attribute::ListProperty(list_property) => matches!(list_property.value_list.as_slice(), [only] if is_null_literal(only.literal())),
            Attribute::ListRelationship(list_relationship) => {
                matches!(list_relationship.object_list.as_slice(), [only] if is_ngsi_ld_null(only.as_str()))
            }
        }
    }
}

/// Whether `literal` is the Null written as a JSON string.
fn is_null_literal(literal: &JsonLiteral) -> bool {
    literal.piece().is_ngsi_ld_null()
}

/// Whether `values` hold the Null and nothing else.
fn is_only_null<T>(values: &OneOrMany<T>, text: impl Fn(&T) -> &str) -> bool {
    match values {
        OneOrMany::One(value) => is_ngsi_ld_null(text(value)),
        OneOrMany::Many(values) => matches!(values.as_slice(), [only] if is_ngsi_ld_null(text(only))),
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use andromeda_json_literal::piece::JsonPiece;
    use serde_json::{Value, json, value::RawValue};
    use std::error::Error;

    /// The one instance `instance` reads as.
    fn instance(instance: &Value) -> Result<Attribute, Box<dyn Error>> {
        let text = RawValue::from_string(instance.to_string())?;
        Attribute::from_node("member", JsonPiece::new(&text)).map_err(|error| format!("{error:?}").into())
    }

    /// Clause 4.5.0 and the family clauses: each family's deletion encoding,
    /// with and without what clause 5.8.6 reports beside it.
    #[test]
    fn every_family_holding_its_deletion_encoding_holds_the_null() -> Result<(), Box<dyn Error>> {
        for written in [
            json!({"type": "Property", "value": "urn:ngsi-ld:null"}),
            json!({"type": "GeoProperty", "value": "urn:ngsi-ld:null"}),
            json!({"type": "Relationship", "object": "urn:ngsi-ld:null", "datasetId": "urn:ngsi-ld:Dataset:a"}),
            json!({"type": "VocabProperty", "vocab": "urn:ngsi-ld:null"}),
            json!({"type": "JsonProperty", "json": "urn:ngsi-ld:null"}),
            json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}}),
            json!({"type": "ListProperty", "valueList": ["urn:ngsi-ld:null"], "createdAt": "2026-01-01T00:00:00Z"}),
            json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:null"]}),
        ] {
            assert!(instance(&written)?.holds_ngsi_ld_null(), "{written}");
        }
        Ok(())
    }

    /// Data, and the Null beside data, is no deletion.
    #[test]
    fn an_instance_holding_data_holds_no_null() -> Result<(), Box<dyn Error>> {
        for written in [
            json!({"type": "Property", "value": 5}),
            json!({"type": "Property", "value": {"street": "urn:ngsi-ld:null"}}),
            json!({"type": "Property", "value": "https://uri.etsi.org/ngsi-ld/null"}),
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}}),
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:a"}),
            json!({"type": "VocabProperty", "vocab": "Tower"}),
            json!({"type": "JsonProperty", "json": {"a": "urn:ngsi-ld:null"}}),
            json!({"type": "LanguageProperty", "languageMap": {"en": "urn:ngsi-ld:null"}}),
            json!({"type": "ListProperty", "valueList": [1, 2]}),
            json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:a"]}),
        ] {
            assert!(!instance(&written)?.holds_ngsi_ld_null(), "{written}");
        }
        Ok(())
    }
}
