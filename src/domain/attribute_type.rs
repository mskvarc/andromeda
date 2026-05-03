//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use crate::jsonld::vocab;
use iri_rs::Iri;
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
    /// Returns the expanded IRI for this attribute type as an [`Iri`].
    pub fn iri(self) -> Iri<&'static str> {
        match self {
            Self::Property => vocab::expanded::PROPERTY,
            Self::Relationship => vocab::expanded::RELATIONSHIP,
            Self::GeoProperty => vocab::expanded::GEO_PROPERTY,
            Self::VocabProperty => vocab::expanded::VOCAB_PROPERTY,
            Self::LanguageProperty => vocab::expanded::LANGUAGE_PROPERTY,
            Self::JsonProperty => vocab::expanded::JSON_PROPERTY,
            Self::ListProperty => vocab::expanded::LIST_PROPERTY,
            Self::ListRelationship => vocab::expanded::LIST_RELATIONSHIP,
        }
    }

    /// Returns the expanded IRI for this attribute type as a static string slice.
    pub fn iri_str(self) -> &'static str {
        self.iri().into_inner()
    }

    /// Returns the expanded IRI of the value container for this attribute type
    /// as an [`Iri`].
    pub fn value_key(self) -> Iri<&'static str> {
        match self {
            Self::Property | Self::GeoProperty => vocab::expanded::VALUE,
            Self::Relationship => vocab::expanded::OBJECT,
            Self::VocabProperty => vocab::expanded::VOCAB,
            Self::LanguageProperty => vocab::expanded::LANGUAGE_MAP,
            Self::JsonProperty => vocab::expanded::JSON,
            Self::ListProperty => vocab::expanded::VALUE_LIST,
            Self::ListRelationship => vocab::expanded::OBJECT_LIST,
        }
    }

    /// Returns the expanded IRI of the value container as a static string slice.
    pub fn value_key_str(self) -> &'static str {
        self.value_key().into_inner()
    }

    /// Returns the compacted (short) value key for this attribute type.
    #[allow(dead_code)]
    pub const fn compacted_value_key(self) -> &'static str {
        match self {
            Self::Property | Self::GeoProperty => vocab::compact::VALUE,
            Self::Relationship => vocab::compact::OBJECT,
            Self::VocabProperty => vocab::compact::VOCAB,
            Self::LanguageProperty => vocab::compact::LANGUAGE_MAP,
            Self::JsonProperty => vocab::compact::JSON,
            Self::ListProperty => vocab::compact::VALUE_LIST,
            Self::ListRelationship => vocab::compact::OBJECT_LIST,
        }
    }

    /// Returns the compacted previous-value key for showChanges support.
    pub const fn previous_key(self) -> &'static str {
        match self {
            Self::Relationship => vocab::compact::PREVIOUS_OBJECT,
            Self::LanguageProperty => vocab::compact::PREVIOUS_LANGUAGE_MAP,
            Self::JsonProperty => vocab::compact::PREVIOUS_JSON,
            Self::VocabProperty => vocab::compact::PREVIOUS_VOCAB,
            Self::ListRelationship => vocab::compact::PREVIOUS_OBJECT_LIST,
            Self::ListProperty => vocab::compact::PREVIOUS_VALUE_LIST,
            Self::Property | Self::GeoProperty => vocab::compact::PREVIOUS_VALUE,
        }
    }

    /// Returns (plural_key, singular_key) for temporal values format.
    pub const fn temporal_values_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Relationship | Self::ListRelationship => (vocab::compact::OBJECTS, vocab::compact::OBJECT),
            Self::LanguageProperty => (vocab::compact::LANGUAGE_MAPS, vocab::compact::LANGUAGE_MAP),
            Self::JsonProperty => (vocab::compact::JSONS, vocab::compact::JSON),
            Self::VocabProperty => (vocab::compact::VOCABS, vocab::compact::VOCAB),
            Self::Property | Self::GeoProperty | Self::ListProperty => (vocab::compact::VALUES, vocab::compact::VALUE),
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
    pub const fn reserved_fields(self) -> &'static [&'static str] {
        match self {
            Self::Property => &[
                vocab::compact::VALUE,
                vocab::compact::PREVIOUS_VALUE,
                vocab::compact::UNIT_CODE,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::Relationship => &[
                vocab::compact::OBJECT,
                vocab::compact::PREVIOUS_OBJECT,
                vocab::compact::OBJECT_TYPE,
                vocab::compact::ENTITY,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::GeoProperty => &[
                vocab::compact::VALUE,
                vocab::compact::PREVIOUS_VALUE,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::VocabProperty => &[
                vocab::compact::VOCAB,
                vocab::compact::PREVIOUS_VOCAB,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::LanguageProperty => &[
                vocab::compact::LANGUAGE_MAP,
                vocab::compact::PREVIOUS_LANGUAGE_MAP,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::JsonProperty => &[
                vocab::compact::JSON,
                vocab::compact::PREVIOUS_JSON,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::ListProperty => &[
                vocab::compact::VALUE_LIST,
                vocab::compact::PREVIOUS_VALUE_LIST,
                vocab::compact::VALUE_TYPE,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
            Self::ListRelationship => &[
                vocab::compact::OBJECT_LIST,
                vocab::compact::PREVIOUS_OBJECT_LIST,
                vocab::compact::OBJECT_TYPE,
                vocab::compact::ENTITY_LIST,
                vocab::compact::OBSERVED_AT,
                vocab::compact::EXPIRES_AT,
                vocab::compact::DATASET_ID,
                vocab::compact::INSTANCE_ID,
                vocab::compact::CREATED_AT,
                vocab::compact::MODIFIED_AT,
                vocab::compact::DELETED_AT,
                "name",
            ],
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
        Self::ALL.iter().find(|v| v.iri().as_str() == iri).copied()
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
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::str::FromStr;

    #[test]
    fn display_and_parse_round_trip() {
        for variant in AttributeType::ALL {
            let s = variant.to_string();
            let parsed = AttributeType::from_str(&s).unwrap();
            pretty_assertions::assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn iri_matches_display_name() {
        for variant in AttributeType::ALL {
            let iri = variant.iri();
            assert!(iri.as_str().ends_with(&variant.to_string()), "IRI {iri} should end with {variant}");
        }
    }

    #[test]
    fn from_iri_suffix_slash() {
        pretty_assertions::assert_eq!(
            AttributeType::from_iri_suffix("https://uri.etsi.org/ngsi-ld/Property"),
            Some(AttributeType::Property)
        );
    }

    #[test]
    fn from_iri_suffix_hash() {
        pretty_assertions::assert_eq!(
            AttributeType::from_iri_suffix("https://example.com#Relationship"),
            Some(AttributeType::Relationship)
        );
    }

    #[test]
    fn from_iri_suffix_unknown() {
        pretty_assertions::assert_eq!(AttributeType::from_iri_suffix("Unknown"), None);
    }

    #[test]
    fn attribute_type_iri_roundtrip() {
        for variant in AttributeType::ALL {
            let iri = variant.iri_str();
            let parsed = AttributeType::from_expanded_iri(iri);
            pretty_assertions::assert_eq!(parsed, Some(variant), "roundtrip failed for {variant}");
        }
    }

    #[test]
    fn value_key_roundtrip() {
        for variant in AttributeType::ALL {
            let key = variant.value_key();
            assert!(!key.is_empty(), "value_key_str for {variant} is empty");
            assert!(
                key.as_str().starts_with("https://"),
                "value_key_str for {variant} should be an expanded IRI, got {key}"
            );
        }
    }

    #[test]
    fn value_key_mapping() {
        pretty_assertions::assert_eq!(AttributeType::Property.value_key(), vocab::expanded::VALUE);
        pretty_assertions::assert_eq!(AttributeType::Relationship.value_key(), vocab::expanded::OBJECT);
        pretty_assertions::assert_eq!(AttributeType::VocabProperty.value_key(), vocab::expanded::VOCAB);
        pretty_assertions::assert_eq!(AttributeType::LanguageProperty.value_key(), vocab::expanded::LANGUAGE_MAP);
        pretty_assertions::assert_eq!(AttributeType::JsonProperty.value_key(), vocab::expanded::JSON);
    }

    #[test]
    fn reserved_fields_includes_expires_at_for_all_types() {
        for variant in AttributeType::ALL {
            assert!(
                variant.reserved_fields().contains(&"expiresAt"),
                "reserved_fields for {variant} should include expiresAt"
            );
        }
    }

    #[test]
    fn serde_round_trip() {
        for variant in AttributeType::ALL {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: AttributeType = serde_json::from_str(&json).unwrap();
            pretty_assertions::assert_eq!(variant, parsed);
        }
    }
}
