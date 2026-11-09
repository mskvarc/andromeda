//! Holding a query's entity selection to the entities a reused Entity Map lists
//! (clause 5.5.14).
//!
//! Clause 5.5.14: "If a subsequent request references an existent `EntityMap`,
//! it shall be used for the purposes of Entity registration matching, and
//! queries shall be filtered to only consider the Entities listed [...] An
//! `EntityMap` fixes the Entities to be considered for subsequent requests
//! based on it."
//!
//! A query names the entities it asks about in two places, and narrowing only
//! one of them narrows nothing: the flat identifier list a query string
//! carries, and the union of clause 5.2.33 selectors a clause 5.2.23 body
//! carries, which is what compilation reads whenever the body carried one. Both
//! the current-state and the temporal bindings scope a follow-up here so the
//! two cannot disagree on what a map fixes.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_map::EntityMap;
use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;

/// Intersect a request's identifiers with the entities a live map lists,
/// preserving the order the request wrote them in.
///
/// `None` is a request that named no identifier, which clause 5.5.14 holds to
/// the whole map: a follow-up may name its own identifiers but never widen past
/// what the map fixed.
#[must_use]
pub fn scope_query_ids(map: &EntityMap, existing_ids: Option<&[Identifier]>) -> Vec<Identifier> {
    match existing_ids {
        Some(existing) => existing
            .iter()
            .filter(|identifier| map.contains(identifier))
            .cloned() // clone: the result owns identifiers borrowed from the incoming query.
            .collect(),
        None => map.entity_ids().cloned().collect(), // clone: the result owns identifiers borrowed from the map.
    }
}

/// Hold a query's whole entity selection to the entities a live map lists.
///
/// A query string says what it selects through the flat identifier list; a
/// clause 5.2.23 body says it through the union of clause 5.2.33 selectors,
/// which is what compilation reads whenever the body carried one. Both have to
/// be narrowed, or a POST binding would answer with entities the map does not
/// list. A selector left with no identifier admits nothing and so contributes
/// nothing to the union; a union emptied that way leaves the request
/// considering no entity at all, which only the flat list can express.
pub fn hold_selection_to_map(map: &EntityMap, id: &mut Option<Vec<Identifier>>, selectors: &mut Vec<EntitySelectorParams>) {
    if selectors.is_empty() {
        *id = Some(scope_query_ids(map, id.as_deref()));
        return;
    }
    selectors.retain_mut(|selector| {
        let scoped = scope_query_ids(map, selector.id.as_deref());
        let admits_an_entity = !scoped.is_empty();
        selector.id = Some(scoped);
        admits_an_entity
    });
    *id = match selectors.as_slice() {
        // The flat list summarises a single selector, exactly as the clause
        // 5.2.23 body conversion writes it.
        [selector] => selector.id.clone(), // clone: the summary owns its copy of the selector's identifiers
        // Several selectors have no single flat summary, and the union decides.
        [_, _, ..] => None,
        // Every selector was emptied, so the union admits nothing.
        [] => Some(Vec::new()),
    };
}

#[cfg(test)]
mod tests {
    use crate::query_scoping::{hold_selection_to_map, scope_query_ids};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use andromeda_ngsi_ld_request::query::entity_selector::EntitySelectorParams;
    use chrono::{TimeDelta, Utc};

    /// An entity identifier, as the request boundary would have validated it.
    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("a valid NGSI-LD URI")
    }

    /// A live map over the entities this broker holds itself.
    fn map_over(entity_ids: &[&str]) -> EntityMap {
        EntityMap::local(
            identifier("urn:ngsi-ld:EntityMap:scoping"),
            Utc::now() + TimeDelta::hours(1),
            entity_ids.iter().map(|id| identifier(id)),
        )
    }

    /// One clause 5.2.33 selector over the given identifiers.
    fn selector(entity_ids: &[&str]) -> EntitySelectorParams {
        EntitySelectorParams {
            id: Some(entity_ids.iter().map(|id| identifier(id)).collect()),
            ..EntitySelectorParams::default()
        }
    }

    /// The identifiers a selector was narrowed to, as wire text.
    fn selector_ids(selectors: &[EntitySelectorParams]) -> Vec<Vec<&str>> {
        selectors
            .iter()
            .map(|selector| selector.id.as_deref().unwrap_or_default().iter().map(Identifier::as_str).collect())
            .collect()
    }

    /// The identifiers a flat list was narrowed to, as wire text.
    fn flat_ids(id: Option<&Vec<Identifier>>) -> Vec<&str> {
        id.map_or_default(|ids| ids.iter().map(Identifier::as_str).collect())
    }

    /// The intersection keeps the order the incoming query wrote, because the
    /// clause 4.23 ordering of a follow-up is the request's, not the map's.
    #[test]
    fn the_intersection_preserves_the_incoming_query_order() {
        let map = map_over(&["urn:a", "urn:b"]);
        let incoming = [identifier("urn:b"), identifier("urn:a"), identifier("urn:c")];
        pretty_assertions::assert_eq!(scope_query_ids(&map, Some(&incoming)), vec![identifier("urn:b"), identifier("urn:a")]);
    }

    /// Clause 5.5.14: a request naming no identifier is held to the whole map.
    #[test]
    fn a_request_naming_no_identifier_is_held_to_the_whole_map() {
        let map = map_over(&["urn:a", "urn:b"]);
        pretty_assertions::assert_eq!(scope_query_ids(&map, None), vec![identifier("urn:a"), identifier("urn:b")]);
    }

    /// Clause 5.5.14: a request naming only identifiers the map does not list
    /// is narrowed to an empty set, never widened back to the whole map.
    #[test]
    fn a_request_naming_nothing_the_map_lists_is_narrowed_to_an_empty_set() {
        let map = map_over(&["urn:a"]);
        let incoming = [identifier("urn:outside")];
        pretty_assertions::assert_eq!(scope_query_ids(&map, Some(&incoming)), Vec::<Identifier>::new());
    }

    /// Clause 5.5.14: a query string names its entities in the flat identifier
    /// list, and a follow-up naming none considers everything the map lists.
    #[test]
    fn a_query_string_follow_up_naming_no_identifier_considers_the_whole_map() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = Vec::new();

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(flat_ids(id.as_ref()), vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
    }

    /// Clause 5.5.14: "queries shall be filtered to only consider the Entities
    /// listed". A clause 5.2.33 selector is what compilation reads on the POST
    /// binding, so it is narrowed to the intersection with the map.
    #[test]
    fn a_body_selector_is_narrowed_to_the_entities_the_map_lists() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![selector(&["urn:ngsi-ld:Vehicle:b", "urn:ngsi-ld:Vehicle:outside"])];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:b"]]);
        pretty_assertions::assert_eq!(flat_ids(id.as_ref()), vec!["urn:ngsi-ld:Vehicle:b"]);
    }

    /// A body selector that named no identifier is held to the whole map, so
    /// the follow-up cannot widen past what the map fixed.
    #[test]
    fn a_body_selector_naming_no_identifier_is_held_to_the_whole_map() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![EntitySelectorParams::default()];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]]);
    }

    /// Clause 5.5.14: a follow-up naming only identifiers the map does not list
    /// considers no entity at all, which the emptied flat list carries.
    #[test]
    fn a_body_selector_naming_nothing_the_map_lists_leaves_no_entity_considered() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a"]);
        let mut id = None;
        let mut selectors = vec![selector(&["urn:ngsi-ld:Vehicle:outside"])];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        assert!(selectors.is_empty(), "a selector admitting nothing contributes nothing to the union");
        pretty_assertions::assert_eq!(id, Some(Vec::new()));
    }

    /// A union keeps only the selectors that still admit an entity, and the
    /// remaining ones decide together.
    #[test]
    fn a_union_keeps_only_the_selectors_the_map_still_admits() {
        let map = map_over(&["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]);
        let mut id = None;
        let mut selectors = vec![
            selector(&["urn:ngsi-ld:Vehicle:outside"]),
            selector(&["urn:ngsi-ld:Vehicle:a"]),
            selector(&["urn:ngsi-ld:Vehicle:b"]),
        ];

        hold_selection_to_map(&map, &mut id, &mut selectors);

        pretty_assertions::assert_eq!(selector_ids(&selectors), vec![vec!["urn:ngsi-ld:Vehicle:a"], vec!["urn:ngsi-ld:Vehicle:b"]]);
        pretty_assertions::assert_eq!(id, None, "several selectors have no single flat summary");
    }

    /// An emptied map admits nothing whatever the follow-up names, on both the
    /// query-string and the body binding: the clause 4.3.6.7 pruning can leave a
    /// map listing no entity at all, and such a map still fixes what a follow-up
    /// considers.
    #[test]
    fn an_emptied_map_leaves_every_binding_considering_no_entity() {
        let map = map_over(&[]);

        let mut string_id = Some(vec![identifier("urn:ngsi-ld:Vehicle:a")]);
        let mut no_selectors = Vec::new();
        hold_selection_to_map(&map, &mut string_id, &mut no_selectors);
        pretty_assertions::assert_eq!(string_id, Some(Vec::new()));

        let mut body_id = None;
        let mut body_selectors = vec![EntitySelectorParams::default()];
        hold_selection_to_map(&map, &mut body_id, &mut body_selectors);
        assert!(body_selectors.is_empty(), "an emptied map admits no selector");
        pretty_assertions::assert_eq!(body_id, Some(Vec::new()));
    }
}
