//! NGSI-LD Attribute type enum (ETSI GS CIM 009 clause 5.2.4).

use andromeda_ngsi_ld_vocabulary::{
    compact::properties as compact_properties,
    expanded::{classes as expanded_classes, properties as expanded_properties},
};
use iri_rs::Iri;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How the simplified representations carry one attribute value.
///
/// Clause 4.5.4 writes the simplified value of a `LanguageProperty`, a
/// `VocabProperty` and a `JsonProperty` as a one-member object naming its
/// value member, and every other family's value bare; clause 4.5.9 carries the
/// first element of each simplified temporal `[value, Temporal Property]` pair
/// the same way. Which of the two a family takes is fixed by the clauses rather
/// than by the value at hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimplifiedValueEncoding {
    /// The value stands alone: a Property or `GeoProperty` value, a
    /// Relationship object, and the ordered arrays of the two list families.
    Bare,
    /// The value is wrapped in a one-member object keyed by the family's value
    /// member, which clauses 4.5.4 and 4.5.9 require of `languageMap`, `json` and
    /// `vocab`.
    Wrapped,
}

/// The eight NGSI-LD attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, strum::IntoStaticStr)]
pub enum AttributeType {
    /// A Property (clause 5.2.5).
    Property,
    /// A Relationship (clause 5.2.6).
    Relationship,
    /// A `GeoProperty` (clause 5.2.7).
    GeoProperty,
    /// A `VocabProperty` (clause 5.2.35).
    VocabProperty,
    /// A `LanguageProperty` (clause 5.2.32).
    LanguageProperty,
    /// A `JsonProperty` (clause 5.2.38).
    JsonProperty,
    /// A `ListProperty` (clause 5.2.36).
    ListProperty,
    /// A `ListRelationship` (clause 5.2.37).
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

/// Expands to the reserved compacted member names of one attribute type: the
/// type-specific names passed in, followed by the members every attribute
/// instance reserves whatever its type — `observedAt` and `expiresAt` (clause
/// 4.5.1), `datasetId` and `instanceId` (clause 4.5.5), the system attributes
/// of clause 4.8, and the `name` member a simplified representation carries.
///
/// Written once and invoked once per [`AttributeType`] variant in
/// [`AttributeType::reserved_fields`], so the shared tail cannot drift between
/// the eight tables.
macro_rules! reserved_compact_members {
    ($($specific:expr),+ $(,)?) => {
        &[
            $($specific,)+
            compact_properties::OBSERVED_AT,
            compact_properties::EXPIRES_AT,
            compact_properties::DATASET_ID,
            compact_properties::INSTANCE_ID,
            compact_properties::CREATED_AT,
            compact_properties::MODIFIED_AT,
            compact_properties::DELETED_AT,
            "name",
        ]
    };
}

impl AttributeType {
    /// Returns the expanded IRI for this attribute type as an [`Iri`].
    #[must_use]
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
    #[must_use]
    pub fn iri_str(self) -> &'static str {
        self.iri().into_inner()
    }

    /// Returns the expanded IRI of the value container for this attribute type
    /// as an [`Iri`].
    #[must_use]
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
    #[must_use]
    pub fn value_key_str(self) -> &'static str {
        self.value_key().into_inner()
    }

    /// Returns the compacted key of the value container for this attribute type.
    ///
    /// The same eight-way table as [`AttributeType::value_key`], spelled in the
    /// compacted terms clause 4.5.1 uses. A rendering works on compacted JSON
    /// and needs this one; an expansion or a store predicate needs the IRI.
    #[must_use]
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
    #[must_use]
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

    /// Returns the compacted member the simplified temporal representation
    /// collapses this attribute type into (clause 4.5.9).
    ///
    /// Clause 4.5.9 gives every family exactly one member, whose value is an
    /// array of two-element `[value, Temporal Property]` arrays: `values` for a
    /// Property or a `GeoProperty`, `objects` for a Relationship,
    /// `languageMaps`, `jsons` and `vocabs` for their own families, and
    /// `valueLists` / `objectLists` for the two list families — never the
    /// generalized `values` / `objects` those specialize, since clauses
    /// 4.5.21.2 and 4.5.22.2 prohibit the generalization on them.
    ///
    /// The value inside each pair is read from
    /// [`AttributeType::compact_value_key`] and carried as
    /// [`AttributeType::simplified_value_encoding`] prescribes.
    #[must_use]
    pub const fn temporal_values_key(self) -> &'static str {
        match self {
            Self::Relationship => compact_properties::OBJECTS,
            Self::ListRelationship => compact_properties::OBJECT_LISTS,
            Self::LanguageProperty => compact_properties::LANGUAGE_MAPS,
            Self::JsonProperty => compact_properties::JSONS,
            Self::VocabProperty => compact_properties::VOCABS,
            Self::ListProperty => compact_properties::VALUE_LISTS,
            Self::Property | Self::GeoProperty => compact_properties::VALUES,
        }
    }

    /// Returns the expanded IRI of the clause 4.5.9 temporal-values member.
    ///
    /// The same eight-way table as [`AttributeType::temporal_values_key`],
    /// spelled as IRIs. The temporal write path stores expanded documents and
    /// needs this one; the renderer works on compacted JSON and needs the other.
    /// Keeping both here is what stops the read and write sides of the temporal
    /// store from disagreeing on which member a list family is held under.
    #[must_use]
    pub const fn expanded_temporal_values_key(self) -> Iri<&'static str> {
        match self {
            Self::Relationship => expanded_properties::OBJECTS,
            Self::ListRelationship => expanded_properties::OBJECT_LISTS,
            Self::LanguageProperty => expanded_properties::LANGUAGE_MAPS,
            Self::JsonProperty => expanded_properties::JSONS,
            Self::VocabProperty => expanded_properties::VOCABS,
            Self::ListProperty => expanded_properties::VALUE_LISTS,
            Self::Property | Self::GeoProperty => expanded_properties::VALUES,
        }
    }

    /// Returns the expanded temporal-values member as a static string slice.
    #[must_use]
    pub fn expanded_temporal_values_key_str(self) -> &'static str {
        self.expanded_temporal_values_key().into_inner()
    }

    /// Returns how the simplified representations of clauses 4.5.4 and 4.5.9
    /// carry this attribute type's value.
    #[must_use]
    pub const fn simplified_value_encoding(self) -> SimplifiedValueEncoding {
        match self {
            Self::LanguageProperty | Self::JsonProperty | Self::VocabProperty => SimplifiedValueEncoding::Wrapped,
            Self::Property | Self::GeoProperty | Self::Relationship | Self::ListProperty | Self::ListRelationship => SimplifiedValueEncoding::Bare,
        }
    }

    /// Returns the reserved compacted field names for this attribute type.
    #[must_use]
    pub const fn reserved_fields(self) -> &'static [&'static str] {
        match self {
            // A `GeoProperty` reserves exactly what a Property reserves.
            // Clause 4.7.3 closes with "an NGSI-LD GeoProperty which includes
            // additional Properties or Relationships shall be treated in the
            // same manner as an ordinary NGSI-LD Property", which is also why
            // both families read their value out of the same `value` member in
            // `AttributeType::value_key`. Table 5.2.7-1 does not restate the
            // `unitCode` of clause 4.5.2.2; clause 4.7.3 is what carries it
            // over.
            Self::Property | Self::GeoProperty => reserved_compact_members![
                compact_properties::VALUE,
                compact_properties::VALUES,
                compact_properties::PREVIOUS_VALUE,
                compact_properties::UNIT_CODE,
                compact_properties::VALUE_TYPE,
            ],
            Self::Relationship => reserved_compact_members![
                compact_properties::OBJECT,
                compact_properties::OBJECTS,
                compact_properties::PREVIOUS_OBJECT,
                compact_properties::OBJECT_TYPE,
                compact_properties::ENTITY,
            ],
            Self::VocabProperty => reserved_compact_members![
                compact_properties::VOCAB,
                compact_properties::VOCABS,
                compact_properties::PREVIOUS_VOCAB,
                compact_properties::VALUE_TYPE,
            ],
            Self::LanguageProperty => reserved_compact_members![
                compact_properties::LANGUAGE_MAP,
                compact_properties::LANGUAGE_MAPS,
                compact_properties::PREVIOUS_LANGUAGE_MAP,
                compact_properties::VALUE_TYPE,
            ],
            Self::JsonProperty => reserved_compact_members![
                compact_properties::JSON,
                compact_properties::JSONS,
                compact_properties::PREVIOUS_JSON,
                compact_properties::VALUE_TYPE,
            ],
            // The plural member is this family's own, `valueLists`, not the
            // generalized `values`: clause 4.5.21.2 prohibits the generalization
            // outright, and clause 4.5.9 gives the family `valueLists`.
            //
            // `unitCode` is inherited. Clause 4.5.21.2 makes a normalized
            // ListProperty "the same as the JSON-LD object in NGSI-LD Property
            // Representation defined in clause 4.5.2.2, with the following
            // differences", and the differences replace `value` with
            // `valueList` without touching the `unitCode` clause 4.5.2.2 lists
            // as Optional -- unlike the four unitless families, whose own
            // clauses each name `unitCode` under Prohibited. Table 5.2.36-1
            // does not restate it; clause 4.5.21.2 is what carries it over.
            Self::ListProperty => reserved_compact_members![
                compact_properties::VALUE_LIST,
                compact_properties::VALUE_LISTS,
                compact_properties::PREVIOUS_VALUE_LIST,
                compact_properties::UNIT_CODE,
                compact_properties::VALUE_TYPE,
            ],
            Self::ListRelationship => reserved_compact_members![
                compact_properties::OBJECT_LIST,
                compact_properties::OBJECT_LISTS,
                compact_properties::PREVIOUS_OBJECT_LIST,
                compact_properties::OBJECT_TYPE,
                compact_properties::ENTITY_LIST,
            ],
        }
    }

    /// Whether the family defines the compact member `member`, one of its
    /// [`AttributeType::reserved_fields`]: the clause that defines the family
    /// lists it, and a member of that name on an instance of the family is the
    /// family's own rather than a sub-attribute.
    #[must_use]
    pub fn defines_member(self, member: &str) -> bool {
        self.reserved_fields().contains(&member)
    }

    /// Returns the short (compacted) name for this attribute type as a static string.
    ///
    /// Equivalent to `Display` / `IntoStaticStr` output (e.g. `"Property"`).
    #[must_use]
    pub fn short_name(self) -> &'static str {
        self.into()
    }

    /// Try to parse an expanded IRI string into an `AttributeType`.
    #[must_use]
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

    /// Clause 4.5.9 names the member each family collapses into, and names
    /// `valueLists` / `objectLists` for the two list families rather than the
    /// `values` / `objects` they specialize.
    #[test]
    fn every_family_collapses_into_the_simplified_temporal_member_clause_4_5_9_names_it() {
        let expected = [
            (AttributeType::Property, "values"),
            (AttributeType::GeoProperty, "values"),
            (AttributeType::Relationship, "objects"),
            (AttributeType::LanguageProperty, "languageMaps"),
            (AttributeType::JsonProperty, "jsons"),
            (AttributeType::VocabProperty, "vocabs"),
            (AttributeType::ListProperty, "valueLists"),
            (AttributeType::ListRelationship, "objectLists"),
        ];
        for (variant, member) in expected {
            pretty_assertions::assert_eq!(variant.temporal_values_key(), member, "{variant}");
        }
    }

    /// Clause 4.5.9 pairs each simplified temporal member with the instance
    /// member the value is read from, which is the family's own value member:
    /// `valueList` for a `ListProperty`, `objectList` for a
    /// `ListRelationship`, never the generalization.
    #[test]
    fn the_simplified_temporal_member_reads_from_the_families_own_value_member() {
        for variant in AttributeType::ALL {
            let plural = variant.temporal_values_key();
            let singular = variant.compact_value_key();
            assert!(
                plural.starts_with(singular),
                "clause 4.5.9 pairs {variant}'s '{plural}' with its own value member, but that is '{singular}'"
            );
        }
    }

    /// Clauses 4.5.4 and 4.5.9 wrap a `languageMap`, a `json` and a `vocab`
    /// in a one-member object and leave every other family's value bare.
    #[test]
    fn only_the_three_wrapped_families_name_their_simplified_value() {
        let wrapped = [AttributeType::LanguageProperty, AttributeType::JsonProperty, AttributeType::VocabProperty];
        for variant in AttributeType::ALL {
            let expected = if wrapped.contains(&variant) {
                SimplifiedValueEncoding::Wrapped
            } else {
                SimplifiedValueEncoding::Bare
            };
            pretty_assertions::assert_eq!(variant.simplified_value_encoding(), expected, "{variant}");
        }
    }

    /// Only the Relationship families define `objectType` and the inline
    /// target members; every other family defines `valueType` (clauses 4.5.2,
    /// 4.5.3, 4.5.18, 4.5.20, 4.5.21, 4.5.22, 4.5.24).
    #[test]
    fn the_relationship_families_alone_define_the_object_members() {
        for variant in AttributeType::ALL {
            let relationship = matches!(variant, AttributeType::Relationship | AttributeType::ListRelationship);
            pretty_assertions::assert_eq!(variant.defines_member(compact_properties::OBJECT_TYPE), relationship, "{variant}");
            pretty_assertions::assert_eq!(variant.defines_member(compact_properties::VALUE_TYPE), !relationship, "{variant}");
        }
        assert!(AttributeType::Relationship.defines_member(compact_properties::ENTITY));
        assert!(AttributeType::ListRelationship.defines_member(compact_properties::ENTITY_LIST));
        assert!(!AttributeType::Property.defines_member("speed"));
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
