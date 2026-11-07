//! The entity states a `q` hop crosses into (clause 4.9).
//!
//! A hop "makes a sub-query on matching target objects" (clause 4.9 EXAMPLE
//! 13): the entities a Relationship's objects name. Whoever evaluates a filter
//! in memory gathers those states beforehand and hands them to the evaluator
//! through this map.

use crate::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::queried_entity::linked_entities::LinkedEntities;
use std::{collections::HashMap, sync::Arc};

/// Linked entity states keyed by identifier.
#[derive(Clone, Debug, Default)]
pub struct LinkedStates(HashMap<Identifier, Arc<EntityState>>);

impl LinkedStates {
    /// No linked state.
    #[must_use]
    pub fn new() -> LinkedStates {
        LinkedStates(HashMap::new())
    }

    /// Whether the map holds the state of `id`.
    #[must_use]
    pub fn holds(&self, id: &Identifier) -> bool {
        self.0.contains_key(id)
    }

    /// The state of `id`, when the map holds it.
    #[must_use]
    pub fn get(&self, id: &Identifier) -> Option<&EntityState> {
        self.0.get(id).map(Arc::as_ref)
    }

    /// Hold `state` under its identifier, replacing any state held there.
    pub fn insert(&mut self, state: Arc<EntityState>) {
        self.0.insert(state.id().clone(), state); // clone: the map keys the state by the identifier it also keeps
    }

    /// Whether the map holds no state.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> LinkedEntities<'a> for &'a LinkedStates {
    type Entity = &'a EntityState;

    /// A Relationship object is an Entity's URI (clause 4.5.3); an object that
    /// is no identifier names no gathered state.
    fn linked(self, object: &str) -> Option<&'a EntityState> {
        let id: Identifier = object.parse().ok()?;
        self.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_state::EntityState, entity_types::EntityTypes, queried::linked_states::LinkedStates};
    use andromeda_ngsi_ld_query::queried_entity::linked_entities::LinkedEntities;
    use std::sync::Arc;

    fn parking() -> Arc<EntityState> {
        Arc::new(EntityState::new(
            "urn:ngsi-ld:Parking:1".parse().expect("valid URI"),
            EntityTypes::one("https://example.com/Parking".parse().expect("valid type name")),
        ))
    }

    #[test]
    fn a_linked_state_is_found_by_its_identifier() {
        let mut linked = LinkedStates::new();
        linked.insert(parking());
        let found = (&linked).linked("urn:ngsi-ld:Parking:1").expect("the parking is held");
        pretty_assertions::assert_eq!(found.id().as_str(), "urn:ngsi-ld:Parking:1");
    }

    #[test]
    fn an_object_naming_no_gathered_entity_links_nothing() {
        let mut linked = LinkedStates::new();
        linked.insert(parking());
        assert!((&linked).linked("urn:ngsi-ld:Parking:2").is_none());
        assert!((&linked).linked("not a uri").is_none());
    }
}
