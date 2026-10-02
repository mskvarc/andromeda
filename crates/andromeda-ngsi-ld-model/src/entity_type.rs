//! The `type` member of an entity, which carries one type name or several (ETSI GS CIM 009 clause 5.2.4).

use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use serde::{Deserialize, Serialize};
use std::slice::{Iter, from_ref};

/// Single or multiple entity type values (clause 5.2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
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
}
