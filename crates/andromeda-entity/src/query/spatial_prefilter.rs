//! Narrowing a geo-query's default `location` instances through the in-memory
//! spatial index.

use crate::query::EntityQueryService;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_constraint::EntityConstraint,
    geo::{
        compiled_geo_query::CompiledGeoQuery,
        default_location_narrowing::{DefaultLocationNarrowing, LocationCandidates},
    },
    name_stage::Expanded,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_spatial_index::{
    entity::index::{Admitted, SpatialEntityIndex},
    key::{SpatialKey, flatten_or_type_iris},
};
use iri_rs::IriBuf;
use std::{collections::HashSet, sync::Arc};

/// The part of a query the index can answer: one geo filter over an
/// enumerable set of entity types.
struct Accelerable<'a> {
    /// The query's geo filter.
    geo: &'a CompiledGeoQuery,
    /// The query's one entity selector.
    selector: &'a EntityConstraint<Expanded>,
    /// The expanded types the selector reaches.
    type_iris: Vec<IriBuf>,
}

/// Whether the index can answer a query at all, before any index is consulted.
///
/// The index holds the default instance of `location` alone, keyed by a single
/// `(type, geoproperty)` pair, so it accelerates a `location` geo-query with
/// exactly one selector whose type selection is a type or a union of types. A
/// query on any other `GeoProperty`, a union of selectors (clause 5.2.33), a
/// conjunctive type selection or an untyped selector is handed to the store to
/// evaluate in full.
fn accelerable(compiled: &CompiledEntityQuery) -> Option<Accelerable<'_>> {
    let geo = compiled.selection.geo.as_ref().filter(|geo| geo.geoproperty.as_str() == LOCATION.as_str())?;
    let [selector] = compiled.selection.selectors.as_slice() else {
        return None;
    };
    let type_iris = flatten_or_type_iris(selector.type_selection.as_ref()?)?;
    Some(Accelerable { geo, selector, type_iris })
}

impl EntityQueryService {
    /// Consult the spatial index for the entities whose default `location`
    /// instance can satisfy the geo-query, and hand them to the store as a
    /// [`DefaultLocationNarrowing`].
    ///
    /// Clause 5.7.2.4 lets any instance of `location` match, and the index holds
    /// one per entity: the default instance, exactly as the store hoists it. Its
    /// answer therefore narrows the default-instance half of the relation only;
    /// the store evaluates every instance carrying a `datasetId` itself, so an
    /// entity whose only matching instance has one is never narrowed away,
    /// whatever the index holds. The index narrows only where it holds every
    /// entity's default instance, what [`SpatialAcceleration`] guarantees for
    /// the configured consistency mode, and what an unseeded scope declines to
    /// claim. A candidate that no longer matches is harmless: the store
    /// re-applies the clause 4.10 relation to every one.
    ///
    /// Returns the query unchanged when it is not geo-accelerable, when no index
    /// may narrow it, when the index declines the relation, a `near` with
    /// only a `minDistance`, which no envelope bounds, when more entities the
    /// request can name may meet it than a narrowing names
    /// ([`LocationCandidates::MAX`]), which the index knows at the first one
    /// past them, or for a Snapshot branch, which the live index does not
    /// describe; the store then evaluates every instance in full. A request
    /// that names its entities is answered by no other, so the index names
    /// and counts only those.
    ///
    /// [`SpatialAcceleration`]: crate::spatial_acceleration::SpatialAcceleration
    pub(crate) async fn spatial_prefilter(&self, target: &ContextTarget, compiled: &Arc<CompiledEntityQuery>) -> Arc<CompiledEntityQuery> {
        if target.snapshot().is_some() {
            return Arc::clone(compiled);
        }
        let Some(accelerable) = accelerable(compiled) else {
            return Arc::clone(compiled);
        };
        let Some(index) = self.spatial.index_for_query().await else {
            return Arc::clone(compiled);
        };
        let requested: HashSet<&Identifier> = accelerable.selector.ids.iter().collect();
        let admitted = if requested.is_empty() { Admitted::Every } else { Admitted::Among(&requested) };
        let Some(candidates) = self.candidates(index, &accelerable, admitted) else {
            return Arc::clone(compiled);
        };
        let Some(candidates) = LocationCandidates::new(candidates.into_iter().collect()) else {
            return Arc::clone(compiled);
        };
        tracing::debug!(
            "spatial prefilter hit: {} default-location candidate(s) for geoQ={:?}",
            candidates.len(),
            accelerable.geo.georel
        );
        let mut narrowed = (**compiled).clone(); // clone: the cached query is shared and must not be mutated in place
        narrowed.selection.default_location = DefaultLocationNarrowing::Candidates(candidates);
        Arc::new(narrowed)
    }

    /// The union of every type partition's candidates that `admitted` admits,
    /// at most [`LocationCandidates::MAX`] of them, or `None` when any one
    /// partition declines to answer or finds more than the union has room
    /// for.
    ///
    /// An entity of two of the types counts against each partition it is
    /// found in, so a union near the bound can decline where its distinct
    /// candidates would fit; the store then evaluates every instance, which
    /// answers the same.
    fn candidates(&self, index: &SpatialEntityIndex, accelerable: &Accelerable<'_>, admitted: Admitted<'_>) -> Option<HashSet<Identifier>> {
        let mut union: HashSet<Identifier> = HashSet::new();
        for type_iri in &accelerable.type_iris {
            let key = SpatialKey::new(self.tenant.clone(), type_iri.clone(), AttributeName::from(accelerable.geo.geoproperty.clone()));
            let room = LocationCandidates::MAX.saturating_sub(union.len());
            // A partition that declines for any one type would otherwise
            // contribute an empty candidate set, which is indistinguishable from
            // that type having no default instance in range. The store then
            // evaluates every instance in full.
            union.extend(index.query(&key, &accelerable.geo.georel, accelerable.geo.geometry.form(), admitted, room)?);
        }
        Some(union)
    }
}
