//! Narrowing a geo-query to an identifier set through the in-memory spatial
//! index.

use crate::service::entity::query::EntityQueryService;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_spatial_index::key::{SpatialKey, flatten_or_type_iris};
use std::{collections::HashSet, sync::Arc};
use urn_rs::Urn;

impl EntityQueryService {
    /// Consult the spatial index to narrow `selection.ids` before the store
    /// runs, and drop `selection.geo` when the index produced an authoritative
    /// candidate set.
    ///
    /// This is not a residual filter in reverse: the index *applies* the
    /// clause 4.10 relation, it does not decline it, so the query the store
    /// receives is equivalent to the one the client wrote.
    ///
    /// Falls through unchanged when the query is not geo-accelerated: no geo
    /// filter, a conjunctive type expression, a cold partition, or a reference
    /// geometry the index cannot use. The store then applies the full `georel`.
    pub(super) fn spatial_prefilter(&self, compiled: &Arc<CompiledEntityQuery>) -> Arc<CompiledEntityQuery> {
        let Some(ref geo_query) = compiled.selection.geo else {
            return Arc::clone(compiled);
        };
        let Some(ref type_expr) = compiled.selection.type_expr else {
            return Arc::clone(compiled);
        };
        let Some(type_iris) = flatten_or_type_iris(type_expr) else {
            return Arc::clone(compiled);
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
                // type having no match.
                return Arc::clone(compiled);
            };
            any_warm = true;
            union.extend(candidates);
        }
        if !any_warm {
            return Arc::clone(compiled);
        }

        let candidate_ids: Vec<Uri> = union.into_iter().map(Uri::Urn).collect();

        let final_ids: Vec<Uri> = if compiled.selection.ids.is_empty() {
            candidate_ids
        } else {
            let requested: HashSet<&str> = compiled.selection.ids.iter().map(AsRef::as_ref).collect();
            candidate_ids.into_iter().filter(|id| requested.contains(AsRef::<str>::as_ref(id))).collect()
        };

        let mut narrowed = (**compiled).clone(); // clone: the cached query is shared and must not be mutated in place
        narrowed.selection.ids = final_ids;
        narrowed.selection.geo = None;
        tracing::debug!(
            "spatial prefilter hit: {} candidate(s) for geoQ={:?}",
            narrowed.selection.ids.len(),
            geo_query.georel
        );
        Arc::new(narrowed)
    }
}
