//! The member of an attribute instance that carries its value.
//!
//! Each family reads its value out of one member (clause 4.5): `value` for a
//! Property and a `GeoProperty`, `object` for a Relationship, and one member of
//! its own for each of the five specialised families. A written fragment may
//! omit the family (a partial update of clause 5.6.4 need not restate it), so
//! the member is what says which value the fragment carries.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::Iri;
use strum::{EnumIter, IntoEnumIterator, IntoStaticStr};

/// A value-carrying member.
#[derive(Clone, Copy, Debug, EnumIter, Eq, Hash, IntoStaticStr, PartialEq)]
pub enum ValueCarrier {
    /// `hasValue`, shared by a Property and a `GeoProperty`.
    Value,
    /// `hasObject`.
    Object,
    /// `hasLanguageMap`.
    LanguageMap,
    /// `hasVocab`.
    Vocab,
    /// `hasJSON`.
    Json,
    /// `hasValueList`.
    ValueList,
    /// `hasObjectList`.
    ObjectList,
}

impl ValueCarrier {
    /// The carrier `family` reads its value from.
    #[must_use]
    pub const fn of(family: AttributeType) -> ValueCarrier {
        match family {
            AttributeType::Property | AttributeType::GeoProperty => ValueCarrier::Value,
            AttributeType::Relationship => ValueCarrier::Object,
            AttributeType::LanguageProperty => ValueCarrier::LanguageMap,
            AttributeType::VocabProperty => ValueCarrier::Vocab,
            AttributeType::JsonProperty => ValueCarrier::Json,
            AttributeType::ListProperty => ValueCarrier::ValueList,
            AttributeType::ListRelationship => ValueCarrier::ObjectList,
        }
    }

    /// The expanded IRI of the member.
    #[must_use]
    pub const fn iri(self) -> Iri<&'static str> {
        match self {
            ValueCarrier::Value => properties::VALUE,
            ValueCarrier::Object => properties::OBJECT,
            ValueCarrier::LanguageMap => properties::LANGUAGE_MAP,
            ValueCarrier::Vocab => properties::VOCAB,
            ValueCarrier::Json => properties::JSON,
            ValueCarrier::ValueList => properties::VALUE_LIST,
            ValueCarrier::ObjectList => properties::OBJECT_LIST,
        }
    }

    /// The carrier whose expanded IRI is `iri`.
    #[must_use]
    pub fn from_iri(iri: &str) -> Option<ValueCarrier> {
        ValueCarrier::iter().find(|carrier| carrier.iri().as_str() == iri)
    }

    /// The family a fragment declaring none is read as when it carries this
    /// member: the member names the family, except `hasValue`, which a
    /// Property and a `GeoProperty` share and clause 4.5.2 reads as a Property.
    #[must_use]
    pub const fn implied_family(self) -> AttributeType {
        match self {
            ValueCarrier::Value => AttributeType::Property,
            ValueCarrier::Object => AttributeType::Relationship,
            ValueCarrier::LanguageMap => AttributeType::LanguageProperty,
            ValueCarrier::Vocab => AttributeType::VocabProperty,
            ValueCarrier::Json => AttributeType::JsonProperty,
            ValueCarrier::ValueList => AttributeType::ListProperty,
            ValueCarrier::ObjectList => AttributeType::ListRelationship,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::value_carrier::ValueCarrier;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use strum::IntoEnumIterator;

    #[test]
    fn every_family_reads_the_carrier_its_value_key_names() {
        for family in AttributeType::ALL {
            pretty_assertions::assert_eq!(ValueCarrier::of(family).iri(), family.value_key(), "{family}");
        }
    }

    #[test]
    fn every_carrier_is_found_by_its_iri() {
        for carrier in ValueCarrier::iter() {
            pretty_assertions::assert_eq!(ValueCarrier::from_iri(carrier.iri().as_str()), Some(carrier));
        }
    }
}
