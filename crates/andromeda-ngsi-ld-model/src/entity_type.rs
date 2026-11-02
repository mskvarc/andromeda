//! The `type` member of an entity, which carries one type name or several (ETSI GS CIM 009 clause 5.2.4).

use crate::attribute::one_or_many::OneOrMany;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use serde::{Deserialize, Serialize};
use std::slice::{Iter, from_ref};

/// Single or multiple entity type values (clause 5.2.4).
///
/// Read as a `String` or `String[]` member, so a name outside the clause 4.6.2
/// grammar is refused with the grammar's own reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, from = "OneOrMany<EntityTypeName>")]
pub enum EntityTypeValue {
    /// Exactly one type.
    Single(EntityTypeName),
    /// Several types declared together.
    Multiple(Vec<EntityTypeName>),
}

impl EntityTypeValue {
    /// Iterate the contained type names uniformly regardless of variant.
    pub fn iter(&self) -> Iter<'_, EntityTypeName> {
        match self {
            EntityTypeValue::Single(name) => from_ref(name).iter(),
            EntityTypeValue::Multiple(names) => names.iter(),
        }
    }
}

// Hand-written: the two enums carry the same payloads under different variant
// names, and no derive maps one enum onto another.
impl From<OneOrMany<EntityTypeName>> for EntityTypeValue {
    fn from(written: OneOrMany<EntityTypeName>) -> EntityTypeValue {
        match written {
            OneOrMany::One(name) => EntityTypeValue::Single(name),
            OneOrMany::Many(names) => EntityTypeValue::Multiple(names),
        }
    }
}

// Borrowed iteration goes through the inherent `iter`; the impl is what lets a
// `&EntityTypeValue` be used directly in a `for` loop or an iterator adapter.
impl<'a> IntoIterator for &'a EntityTypeValue {
    type Item = &'a EntityTypeName;
    type IntoIter = Iter<'a, EntityTypeName>;

    fn into_iter(self) -> Iter<'a, EntityTypeName> {
        self.iter()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_type::EntityTypeValue;

    #[test]
    fn an_entity_type_value_keeps_the_single_and_array_wire_forms_apart() {
        let single: EntityTypeValue = serde_json::from_value(serde_json::json!("Building")).expect("single");
        pretty_assertions::assert_eq!(serde_json::to_value(&single).expect("serialises"), serde_json::json!("Building"));
        pretty_assertions::assert_eq!(single.iter().count(), 1);

        let multiple: EntityTypeValue = serde_json::from_value(serde_json::json!(["Building", "Venue"])).expect("multiple");
        pretty_assertions::assert_eq!(serde_json::to_value(&multiple).expect("serialises"), serde_json::json!(["Building", "Venue"]));
        pretty_assertions::assert_eq!(multiple.iter().count(), 2);
    }

    #[test]
    fn a_malformed_type_name_and_a_type_of_another_kind_are_refused_for_different_reasons() {
        let malformed = serde_json::from_str::<EntityTypeValue>(r#""Bio-geographicalRegion""#).expect_err("a hyphenated name");
        assert!(
            malformed.to_string().contains("a name must be a term, a compact IRI or an absolute IRI"),
            "{malformed}"
        );
        let in_array = serde_json::from_str::<EntityTypeValue>(r#"["Building", "Bio-geographicalRegion"]"#).expect_err("a hyphenated name");
        assert!(
            in_array.to_string().contains("a name must be a term, a compact IRI or an absolute IRI"),
            "{in_array}"
        );
        let number = serde_json::from_str::<EntityTypeValue>("42").expect_err("a number");
        assert!(number.to_string().contains("expected a string or an array of strings"), "{number}");
    }
}
