//! Which Linked Entities a payload's Relationships still name (clause 4.5.23).
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.1.5 makes membership of the flattened
//! array follow the payload rather than the traversal: "If any of the returned
//! Attributes corresponds to an annotated Relationship, unless a target URI has
//! been previously encountered, the data corresponding to each URI of the
//! Relationship's target object URIs is appended to the array." Clause 5.7.2.5
//! says the same of a query. The Attributes that decide it are the *returned*
//! ones, so a Relationship the clause 4.21 projection removed names nothing any
//! more, and whatever was reachable only through it is no longer part of the
//! answer.
//!
//! The traversal cannot decide this on its own: it resolves a level before the
//! nested projections of the level below are known, so an entity it legitimately
//! fetched can become unreachable once those projections are applied. This is
//! the pass that removes those.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_model::{attribute::instance::Attribute, entity::Entity};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Drop every linked entity no returned Relationship names any more.
///
/// The walk starts at the roots and follows the Relationships the entities
/// still carry, so an entity reached only through a Relationship the projection
/// removed is dropped along with everything reachable only through it. The
/// surviving entities keep their traversal order, which clause 4.5.23.3 leaves
/// open and Andromeda fixes.
pub fn retain_reachable(roots: &[Entity], linked: &mut IndexMap<Identifier, Entity>) {
    let mut reachable: HashSet<Identifier> = HashSet::new();
    let mut frontier: Vec<Identifier> = roots.iter().flat_map(named_targets).collect();

    while let Some(id) = frontier.pop() {
        if !linked.contains_key(&id) || !reachable.insert(id.clone()) {
            // clone: the reachable set outlives this frontier entry
            continue;
        }
        if let Some(entity) = linked.get(&id) {
            frontier.extend(named_targets(entity));
        }
    }

    linked.retain(|id, _| reachable.contains(id));
}

/// Every identifier an entity's Relationships and `ListRelationships` name.
fn named_targets(entity: &Entity) -> Vec<Identifier> {
    entity
        .attrs
        .values()
        .flat_map(|value| value.instances())
        .filter_map(relationship_targets)
        .flatten()
        .collect()
}

/// The identifiers a Relationship or `ListRelationship` instance points at, or
/// `None` for every other attribute family.
///
/// An object longer than an identifier may be names no entity any store holds,
/// so it is not among them: clause 4.6.2 admits it as a URI, but nothing can be
/// linked through it.
pub(crate) fn relationship_targets(attr: &Attribute) -> Option<Vec<Identifier>> {
    // clone: owned ids returned to the caller
    let identifiers = |objects: &mut dyn Iterator<Item = &Uri>| objects.filter_map(|object| Identifier::try_from(object.clone()).ok()).collect();
    match attr {
        Attribute::Relationship(relationship) => Some(identifiers(&mut relationship.object.iter())),
        Attribute::ListRelationship(list) => Some(identifiers(&mut list.object_list.iter())),
        Attribute::Property(_)
        | Attribute::GeoProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::VocabProperty(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::query::linked_reachability::retain_reachable;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity::Entity;
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    fn entity(payload: Value) -> Entity {
        serde_json::from_value(payload).expect("entity")
    }

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("valid URI")
    }

    fn linked_of(entities: [(&str, Value); 2]) -> IndexMap<Identifier, Entity> {
        entities.into_iter().map(|(raw, payload)| (id(raw), entity(payload))).collect()
    }

    fn building(relationship: Option<&str>) -> Value {
        let mut payload = json!({"id": "urn:ngsi-ld:Building:1", "type": "Building"});
        if let Some(target) = relationship {
            payload["locatedAt"] = json!({"type": "Relationship", "object": target});
        }
        payload
    }

    fn city(relationship: Option<&str>) -> Value {
        let mut payload = json!({"id": "urn:ngsi-ld:City:1", "type": "City"});
        if let Some(target) = relationship {
            payload["isInCountry"] = json!({"type": "Relationship", "object": target});
        }
        payload
    }

    fn country() -> Value {
        json!({"id": "urn:ngsi-ld:Country:1", "type": "Country"})
    }

    /// Every entity a chain of surviving Relationships names stays in the
    /// array, however deep it sits.
    #[test]
    fn a_chain_of_surviving_relationships_keeps_every_entity() {
        let roots = [entity(building(Some("urn:ngsi-ld:City:1")))];
        let mut linked = linked_of([
            ("urn:ngsi-ld:City:1", city(Some("urn:ngsi-ld:Country:1"))),
            ("urn:ngsi-ld:Country:1", country()),
        ]);

        retain_reachable(&roots, &mut linked);

        pretty_assertions::assert_eq!(linked.len(), 2);
    }

    /// Clause 5.7.1.5 reads the *returned* Attributes: once the projection has
    /// removed the City's own Relationship, the Country it named is no longer
    /// part of the answer.
    #[test]
    fn a_projected_away_relationship_drops_what_it_alone_reached() {
        let roots = [entity(building(Some("urn:ngsi-ld:City:1")))];
        let mut linked = linked_of([("urn:ngsi-ld:City:1", city(None)), ("urn:ngsi-ld:Country:1", country())]);

        retain_reachable(&roots, &mut linked);

        pretty_assertions::assert_eq!(linked.keys().collect::<Vec<_>>(), vec![&id("urn:ngsi-ld:City:1")]);
    }

    /// A root left with no Relationship at all names nothing, so the whole
    /// resolved set goes.
    #[test]
    fn a_root_with_no_relationship_left_names_nothing() {
        let roots = [entity(building(None))];
        let mut linked = linked_of([
            ("urn:ngsi-ld:City:1", city(Some("urn:ngsi-ld:Country:1"))),
            ("urn:ngsi-ld:Country:1", country()),
        ]);

        retain_reachable(&roots, &mut linked);

        assert!(linked.is_empty());
    }

    /// A cycle terminates: an entity already reached is not walked twice.
    #[test]
    fn a_cycle_between_linked_entities_terminates() {
        let roots = [entity(building(Some("urn:ngsi-ld:City:1")))];
        let mut linked = linked_of([
            ("urn:ngsi-ld:City:1", city(Some("urn:ngsi-ld:Country:1"))),
            (
                "urn:ngsi-ld:Country:1",
                json!({"id": "urn:ngsi-ld:Country:1", "type": "Country", "capital": {"type": "Relationship", "object": "urn:ngsi-ld:City:1"}}),
            ),
        ]);

        retain_reachable(&roots, &mut linked);

        pretty_assertions::assert_eq!(linked.len(), 2);
    }
}
