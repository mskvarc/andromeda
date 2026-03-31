//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use crate::domain::iri;
use iref::Iri;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The eight NGSI-LD attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum AttributeType {
    Property,
    Relationship,
    GeoProperty,
    VocabProperty,
    LanguageProperty,
    JsonProperty,
    ListProperty,
    ListRelationship,
}

impl AttributeType {
    /// Returns the expanded IRI for this attribute type.
    pub fn iri(self) -> &'static Iri {
        match self {
            Self::Property => iri::PROPERTY,
            Self::Relationship => iri::RELATIONSHIP,
            Self::GeoProperty => iri::GEO_PROPERTY,
            Self::VocabProperty => iri::VOCAB_PROPERTY,
            Self::LanguageProperty => iri::LANGUAGE_PROPERTY,
            Self::JsonProperty => iri::JSON_PROPERTY,
            Self::ListProperty => iri::LIST_PROPERTY,
            Self::ListRelationship => iri::LIST_RELATIONSHIP,
        }
    }

    /// Returns the expanded IRI of the value container for this attribute type.
    pub fn value_key(self) -> &'static Iri {
        match self {
            Self::Property | Self::GeoProperty => iri::HAS_VALUE,
            Self::Relationship => iri::HAS_OBJECT,
            Self::VocabProperty => iri::HAS_VOCAB,
            Self::LanguageProperty => iri::HAS_LANGUAGE_MAP,
            Self::JsonProperty => iri::HAS_JSON,
            Self::ListProperty => iri::HAS_VALUE_LIST,
            Self::ListRelationship => iri::HAS_OBJECT_LIST,
        }
    }

    /// Returns the compacted (short) value key for this attribute type.
    #[allow(dead_code)]
    pub fn compacted_value_key(self) -> &'static str {
        match self {
            Self::Property | Self::GeoProperty => "value",
            Self::Relationship => "object",
            Self::VocabProperty => "vocab",
            Self::LanguageProperty => "languageMap",
            Self::JsonProperty => "json",
            Self::ListProperty => "valueList",
            Self::ListRelationship => "objectList",
        }
    }

    /// Returns the compacted previous-value key for showChanges support.
    pub fn previous_key(self) -> &'static str {
        match self {
            Self::Relationship | Self::ListRelationship => "previousObject",
            Self::LanguageProperty => "previousLanguageMap",
            Self::JsonProperty => "previousJson",
            Self::VocabProperty => "previousVocab",
            Self::Property | Self::GeoProperty | Self::ListProperty => "previousValue",
        }
    }

    /// Returns (plural_key, singular_key) for temporal values format.
    pub fn temporal_values_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Relationship | Self::ListRelationship => ("objects", "object"),
            Self::LanguageProperty => ("languageMaps", "languageMap"),
            Self::JsonProperty => ("jsons", "json"),
            Self::VocabProperty => ("vocabs", "vocab"),
            Self::Property | Self::GeoProperty | Self::ListProperty => ("values", "value"),
        }
    }

    /// Parse strings ending with "/Property", "/Relationship", etc. (for discovery).
    pub fn from_iri_suffix(s: &str) -> Option<AttributeType> {
        for variant in Self::ALL {
            let name = variant.to_string();
            if s.ends_with(&format!("/{name}")) || s.ends_with(&format!("#{name}")) {
                return Some(variant);
            }
        }
        None
    }

    /// Returns the reserved compacted field names for this attribute type.
    pub fn reserved_fields(self) -> &'static [&'static str] {
        match self {
            Self::Property => &["value", "unitCode", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::Relationship => &["object", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::GeoProperty => &["value", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::VocabProperty => &["vocab", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::LanguageProperty => &["languageMap", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::JsonProperty => &["json", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::ListProperty => &["valueList", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
            Self::ListRelationship => &["objectList", "observedAt", "datasetId", "instanceId", "createdAt", "modifiedAt", "name"],
        }
    }

    const ALL: [AttributeType; 8] = [
        Self::Property,
        Self::Relationship,
        Self::GeoProperty,
        Self::VocabProperty,
        Self::LanguageProperty,
        Self::JsonProperty,
        Self::ListProperty,
        Self::ListRelationship,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn display_and_parse_round_trip() {
        for variant in AttributeType::ALL {
            let s = variant.to_string();
            let parsed = AttributeType::from_str(&s).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn iri_matches_display_name() {
        for variant in AttributeType::ALL {
            let iri_str = variant.iri().as_str();
            assert!(iri_str.ends_with(&variant.to_string()), "IRI {iri_str} should end with {variant}");
        }
    }

    #[test]
    fn from_iri_suffix_slash() {
        assert_eq!(
            AttributeType::from_iri_suffix("https://uri.etsi.org/ngsi-ld/Property"),
            Some(AttributeType::Property)
        );
    }

    #[test]
    fn from_iri_suffix_hash() {
        assert_eq!(
            AttributeType::from_iri_suffix("https://example.com#Relationship"),
            Some(AttributeType::Relationship)
        );
    }

    #[test]
    fn from_iri_suffix_unknown() {
        assert_eq!(AttributeType::from_iri_suffix("Unknown"), None);
    }

    #[test]
    fn value_key_mapping() {
        assert_eq!(AttributeType::Property.value_key(), iri::HAS_VALUE);
        assert_eq!(AttributeType::Relationship.value_key(), iri::HAS_OBJECT);
        assert_eq!(AttributeType::VocabProperty.value_key(), iri::HAS_VOCAB);
        assert_eq!(AttributeType::LanguageProperty.value_key(), iri::HAS_LANGUAGE_MAP);
        assert_eq!(AttributeType::JsonProperty.value_key(), iri::HAS_JSON);
    }

    #[test]
    fn serde_round_trip() {
        for variant in AttributeType::ALL {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AttributeType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }
}
