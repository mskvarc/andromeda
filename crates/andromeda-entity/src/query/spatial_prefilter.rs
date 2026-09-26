//! Narrowing a geo-query to an identifier set through the in-memory spatial
//! index.

use crate::query::EntityQueryService;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::{entity::compiled::CompiledEntityQuery, entity_selection::EntitySelector, geo::query::GeoQuery};
use andromeda_spatial_index::{
    entity::index::SpatialEntityIndex,
    key::{SpatialKey, flatten_or_type_iris},
};
use iri_rs::IriBuf;
use std::{collections::HashSet, sync::Arc};

/// The outcome of consulting the in-memory spatial index for a geo query.
pub(crate) enum SpatialNarrowing {
    /// Run this query against the store. It is the client's query, either
    /// unchanged (not geo-accelerated, an index that may not narrow it, or a
    /// reference geometry the index cannot use) or narrowed to the candidate
    /// identifier set the index produced.
    Query(Arc<CompiledEntityQuery>),
    /// The index authoritatively matched nothing. The result set is empty
    /// and the store must not be consulted: an empty candidate set must never be
    /// expressed as an empty `ids` vector, which the statement builder reads as
    /// "no id filter" and — with the geo filter already dropped — turns into a
    /// query that returns every entity matching the remaining filters. Clause
    /// 5.5.14 has its own spelling for a request that considers no entity
    /// ([`ConsideredEntities::NoEntity`]); this one answers without a round trip
    /// at all, which is what an index that already knows the answer is for.
    ///
    /// [`ConsideredEntities::NoEntity`]: andromeda_ngsi_ld_query::entity_selection::ConsideredEntities::NoEntity
    EmptyResult,
}

/// The part of a query the index can answer: one geo filter over an
/// enumerable set of entity types.
struct Accelerable<'a> {
    /// The query's geo filter.
    geo: &'a GeoQuery,
    /// The query's one entity selector.
    selector: &'a EntitySelector,
    /// The expanded types the selector reaches.
    type_iris: Vec<IriBuf>,
}

/// Whether the index can answer a query at all, before any index is consulted.
///
/// The index is keyed by a single `(type, geoproperty)` pair, so it
/// accelerates a query with exactly one selector whose type expression is a
/// type or a union of types. A union of selectors (clause 5.2.33), a
/// conjunctive type expression or an untyped selector is handed to the store
/// to evaluate in full.
fn accelerable(compiled: &CompiledEntityQuery) -> Option<Accelerable<'_>> {
    let geo = compiled.selection.geo.as_ref()?;
    let [selector] = compiled.selection.selectors.as_slice() else {
        return None;
    };
    let type_iris = flatten_or_type_iris(selector.type_expr.as_ref()?)?;
    Some(Accelerable { geo, selector, type_iris })
}

impl EntityQueryService {
    /// Consult the spatial index to narrow the selector's identifiers before
    /// the store runs, keeping `selection.geo` so the store still re-verifies
    /// each candidate's geometry.
    ///
    /// The index narrows only where it holds every entity the geo filter could
    /// match -- what [`SpatialAcceleration`] guarantees for the configured
    /// consistency mode, and what an unseeded scope declines to claim. Holding
    /// entities that no longer match is harmless: the store re-applies the
    /// clause 4.10 relation to every candidate, and drops the ones that do not
    /// match. The cost is one `georel` check over a small candidate set instead
    /// of the whole table.
    ///
    /// Falls through to [`SpatialNarrowing::Query`] with the query unchanged when
    /// it is not geo-accelerable, when no index may narrow it, or for a
    /// Snapshot branch, which the live index does not describe. The store then
    /// applies the full `georel`. An index that matches nothing yields
    /// [`SpatialNarrowing::EmptyResult`].
    ///
    /// [`SpatialAcceleration`]: crate::spatial_acceleration::SpatialAcceleration
    pub(crate) async fn spatial_prefilter(&self, target: &ContextTarget, compiled: &Arc<CompiledEntityQuery>) -> SpatialNarrowing {
        if target.snapshot().is_some() {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        }
        let Some(accelerable) = accelerable(compiled) else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };
        let Some(index) = self.spatial.index_for_query().await else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };
        let Some(candidates) = self.candidates(index, &accelerable) else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };

        let final_ids: Vec<Uri> = if accelerable.selector.ids.is_empty() {
            candidates.into_iter().collect()
        } else {
            let requested: HashSet<&Uri> = accelerable.selector.ids.iter().collect();
            candidates.into_iter().filter(|id| requested.contains(id)).collect()
        };

        // An index that narrowed to no candidates is an authoritative empty
        // answer. It must not become `selection.ids = []`, which the statement
        // builder reads as unconstrained (see [`SpatialNarrowing::EmptyResult`]).
        if final_ids.is_empty() {
            return SpatialNarrowing::EmptyResult;
        }

        let mut narrowed = (**compiled).clone(); // clone: the cached query is shared and must not be mutated in place
        // The single selector's own id list is narrowed to the index candidates.
        narrowed.selection.selectors[0].ids = final_ids;
        // `selection.geo` is deliberately left in place: the store re-verifies
        // every candidate, so a candidate that no longer matches is dropped.
        tracing::debug!(
            "spatial prefilter hit: {} candidate(s) for geoQ={:?}",
            narrowed.selection.selectors[0].ids.len(),
            accelerable.geo.georel
        );
        SpatialNarrowing::Query(Arc::new(narrowed))
    }

    /// The union of every type partition's candidates, or `None` when any one
    /// partition declines to answer.
    fn candidates(&self, index: &SpatialEntityIndex, accelerable: &Accelerable<'_>) -> Option<HashSet<Uri>> {
        let mut union: HashSet<Uri> = HashSet::new();
        for type_iri in &accelerable.type_iris {
            let key = SpatialKey::new(
                self.tenant.clone(),                 // clone: the spatial key owns its tenant
                type_iri.clone(),                    // clone: the spatial key owns its type
                accelerable.geo.geoproperty.clone(), // clone: the spatial key owns its geoproperty
            );
            // A partition that declines for any one type would otherwise
            // contribute an empty candidate set, which is indistinguishable from
            // that type having no match. The store applies the full `georel`.
            union.extend(index.query(&key, &accelerable.geo.georel, &accelerable.geo.geometry)?);
        }
        Some(union)
    }
}
