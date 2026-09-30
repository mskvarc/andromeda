//! Which entities a prepared result page sheds from the Entity Map it ran
//! under (clauses 4.3.6.7, 5.5.9.3 and 5.5.14).

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_map::{EntityMap, EntityMapSource};
use std::collections::HashSet;

/// What preparing one result page from a reused Entity Map established about
/// the entities the map lists.
///
/// Clause 4.3.6.7 applies the original request's query, geoquery, scope query
/// and attribute filters to an entity before it joins a result page, and
/// removes from the map "identifiers of Entities not matching the filters at
/// the time of checking"; clause 5.5.9.3 repeats it for the candidate entities
/// a split-entity map holds, and clause 5.5.14 puts the duty on the broker that
/// created the map.
///
/// "At the time of checking" is why the consulted sources are recorded beside
/// the matches: an entity whose only holder did not answer this preparation was
/// never checked, so nothing is known about it and it stays in the map.
pub struct PageCheck {
    /// The identifiers that still matched while the page was prepared.
    matching: HashSet<Identifier>,
    /// The registrations whose context sources answered this preparation.
    consulted: HashSet<Identifier>,
}

impl PageCheck {
    /// Record what one page preparation checked: the identifiers that still
    /// matched the request's filters, and the registrations whose context
    /// sources answered it.
    pub fn new(matching: impl IntoIterator<Item = Identifier>, consulted: impl IntoIterator<Item = Identifier>) -> PageCheck {
        PageCheck {
            matching: matching.into_iter().collect(),
            consulted: consulted.into_iter().collect(),
        }
    }

    /// The identifiers `map` sheds: every entity it lists that this preparation
    /// checked and found no longer matching, in map order.
    #[must_use]
    pub fn shed_from(&self, map: &EntityMap) -> Vec<Identifier> {
        map.entity_map
            .iter()
            .filter(|(entity_id, sources)| !self.matching.contains(*entity_id) && sources.iter().all(|source| self.was_consulted(source)))
            .map(|(entity_id, _)| entity_id.clone()) // clone: the removal list outlives the borrow of the map it was read from
            .collect()
    }

    /// Whether the holder `source` names answered this page preparation.
    ///
    /// A broker reads its own store for every page it prepares, so an entity it
    /// holds itself is always checked; a registration was checked only when its
    /// context source contributed to this preparation.
    fn was_consulted(&self, source: &EntityMapSource) -> bool {
        match source {
            EntityMapSource::Local => true,
            EntityMapSource::Registration(registration) => self.consulted.contains(registration),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pruning::PageCheck;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_map::EntityMap;
    use chrono::{TimeZone, Utc};

    fn identifier(value: &str) -> Identifier {
        value.parse::<Identifier>().expect("a valid test identifier")
    }

    fn map_of(entity_ids: impl IntoIterator<Item = Identifier>) -> EntityMap {
        let expires_at = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).single().expect("a valid instant");
        EntityMap::local(identifier("urn:ngsi-ld:EntityMap:1"), expires_at, entity_ids)
    }

    /// Clause 4.3.6.7: an entity this broker holds that no longer matches the
    /// filters is shed, and one that still matches stays.
    #[test]
    fn a_locally_held_entity_that_stopped_matching_is_shed() {
        let map = map_of([identifier("urn:a"), identifier("urn:b")]);
        let check = PageCheck::new([identifier("urn:a")], Vec::new());
        pretty_assertions::assert_eq!(check.shed_from(&map), vec![identifier("urn:b")]);
    }

    /// Clause 4.3.6.7 removes what was checked and found not matching, so an
    /// entity held only by a context source that did not answer this
    /// preparation was never checked and stays in the map.
    #[test]
    fn an_entity_whose_only_source_was_not_consulted_stays() {
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b");
        let mut map = map_of([identifier("urn:a")]);
        map.add_remote_entities(&registration, [identifier("urn:remote")]);

        let unconsulted = PageCheck::new(Vec::new(), Vec::new());
        pretty_assertions::assert_eq!(unconsulted.shed_from(&map), vec![identifier("urn:a")]);

        let consulted = PageCheck::new(Vec::new(), [registration]);
        pretty_assertions::assert_eq!(consulted.shed_from(&map), vec![identifier("urn:a"), identifier("urn:remote")]);
    }

    /// An entity held both locally and remotely is only shed once every holder
    /// has been checked, since a source that did not answer may still hold a
    /// matching part of it (clause 5.5.9.3).
    #[test]
    fn an_entity_with_an_unconsulted_holder_beside_a_checked_one_stays() {
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b");
        let mut map = map_of([identifier("urn:split")]);
        map.add_remote_entities(&registration, [identifier("urn:split")]);
        let check = PageCheck::new(Vec::new(), Vec::new());
        pretty_assertions::assert_eq!(check.shed_from(&map), Vec::<Identifier>::new());
    }

    /// An identifier the preparation still matched is never shed, whichever
    /// source reported it.
    #[test]
    fn a_still_matching_entity_is_never_shed() {
        let registration = identifier("urn:ngsi-ld:ContextSourceRegistration:b");
        let mut map = map_of([identifier("urn:a")]);
        map.add_remote_entities(&registration, [identifier("urn:remote")]);
        let check = PageCheck::new([identifier("urn:a"), identifier("urn:remote")], [registration]);
        pretty_assertions::assert_eq!(check.shed_from(&map), Vec::<Identifier>::new());
    }
}
