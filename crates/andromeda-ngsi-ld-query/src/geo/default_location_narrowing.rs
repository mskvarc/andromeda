//! What an in-memory spatial index established about a geo-query's default
//! `location` instances before the store runs it.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use derive_more::Deref;

/// Which entities' default `location` instance can satisfy a geo-query, when an
/// index has already answered that part of it.
///
/// Clause 5.7.2.4 counts an entity as matching when any instance of the target
/// `GeoProperty` meets the restriction. An index that holds one geometry per
/// entity (the default instance, the one carrying no `datasetId` or, when
/// every instance carries one, the first, clause 4.5.16.1) can answer for
/// that instance only. Its answer therefore narrows the default-instance half
/// of the relation and nothing else: every other instance of `location` is
/// still evaluated by the store, so an entity whose only matching instance has
/// a `datasetId` is found whatever the index held.
///
/// Meaningful only on a geo-query whose `geoproperty` is `location`; any other
/// `GeoProperty` is never narrowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DefaultLocationNarrowing {
    /// No index answered: the store evaluates every instance itself.
    #[default]
    Unnarrowed,
    /// The index answered: only these entities' default instance can satisfy
    /// the geo-query. The store still re-verifies each of them, so an entity
    /// listed here that no longer matches is dropped, and an empty list means
    /// no default instance matches.
    Candidates(LocationCandidates),
}

/// The entities an index names as the only ones whose default `location`
/// instance can satisfy a geo-query, at most [`LocationCandidates::MAX`] of
/// them.
///
/// A store looks each candidate up by its identifier, while its own spatial
/// index reads the instances a box holds from a few adjacent pages, so past a
/// few dozen candidates the box reads less. An index that finds more leaves
/// the geo-query unnarrowed rather than name them.
#[derive(Debug, Clone, PartialEq, Eq, Deref)]
#[deref(forward)]
pub struct LocationCandidates(
    /// The candidates, each once.
    Vec<Identifier>,
);

impl LocationCandidates {
    /// The most candidates a narrowing names.
    pub const MAX: usize = 64;

    /// `identifiers` as a narrowing, or `None` when they number more than
    /// [`LocationCandidates::MAX`].
    #[must_use]
    pub fn new(identifiers: Vec<Identifier>) -> Option<LocationCandidates> {
        (identifiers.len() <= LocationCandidates::MAX).then_some(LocationCandidates(identifiers))
    }
}

#[cfg(test)]
mod tests {
    use crate::geo::default_location_narrowing::LocationCandidates;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn vehicles(count: usize) -> Vec<Identifier> {
        (0..count)
            .map(|n| format!("urn:ngsi-ld:Vehicle:v{n}").parse().expect("a well-formed URI"))
            .collect()
    }

    #[test]
    fn a_narrowing_names_at_most_the_candidates_a_store_looks_up_one_by_one() {
        assert_eq!(LocationCandidates::new(vehicles(64)).map(|candidates| candidates.to_vec()), Some(vehicles(64)));
        assert_eq!(LocationCandidates::new(vehicles(65)), None);
        assert_eq!(LocationCandidates::new(Vec::new()).map(|candidates| candidates.is_empty()), Some(true));
    }
}
