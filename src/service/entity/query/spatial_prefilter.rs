//! Narrowing a geo-query to an identifier set through the in-memory spatial
//! index.

use crate::service::entity::query::EntityQueryService;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_spatial_index::key::{SpatialKey, flatten_or_type_iris};
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

/// The outcome of consulting the in-memory spatial index for a geo query.
pub(super) enum SpatialNarrowing {
    /// Run this query against the store. It is the client's query, either
    /// unchanged (not geo-accelerated, a cold partition, or a reference geometry
    /// the index cannot use) or narrowed to the candidate identifier set a warm
    /// index produced.
    Query(Arc<CompiledEntityQuery>),
    /// The warm index authoritatively matched nothing. The result set is empty
    /// and the store must not be consulted: an empty candidate set must never be
    /// expressed as an empty `ids` vector, which the statement builder reads as
    /// "no id filter" and — with the geo filter already dropped — turns into a
    /// query that returns every entity matching the remaining filters.
    EmptyResult,
}

impl EntityQueryService {
    /// Consult the spatial index to narrow `selection.ids` before the store
    /// runs, and drop `selection.geo` when the index produced an authoritative
    /// candidate set.
    ///
    /// This is not a residual filter in reverse: the index *applies* the
    /// clause 4.10 relation, it does not decline it, so the query the store
    /// receives is equivalent to the one the client wrote.
    ///
    /// Falls through to [`SpatialNarrowing::Query`] with the query unchanged when
    /// it is not geo-accelerated: no geo filter, a conjunctive type expression,
    /// a cold partition, or a reference geometry the index cannot use. The store
    /// then applies the full `georel`. A warm partition that matches nothing
    /// yields [`SpatialNarrowing::EmptyResult`].
    pub(super) fn spatial_prefilter(&self, compiled: &Arc<CompiledEntityQuery>) -> SpatialNarrowing {
        let Some(ref geo_query) = compiled.selection.geo else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };
        let Some(ref type_expr) = compiled.selection.type_expr else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };
        let Some(type_iris) = flatten_or_type_iris(type_expr) else {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        };
        let geoproperty_iri = &geo_query.geoproperty_iri;
        let reference: &geo::Geometry<f64> = geo_query.geometry.as_ref();

        let mut any_warm = false;
        let mut union: HashSet<Urn> = HashSet::new();
        for type_iri in type_iris {
            // clone: the spatial key owns its IRIs and outlives the borrowed query
            let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty_iri.clone());
            let Some(candidates) = self.spatial.query(&key, &geo_query.georel, reference) else {
                // A cold partition for any one type would otherwise contribute
                // an empty candidate set, which is indistinguishable from that
                // type having no match. Let the store apply the full `georel`.
                return SpatialNarrowing::Query(Arc::clone(compiled));
            };
            any_warm = true;
            union.extend(candidates);
        }
        if !any_warm {
            return SpatialNarrowing::Query(Arc::clone(compiled));
        }

        let candidate_ids: Vec<Uri> = union.into_iter().map(Uri::Urn).collect();

        let final_ids: Vec<Uri> = if compiled.selection.ids.is_empty() {
            candidate_ids
        } else {
            let requested: HashSet<&str> = compiled.selection.ids.iter().map(AsRef::as_ref).collect();
            candidate_ids.into_iter().filter(|id| requested.contains(AsRef::<str>::as_ref(id))).collect()
        };

        // A warm index that narrowed to no candidates is an authoritative empty
        // answer. It must not become `selection.ids = []`, which the statement
        // builder reads as unconstrained (see [`SpatialNarrowing::EmptyResult`]).
        if final_ids.is_empty() {
            return SpatialNarrowing::EmptyResult;
        }

        let mut narrowed = (**compiled).clone(); // clone: the cached query is shared and must not be mutated in place
        narrowed.selection.ids = final_ids;
        narrowed.selection.geo = None;
        tracing::debug!(
            "spatial prefilter hit: {} candidate(s) for geoQ={:?}",
            narrowed.selection.ids.len(),
            geo_query.georel
        );
        SpatialNarrowing::Query(Arc::new(narrowed))
    }
}
