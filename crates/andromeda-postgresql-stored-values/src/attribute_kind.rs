//! The persisted attribute family of one history row.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The NGSI-LD attribute family a history row was written as (clause 5.2.4).
    ///
    /// The kind is stored on every row rather than once per stream because a
    /// historical row must not be reinterpreted through a mutable catalogue: an
    /// attribute whose type changes does not retroactively change the shape of
    /// what was already recorded. It is present on tombstones too, since
    /// synthesising `urn:ngsi-ld:null` for a deletion requires the member shape
    /// the family dictates (a value for a `Property`, an object for a
    /// `Relationship`, a language map for a `LanguageProperty`; clauses 4.5.7 and
    /// 4.5.8). That shape is unknowable without the family.
    ///
    /// The labels are the specification's own type tokens, which is the casing
    /// clause 5.2.4 mandates on the wire and therefore the only spelling that
    /// stays meaningful in a stored registry.
    AttributeKind {
        frozen_at: (1, 0, 0),
        frozen_prefix: 8,
        frozen_digest: 0x7bfe_1036_5abf_8069,
        codes: {
            /// A `Property` (clause 5.2.5).
            Property = 1, "Property";
            /// A `Relationship` (clause 5.2.6).
            Relationship = 2, "Relationship";
            /// A `GeoProperty` (clause 5.2.7).
            GeoProperty = 3, "GeoProperty";
            /// A `VocabProperty` (clause 5.2.35).
            VocabProperty = 4, "VocabProperty";
            /// A `LanguageProperty` (clause 5.2.32).
            LanguageProperty = 5, "LanguageProperty";
            /// A `JsonProperty` (clause 5.2.38).
            JsonProperty = 6, "JsonProperty";
            /// A `ListProperty` (clause 5.2.36).
            ListProperty = 7, "ListProperty";
            /// A `ListRelationship` (clause 5.2.37).
            ListRelationship = 8, "ListRelationship";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_kind::AttributeKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_registry_covers_every_attribute_family_the_specification_defines() {
        assert_eq!(AttributeKind::ALL.len(), 8);
    }

    #[test]
    fn the_families_are_numbered_as_the_migration_writes_them() {
        assert_eq!(AttributeKind::Property.code(), 1);
        assert_eq!(AttributeKind::ListRelationship.code(), 8);
    }

    #[test]
    fn zero_is_not_an_attribute_family() {
        assert!(AttributeKind::try_from(0).is_err());
    }

    #[test]
    fn a_family_a_newer_release_might_add_fails_to_decode() {
        assert!(AttributeKind::try_from(9).is_err());
    }
}
