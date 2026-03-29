//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use crate::iri;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

impl fmt::Display for AttributeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AttributeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Property" | "https://uri.etsi.org/ngsi-ld/Property" => Ok(Self::Property),
            "Relationship" | "https://uri.etsi.org/ngsi-ld/Relationship" => Ok(Self::Relationship),
            "GeoProperty" | "https://uri.etsi.org/ngsi-ld/GeoProperty" => Ok(Self::GeoProperty),
            "VocabProperty" | "https://uri.etsi.org/ngsi-ld/VocabProperty" => Ok(Self::VocabProperty),
            "LanguageProperty" | "https://uri.etsi.org/ngsi-ld/LanguageProperty" => Ok(Self::LanguageProperty),
            "JsonProperty" | "https://uri.etsi.org/ngsi-ld/JsonProperty" => Ok(Self::JsonProperty),
            "ListProperty" | "https://uri.etsi.org/ngsi-ld/ListProperty" => Ok(Self::ListProperty),
            "ListRelationship" | "https://uri.etsi.org/ngsi-ld/ListRelationship" => Ok(Self::ListRelationship),
            _ => Err(format!("unknown attribute type: {}", s)),
        }
    }
}

impl AttributeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Property => "Property",
            Self::Relationship => "Relationship",
            Self::GeoProperty => "GeoProperty",
            Self::VocabProperty => "VocabProperty",
            Self::LanguageProperty => "LanguageProperty",
            Self::JsonProperty => "JsonProperty",
            Self::ListProperty => "ListProperty",
            Self::ListRelationship => "ListRelationship",
        }
    }

    pub fn iri(self) -> &'static str {
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

    pub fn value_key(self) -> &'static str {
        match self {
            Self::Property | Self::GeoProperty | Self::ListProperty => iri::HAS_VALUE,
            Self::Relationship | Self::ListRelationship => iri::HAS_OBJECT,
            Self::VocabProperty => iri::HAS_VOCAB,
            Self::LanguageProperty => iri::HAS_LANGUAGE_MAP,
            Self::JsonProperty => iri::HAS_JSON,
        }
    }

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

    pub fn previous_key(self) -> &'static str {
        match self {
            Self::Relationship | Self::ListRelationship => "previousObject",
            Self::LanguageProperty => "previousLanguageMap",
            Self::JsonProperty => "previousJson",
            Self::VocabProperty => "previousVocab",
            _ => "previousValue",
        }
    }

    /// Returns (plural_key, singular_key) for temporal values format.
    pub fn temporal_values_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Relationship | Self::ListRelationship => ("objects", "object"),
            Self::LanguageProperty => ("languageMaps", "languageMap"),
            Self::JsonProperty => ("jsons", "json"),
            Self::VocabProperty => ("vocabs", "vocab"),
            _ => ("values", "value"),
        }
    }

    /// Parse strings ending with "/Property", "/Relationship", etc. (for discovery).
    pub fn from_iri_suffix(s: &str) -> Option<Self> {
        for variant in Self::ALL {
            let suffix = format!("/{}", variant.as_str());
            let hash_suffix = format!("#{}", variant.as_str());
            if s.ends_with(&suffix) || s.ends_with(&hash_suffix) {
                return Some(variant);
            }
        }
        None
    }

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

    const ALL: [Self; 8] = [
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

    #[test]
    fn display_roundtrip() {
        for variant in AttributeType::ALL {
            let s = variant.to_string();
            let parsed: AttributeType = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn from_iri_roundtrip() {
        for variant in AttributeType::ALL {
            let parsed: AttributeType = variant.iri().parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn from_iri_suffix() {
        assert_eq!(
            AttributeType::from_iri_suffix("https://uri.etsi.org/ngsi-ld/Property"),
            Some(AttributeType::Property)
        );
        assert_eq!(
            AttributeType::from_iri_suffix("https://example.com#Relationship"),
            Some(AttributeType::Relationship)
        );
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
    fn serde_roundtrip() {
        for variant in AttributeType::ALL {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AttributeType = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }
}
