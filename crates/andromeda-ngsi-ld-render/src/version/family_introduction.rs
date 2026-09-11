//! The NGSI-LD version each attribute family was introduced at (Table
//! 4.3.6.8-1, "Version Introduced").

use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};

/// The NGSI-LD version `family` was introduced at (Table 4.3.6.8-1, "Version
/// Introduced").
///
/// A target older than the version a family was introduced at is a target that
/// never defined the family, so Table 4.3.6.8-1's "Conformant Data Fallback"
/// reformats it as the generalization it specializes: every Property subclass
/// as a Property, and a `ListRelationship` as a Relationship.
pub(crate) const fn family_introduced(family: AttributeType) -> SpecVersion {
    match family {
        AttributeType::Property | AttributeType::GeoProperty | AttributeType::Relationship => SpecVersion::V1_0,
        AttributeType::LanguageProperty => SpecVersion::V1_4,
        AttributeType::VocabProperty | AttributeType::JsonProperty | AttributeType::ListProperty | AttributeType::ListRelationship => SpecVersion::V1_8,
    }
}

#[cfg(test)]
mod tests {
    use crate::version::family_introduction::family_introduced;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};

    /// Table 4.3.6.8-1's "Version Introduced" column, row by row: `Property`,
    /// `GeoProperty` and `Relationship` at 1.0, the `LanguageProperty` at 1.4,
    /// and the `VocabProperty`, `JsonProperty`, `ListProperty` and
    /// `ListRelationship` at 1.8.
    #[test]
    fn each_family_names_the_version_table_4_3_6_8_1_introduced_it_at() {
        pretty_assertions::assert_eq!(family_introduced(AttributeType::Property), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::GeoProperty), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::Relationship), SpecVersion::V1_0);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::LanguageProperty), SpecVersion::V1_4);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::VocabProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::JsonProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::ListProperty), SpecVersion::V1_8);
        pretty_assertions::assert_eq!(family_introduced(AttributeType::ListRelationship), SpecVersion::V1_8);
    }
}
