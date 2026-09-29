//! What an in-memory spatial index established about a geo-query's default
//! `location` instances before the store runs it.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Which entities' default `location` instance can satisfy a geo-query, when an
/// index has already answered that part of it.
///
/// Clause 5.7.2.4 counts an entity as matching when any instance of the target
/// `GeoProperty` meets the restriction. An index that holds one geometry per
/// entity -- the default instance, the one carrying no `datasetId` or, when
/// every instance carries one, the first (clause 4.5.16.1) -- can answer for
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
    Candidates(Vec<Identifier>),
}
