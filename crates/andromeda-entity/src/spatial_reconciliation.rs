//! Rebuilding the in-memory entity spatial index from a read of the store.

use crate::{
    error::EntityError,
    port::spatial_snapshot::{ChangeHorizon, EntitySpatialSnapshotSource},
    spatial_placement::LocationPlacement,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::info;

/// Result of a rebuild pass.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RebuildOutcome {
    /// Number of `(tenant, type, location)` partitions the store's read filled.
    pub partition_count: usize,
    /// Total number of entries the store's read placed across all partitions.
    pub total_entries: usize,
    /// The horizon the store's read was taken at.
    pub horizon: ChangeHorizon,
}

/// Rebuild every `location` partition of one tenant's index from the store,
/// and seed the scope so the index may narrow queries against it.
///
/// A write applied to the index while the store is being read keeps its state
/// over the read's: the generation is captured before the read, and
/// [`SpatialEntityIndex::rebuild`] keeps every entity mutated after it.
///
/// # Errors
///
/// Reports [`EntityError::Persistence`] when the store cannot be read.
pub async fn rebuild_location_index(
    index: &SpatialEntityIndex,
    source: &dyn EntitySpatialSnapshotSource,
    placement: &LocationPlacement,
) -> Result<RebuildOutcome, EntityError> {
    let started = Instant::now();
    let snapshot_generation = index.current_generation();
    let read = source.locations().await?;
    let mut partitions: HashMap<SpatialKey, Vec<SpatialEntry>> = HashMap::new();
    for location in read.locations {
        let entity_id = Arc::new(location.entity_id);
        for (key, geometry) in placement.of(&location.entity_types, location.location.as_ref().map(Geometry::form)) {
            // clone: a reference-count bump, every partition's entry shares the identifier
            partitions.entry(key).or_default().extend(SpatialEntry::new(Arc::clone(&entity_id), geometry));
        }
    }
    let partition_count = partitions.len();
    let total_entries = partitions.values().map(Vec::len).sum();
    index.rebuild(placement.tenant(), placement.geoproperty(), partitions, snapshot_generation);
    info!(target: "spatial.rebuild", partition_count, total_entries, elapsed_ms = started.elapsed().as_millis(), "spatial index rebuilt the default location geoproperty");
    Ok(RebuildOutcome {
        partition_count,
        total_entries,
        horizon: read.horizon,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::EntityError,
        port::spatial_snapshot::{ChangeHorizon, EntityLocation, EntitySpatialSnapshotSource, LocationRead},
        spatial_placement::LocationPlacement,
        spatial_reconciliation::rebuild_location_index,
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geo_rel::GeoRel, geometry::Geometry, position::Position};
    use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
    use andromeda_tenancy::tenant_ref::TenantRef;
    use futures::future::BoxFuture;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    /// A store holding one located vehicle.
    struct OneVehicle;

    impl EntitySpatialSnapshotSource for OneVehicle {
        fn locations<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async {
                Ok(LocationRead {
                    locations: vec![EntityLocation {
                        entity_id: "urn:ngsi-ld:Vehicle:stored".parse().expect("a well-formed URI"),
                        entity_types: vec!["https://example.org/Vehicle".parse().expect("a well-formed type")],
                        location: Some(Geometry::new(GeometryForm::Point(
                            Position::try_new(0.0, 0.0, None).expect("a test position lies on the CRS84 axes"),
                        ))),
                    }],
                    horizon: ChangeHorizon::new(7),
                })
            })
        }

        fn locations_changed_since<'life0, 'async_trait>(&'life0 self, horizon: ChangeHorizon) -> BoxFuture<'async_trait, Result<LocationRead, EntityError>>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                Ok(LocationRead {
                    locations: Vec::new(),
                    horizon,
                })
            })
        }
    }

    #[tokio::test]
    async fn a_rebuild_seeds_the_scope_with_what_the_store_holds() {
        let index = SpatialEntityIndex::new();
        let placement = LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name");
        let key = SpatialKey::new(
            TenantRef::UnnamedDefault,
            IriBuf::new("https://example.org/Vehicle".to_owned()).expect("a well-formed IRI"),
            placement.geoproperty().clone(),
        );
        let reference = GeometryForm::Point(Position::try_new(0.0, 0.0, None).expect("a test position lies on the CRS84 axes"));
        assert!(index.query(&key, &GeoRel::Intersects, &reference).is_none());

        let outcome = rebuild_location_index(&index, &OneVehicle, &placement).await.expect("the store is readable");

        assert_eq!((outcome.partition_count, outcome.total_entries, outcome.horizon), (1, 1, ChangeHorizon::new(7)));
        assert_eq!(
            index.query(&key, &GeoRel::Intersects, &reference),
            Some(HashSet::from(["urn:ngsi-ld:Vehicle:stored".parse().expect("a well-formed URI")]))
        );
    }
}
