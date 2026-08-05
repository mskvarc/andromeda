//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use andromeda_ngsi_ld_vocabulary::{
    compact::properties as compact_properties,
    expanded::{classes as expanded_classes, properties as expanded_properties},
};
use iri_rs::Iri;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The eight NGSI-LD attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, strum::IntoStaticStr)]
pub enum AttributeType {
    /// A Property (clause 5.2.5).
    Property,
    /// A Relationship (clause 5.2.6).
    Relationship,
    /// A GeoProperty (clause 5.2.7).
    GeoProperty,
    /// A VocabProperty (clause 5.2.35).
    VocabProperty,
    /// A LanguageProperty (clause 5.2.32).
    LanguageProperty,
    /// A JsonProperty (clause 5.2.38).
    JsonProperty,
    /// A ListProperty (clause 5.2.36).
    ListProperty,
    /// A ListRelationship (clause 5.2.37).
    ListRelationship,
}

// `derive_more::AsRef` forwards to a struct field, and a fieldless enum has
// none; this forwards to the `IntoStaticStr` derive instead, so the rendering
// stays the one place `strum` already owns.
impl AsRef<str> for AttributeType {
    fn as_ref(&self) -> &str {
        <&'static str>::from(*self)
    }
}

impl AttributeType {
    /// Returns the expanded IRI for this attribute type as an [`Iri`].
    pub const fn iri(self) -> Iri<&'static str> {
        match self {
            Self::Property => expanded_classes::PROPERTY,
            Self::Relationship => expanded_classes::RELATIONSHIP,
            Self::GeoProperty => expanded_classes::GEO_PROPERTY,
            Self::VocabProperty => expanded_classes::VOCAB_PROPERTY,
            Self::LanguageProperty => expanded_classes::LANGUAGE_PROPERTY,
            Self::JsonProperty => expanded_classes::JSON_PROPERTY,
            Self::ListProperty => expanded_classes::LIST_PROPERTY,
            Self::ListRelationship => expanded_classes::LIST_RELATIONSHIP,
        }
    }

    /// Returns the expanded IRI for this attribute type as a static string slice.
    pub fn iri_str(self) -> &'static str {
        self.iri().into_inner()
    }

    /// Returns the expanded IRI of the value container for this attribute type
    /// as an [`Iri`].
    pub const fn value_key(self) -> Iri<&'static str> {
        match self {
            Self::Property | Self::GeoProperty => expanded_properties::VALUE,
            Self::Relationship => expanded_properties::OBJECT,
            Self::VocabProperty => expanded_properties::VOCAB,
            Self::LanguageProperty => expanded_properties::LANGUAGE_MAP,
            Self::JsonProperty => expanded_properties::JSON,
            Self::ListProperty => expanded_properties::VALUE_LIST,
            Self::ListRelationship => expanded_properties::OBJECT_LIST,
        }
    }

    /// Returns the expanded IRI of the value container as a static string slice.
    pub fn value_key_str(self) -> &'static str {
        self.value_key().into_inner()
    }

    /// Returns the compacted key of the value container for this attribute type.
    ///
    /// The same eight-way table as [`AttributeType::value_key`], spelled in the
    /// compacted terms clause 4.5.1 uses. A rendering works on compacted JSON
    /// and needs this one; an expansion or a store predicate needs the IRI.
    pub const fn compact_value_key(self) -> &'static str {
        match self {
            Self::Property | Self::GeoProperty => compact_properties::VALUE,
            Self::Relationship => compact_properties::OBJECT,
            Self::VocabProperty => compact_properties::VOCAB,
            Self::LanguageProperty => compact_properties::LANGUAGE_MAP,
            Self::JsonProperty => compact_properties::JSON,
            Self::ListProperty => compact_properties::VALUE_LIST,
            Self::ListRelationship => compact_properties::OBJECT_LIST,
        }
    }

    /// Returns the compacted previous-value key for showChanges support.
    pub const fn previous_key(self) -> &'static str {
        match self {
            Self::Relationship => compact_properties::PREVIOUS_OBJECT,
            Self::LanguageProperty => compact_properties::PREVIOUS_LANGUAGE_MAP,
            Self::JsonProperty => compact_properties::PREVIOUS_JSON,
            Self::VocabProperty => compact_properties::PREVIOUS_VOCAB,
            Self::ListRelationship => compact_properties::PREVIOUS_OBJECT_LIST,
            Self::ListProperty => compact_properties::PREVIOUS_VALUE_LIST,
            Self::Property | Self::GeoProperty => compact_properties::PREVIOUS_VALUE,
        }
    }

    /// Returns (plural_key, singular_key) for temporal values format.
    pub const fn temporal_values_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Relationship | Self::ListRelationship => (compact_properties::OBJECTS, compact_properties::OBJECT),
            Self::LanguageProperty => (compact_properties::LANGUAGE_MAPS, compact_properties::LANGUAGE_MAP),
            Self::JsonProperty => (compact_properties::JSONS, compact_properties::JSON),
            Self::VocabProperty => (compact_properties::VOCABS, compact_properties::VOCAB),
            Self::Property | Self::GeoProperty | Self::ListProperty => (compact_properties::VALUES, compact_properties::VALUE),
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
                compact_properties::VALUE,
                compact_properties::PREVIOUS_VALUE,
                compact_properties::UNIT_CODE,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::Relationship => &[
                compact_properties::OBJECT,
                compact_properties::PREVIOUS_OBJECT,
                compact_properties::OBJECT_TYPE,
                compact_properties::ENTITY,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::GeoProperty => &[
                compact_properties::VALUE,
                compact_properties::PREVIOUS_VALUE,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::VocabProperty => &[
                compact_properties::VOCAB,
                compact_properties::PREVIOUS_VOCAB,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::LanguageProperty => &[
                compact_properties::LANGUAGE_MAP,
                compact_properties::PREVIOUS_LANGUAGE_MAP,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::JsonProperty => &[
                compact_properties::JSON,
                compact_properties::PREVIOUS_JSON,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::ListProperty => &[
                compact_properties::VALUE_LIST,
                compact_properties::PREVIOUS_VALUE_LIST,
                compact_properties::VALUE_TYPE,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
                "name",
            ],
            Self::ListRelationship => &[
                compact_properties::OBJECT_LIST,
                compact_properties::PREVIOUS_OBJECT_LIST,
                compact_properties::OBJECT_TYPE,
                compact_properties::ENTITY_LIST,
                compact_properties::OBSERVED_AT,
                compact_properties::EXPIRES_AT,
                compact_properties::DATASET_ID,
                compact_properties::INSTANCE_ID,
                compact_properties::CREATED_AT,
                compact_properties::MODIFIED_AT,
                compact_properties::DELETED_AT,
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
    fn the_expanded_and_compacted_value_keys_group_the_variants_the_same_way() {
        // The two are not textually related -- `value` expands to `hasValue` --
        // so what can be checked is that they say the same thing about which
        // attribute types share a value member. Today only `Property` and
        // `GeoProperty` do; a variant added to one table under a different
        // grouping than the other fails here rather than silently rendering
        // one attribute type as another.
        let by_expanded: Vec<Vec<AttributeType>> = grouped(AttributeType::value_key_str);
        let by_compact: Vec<Vec<AttributeType>> = grouped(AttributeType::compact_value_key);
        pretty_assertions::assert_eq!(by_expanded, by_compact);
    }

    /// The variants grouped by the key the given reading gives them.
    fn grouped(key_of: fn(AttributeType) -> &'static str) -> Vec<Vec<AttributeType>> {
        let mut groups: Vec<(&'static str, Vec<AttributeType>)> = Vec::new();
        for variant in AttributeType::ALL {
            let key = key_of(variant);
            match groups.iter_mut().find(|(seen, _)| *seen == key) {
                Some((_, members)) => members.push(variant),
                None => groups.push((key, vec![variant])),
            }
        }
        groups.into_iter().map(|(_, members)| members).collect()
    }

    #[test]
    fn every_compacted_value_key_is_a_bare_term_not_an_iri() {
        for variant in AttributeType::ALL {
            let compact = variant.compact_value_key();
            assert!(!compact.is_empty(), "compact_value_key for {variant} is empty");
            assert!(
                !compact.contains(':') && !compact.contains('/'),
                "compact_value_key for {variant} should be a compacted term, got {compact}"
            );
        }
    }

    #[test]
    fn value_key_mapping() {
        pretty_assertions::assert_eq!(AttributeType::Property.value_key(), expanded_properties::VALUE);
        pretty_assertions::assert_eq!(AttributeType::Relationship.value_key(), expanded_properties::OBJECT);
        pretty_assertions::assert_eq!(AttributeType::VocabProperty.value_key(), expanded_properties::VOCAB);
        pretty_assertions::assert_eq!(AttributeType::LanguageProperty.value_key(), expanded_properties::LANGUAGE_MAP);
        pretty_assertions::assert_eq!(AttributeType::JsonProperty.value_key(), expanded_properties::JSON);
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
