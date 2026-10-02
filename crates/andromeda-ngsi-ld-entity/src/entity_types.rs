//! The types of one entity (clause 4.5.1, clause 4.16).
//!
//! An entity has at least one type and may have several. The types are a set,
//! so a repeated name names nothing new; the order is kept because the stored
//! document and every representation list them in the order they were written.

use crate::non_empty::{EmptyCollection, NonEmpty};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use derive_more::{AsRef, Deref};

/// One or more entity type names, in order, each at most once.
#[derive(AsRef, Clone, Debug, Deref, Eq, PartialEq)]
#[as_ref(forward)]
pub struct EntityTypes(NonEmpty<EntityTypeName>);

impl EntityTypes {
    /// Hold exactly one type.
    #[must_use]
    pub fn one(name: EntityTypeName) -> EntityTypes {
        EntityTypes(NonEmpty::one(name))
    }

    /// Hold `names` in order, keeping the first occurrence of a repeated name.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyCollection`] when `names` is empty.
    pub fn try_from_names(names: impl IntoIterator<Item = EntityTypeName>) -> Result<EntityTypes, EmptyCollection> {
        let mut distinct: Vec<EntityTypeName> = Vec::new();
        for name in names {
            if !distinct.contains(&name) {
                distinct.push(name);
            }
        }
        NonEmpty::try_from_vec(distinct).map(EntityTypes)
    }

    /// Add the names this set lacks, in the order given (the clause 5.6.17
    /// type union).
    pub fn extend(&mut self, names: impl IntoIterator<Item = EntityTypeName>) {
        for name in names {
            if !self.0.contains(&name) {
                self.0.push(name);
            }
        }
    }

    /// Whether `name` is one of the types.
    #[must_use]
    pub fn contains(&self, name: &EntityTypeName) -> bool {
        self.0.contains(name)
    }

    /// The types as a slice, in order.
    #[must_use]
    pub fn as_slice(&self) -> &[EntityTypeName] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_types::EntityTypes, non_empty::EmptyCollection};
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;

    fn name(raw: &str) -> EntityTypeName {
        raw.parse().expect("a valid type name")
    }

    #[test]
    fn a_repeated_type_is_kept_once_in_first_position() {
        let types = EntityTypes::try_from_names([name("https://ex.org/A"), name("https://ex.org/B"), name("https://ex.org/A")]).expect("two types");
        pretty_assertions::assert_eq!(types.as_slice(), &[name("https://ex.org/A"), name("https://ex.org/B")]);
    }

    #[test]
    fn no_type_is_refused() {
        pretty_assertions::assert_eq!(EntityTypes::try_from_names([]), Err(EmptyCollection));
    }

    #[test]
    fn a_union_appends_only_the_missing_types() {
        let mut types = EntityTypes::one(name("https://ex.org/A"));
        types.extend([name("https://ex.org/A"), name("https://ex.org/C")]);
        pretty_assertions::assert_eq!(types.as_slice(), &[name("https://ex.org/A"), name("https://ex.org/C")]);
    }
}
