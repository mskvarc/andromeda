//! Rebuilding the in-memory entity spatial index from typed store snapshots.

use crate::{error::EntityError, port::spatial_snapshot::EntitySpatialSnapshotSource};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::{debug, info, warn};

/// Result of a warm-up or rebuild pass.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct WarmupStats {
    /// Number of `(tenant, type, location)` partitions populated.
    pub partition_count: usize,
    /// Total number of entries bulk-loaded across all partitions.
    pub total_entries: usize,
}

/// Rebuild every default `location` partition from one typed snapshot source.
///
/// # Errors
///
/// Reports [`EntityError::Persistence`] when the snapshot source cannot be
/// read, and [`EntityError::Internal`] when the core `location` term is not a
/// well-formed attribute name.
pub async fn reconcile_location(
    index: &Arc<SpatialEntityIndex>,
    source: &dyn EntitySpatialSnapshotSource,
    tenant: &TenantRef,
) -> Result<WarmupStats, EntityError> {
    let started = Instant::now();
    // A live write landing after the snapshot starts has a later generation and must survive the replacement.
    let snapshot_generation = index.current_generation();
    let snapshots = source.location_snapshots().await?;
    let location: AttributeName = expanded_properties::LOCATION
        .as_str()
        .parse()
        .map_err(|error| EntityError::internal(format!("invalid default location attribute name: {error}")))?;
    let mut per_type: HashMap<IriBuf, Vec<SpatialEntry>> = HashMap::new();

    for snapshot in snapshots {
        let entity_urn = match snapshot.entity_id {
            Uri::Urn(entity_urn) => entity_urn,
            Uri::Url(entity_url) => {
                warn!(entity = %entity_url, "spatial warmup skipped a URL identifier unsupported by the current spatial index");
                continue;
            }
            Uri::Iri(entity_iri) => {
                warn!(entity = %entity_iri, "spatial warmup skipped an IRI identifier unsupported by the current spatial index");
                continue;
            }
        };
        for entity_type in snapshot.entity_types {
            let type_iri = match IriBuf::new(entity_type.to_string()) {
                Ok(type_iri) => type_iri,
                Err(error) => {
                    warn!(entity_type = %entity_type, %error, "spatial warmup skipped an unexpanded entity type");
                    continue;
                }
            };
            let Some(entry) = SpatialEntry::new(entity_urn.clone(), snapshot.geometry.clone()) else {
                // clone: each entity type owns one independently indexed entry.
                continue;
            };
            per_type.entry(type_iri).or_default().push(entry);
        }
    }

    let partition_count = per_type.len();
    let total_entries = per_type.values().map(Vec::len).sum();
    for (type_iri, entries) in per_type {
        index.bulk_load(
            SpatialKey::new(
                tenant.clone(), // clone: each partition key owns its tenant identifier.
                type_iri,
                location.clone(), // clone: each partition key owns the default geoproperty name.
            ),
            entries,
            snapshot_generation,
        );
    }

    let elapsed = started.elapsed();
    info!(target: "spatial.warmup", partition_count, total_entries, elapsed_ms = elapsed.as_millis(), "spatial warmup loaded the default location geoproperty");
    debug!(?elapsed, "spatial warmup complete");
    Ok(WarmupStats {
        partition_count,
        total_entries,
    })
}
