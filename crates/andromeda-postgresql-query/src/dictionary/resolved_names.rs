//! The dictionary keys a statement is rendered against.

use crate::dictionary::interned_name::InternedName;
use std::collections::HashMap;

/// Dictionary keys for the names a compiled query mentions, hits only.
///
/// A name that is absent has no dictionary row, and therefore no row in any
/// relation keyed on that dictionary either, so a predicate on it compiles to
/// constant `FALSE`. There is deliberately no way to represent a *cached*
/// miss: resolution runs per query execution against a cache that only ever
/// stores hits, so a name created after a filter was first compiled becomes
/// visible on the next execution instead of being pinned absent.
#[derive(Debug, Clone)]
pub struct ResolvedNames<N: InternedName> {
    /// Name to dictionary key, hits only.
    keys: HashMap<N, N::Key>,
}

// Derived `Default` would demand `N: Default`, which a validated name cannot
// offer; an empty map needs nothing from the name type.
impl<N: InternedName> Default for ResolvedNames<N> {
    fn default() -> ResolvedNames<N> {
        ResolvedNames { keys: HashMap::new() }
    }
}

impl<N: InternedName> ResolvedNames<N> {
    /// Build from resolved `(name, dictionary key)` pairs.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (N, N::Key)>) -> ResolvedNames<N> {
        ResolvedNames {
            keys: pairs.into_iter().collect(),
        }
    }

    /// Dictionary key for a name, if the broker knows it.
    pub fn get(&self, name: &N) -> Option<N::Key> {
        self.keys.get::<N>(name).copied()
    }

    /// Every resolved name with its dictionary key, in no order.
    pub fn iter(&self) -> impl Iterator<Item = (&N, N::Key)> {
        self.keys.iter().map(|(name, key)| (name, *key))
    }

    /// Dictionary key for a name given as text, if the broker knows it.
    ///
    /// The query grammars carry some names in a sibling type — a type
    /// selection holds an `IriBuf`, a linked-entity guard holds attribute-name
    /// typed tokens — and their text is the dictionary's own key form, so it
    /// is looked up without re-validating it into `N`.
    #[must_use]
    pub fn get_str(&self, name: &str) -> Option<N::Key> {
        self.keys.get::<str>(name).copied()
    }

    /// The dictionary keys of those of `names` the broker knows, in the order
    /// given: a name it does not know was never written, so it has no key and
    /// is left out.
    #[must_use]
    pub fn keys_of<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Vec<N::Key> {
        names.into_iter().filter_map(|name| self.get_str(name)).collect()
    }

    /// Whether no name resolved to a dictionary key, in which case every
    /// dictionary-keyed probe the query would compile is already known to be
    /// unsatisfiable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames, entity_type_name_key::EntityTypeNameKey};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

    #[test]
    fn a_resolved_name_answers_by_value_and_by_text() {
        let name: EntityTypeName = "https://example.com/Vehicle".parse().expect("valid name");
        let resolved = ResolvedNames::from_pairs([(name.clone(), EntityTypeNameKey::from(4))]);
        pretty_assertions::assert_eq!(resolved.get(&name), Some(EntityTypeNameKey::from(4)));
        pretty_assertions::assert_eq!(resolved.get_str("https://example.com/Vehicle"), Some(EntityTypeNameKey::from(4)));
        assert!(!resolved.is_empty());
    }

    #[test]
    fn every_resolved_name_is_listed_with_its_key() {
        let name: EntityTypeName = "https://example.com/Vehicle".parse().expect("valid name");
        let resolved = ResolvedNames::from_pairs([(name.clone(), EntityTypeNameKey::from(4))]);
        pretty_assertions::assert_eq!(resolved.iter().collect::<Vec<_>>(), vec![(&name, EntityTypeNameKey::from(4))]);
    }

    #[test]
    fn an_unresolved_name_answers_nothing() {
        let resolved: ResolvedNames<AttributeName> = ResolvedNames::default();
        let name: AttributeName = "https://example.com/speed".parse().expect("valid name");
        pretty_assertions::assert_eq!(resolved.get(&name), None::<AttributeNameKey>);
        assert!(resolved.is_empty());
    }
}
