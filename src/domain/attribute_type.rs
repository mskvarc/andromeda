//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use crate::jsonld::vocab;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The eight NGSI-LD attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, strum::IntoStaticStr)]
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
    /// Returns the expanded IRI string for this attribute type.
    pub fn iri_str(self) -> &'static str {
        match self {
            Self::Property => vocab::PROPERTY,
            Self::Relationship => vocab::RELATIONSHIP,
            Self::GeoProperty => vocab::GEO_PROPERTY,
            Self::VocabProperty => vocab::VOCAB_PROPERTY,
            Self::LanguageProperty => vocab::LANGUAGE_PROPERTY,
            Self::JsonProperty => vocab::JSON_PROPERTY,
            Self::ListProperty => vocab::LIST_PROPERTY,
            Self::ListRelationship => vocab::LIST_RELATIONSHIP,
        }
    }

    /// Returns the expanded IRI string of the value container for this attribute type.
    pub fn value_key_str(self) -> &'static str {
        match self {
            Self::Property | Self::GeoProperty => vocab::HAS_VALUE,
            Self::Relationship => vocab::HAS_OBJECT,
            Self::VocabProperty => vocab::HAS_VOCAB,
            Self::LanguageProperty => vocab::HAS_LANGUAGE_MAP,
            Self::JsonProperty => vocab::HAS_JSON,
            Self::ListProperty => vocab::HAS_VALUE_LIST,
            Self::ListRelationship => vocab::HAS_OBJECT_LIST,
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

    /// Returns the short (compacted) name for this attribute type as a static string.
    ///
    /// Equivalent to `Display` / `IntoStaticStr` output (e.g. `"Property"`).
    pub fn short_name(self) -> &'static str {
        self.into()
    }

    /// Try to parse an expanded IRI string into an `AttributeType`.
    pub fn from_expanded_iri(iri: &str) -> Option<AttributeType> {
        Self::ALL.iter().find(|v| v.iri_str() == iri).copied()
    }

    /// All eight NGSI-LD attribute type variants.
    pub const ALL: [AttributeType; 8] = [
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
            let iri = variant.iri_str();
            assert!(iri.ends_with(&variant.to_string()), "IRI {iri} should end with {variant}");
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
    fn attribute_type_iri_roundtrip() {
        for variant in AttributeType::ALL {
            let iri = variant.iri_str();
            let parsed = AttributeType::from_expanded_iri(iri);
            assert_eq!(parsed, Some(variant), "roundtrip failed for {variant}");
        }
    }

    #[test]
    fn value_key_roundtrip() {
        for variant in AttributeType::ALL {
            let key = variant.value_key_str();
            assert!(!key.is_empty(), "value_key_str for {variant} is empty");
            // Verify it's a valid expanded IRI
            assert!(key.starts_with("https://"), "value_key_str for {variant} should be an expanded IRI, got {key}");
        }
    }

    #[test]
    fn value_key_mapping() {
        assert_eq!(AttributeType::Property.value_key_str(), vocab::HAS_VALUE);
        assert_eq!(AttributeType::Relationship.value_key_str(), vocab::HAS_OBJECT);
        assert_eq!(AttributeType::VocabProperty.value_key_str(), vocab::HAS_VOCAB);
        assert_eq!(AttributeType::LanguageProperty.value_key_str(), vocab::HAS_LANGUAGE_MAP);
        assert_eq!(AttributeType::JsonProperty.value_key_str(), vocab::HAS_JSON);
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
