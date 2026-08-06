//! Filling the entity spatial index from the store.
//!
//! Eagerly bulk-loads the `location` geoproperty for every entity in the
//! tenant so the very first `georel` query against `location` already hits the
//! R-tree. Every other geoproperty stays cold and is populated lazily on first
//! use, because most deployments have exactly one.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::{debug, info, warn};
use urn_rs::Urn;

/// Result of a warm-up or rebuild pass.
#[derive(Debug, Clone, Copy)]
pub struct WarmupStats {
    /// Number of `(tenant, type, location)` partitions populated.
    pub partition_count: usize,
    /// Total number of entries bulk-loaded across all partitions.
    pub total_entries: usize,
}

/// Load every entity with a non-null `location` into the index, bulk-loading
/// one partition per entity type.
pub async fn warmup_location(index: &Arc<SpatialEntityIndex>, pool: &PgPool, tenant: &TenantRef) -> Result<WarmupStats, AndromedaError> {
    let started = Instant::now();
    // PostGIS hands the column back binary-encoded, so reading it as `bytea`
    // skips a GeoJSON round-trip per row.
    let rows = sqlx::query_as::<_, (String, Vec<String>, Vec<u8>)>("SELECT id, types, location::bytea FROM entities WHERE location IS NOT NULL")
        .fetch_all(pool)
        .await
        .map_err(|error| AndromedaError::internal(format!("spatial warmup query failed: {error}")))?;

    let Ok(location_iri) = IriBuf::new(expanded_properties::LOCATION.as_str().to_string()) else {
        return Err(AndromedaError::internal("spatial warmup: LOCATION IRI invalid"));
    };

    let mut per_type: HashMap<IriBuf, Vec<SpatialEntry>> = HashMap::new();

    for (id_raw, types, ewkb_bytes) in rows {
        let Ok(urn) = id_raw.parse::<Urn>() else {
            continue;
        };
        let Ok(geometry) = Ewkb::try_from(ewkb_bytes.as_slice()).and_then(|ewkb| Geometry::try_from(&ewkb)) else {
            warn!(entity = %id_raw, "spatial warmup: skipping entity with undecodable location EWKB");
            continue;
        };
        for entity_type in &types {
            let Ok(type_iri) = IriBuf::new(entity_type.clone()) else {
                // clone: the partition key owns its IRI
                continue;
            };
            let Some(entry) = SpatialEntry::new(urn.clone(), geometry.clone()) else {
                // clone: one entry per type the entity carries
                continue;
            };
            per_type.entry(type_iri).or_default().push(entry);
        }
    }

    let partition_count = per_type.len();
    let mut total_entries = 0usize;
    for (type_iri, entries) in per_type {
        total_entries += entries.len();
        // clone: one key per partition, all derived from the same tenant and geoproperty
        index.bulk_load(SpatialKey::new(tenant.clone(), type_iri, location_iri.clone()), entries);
    }

    let elapsed = started.elapsed();
    info!(
        target: "spatial.warmup",
        partition_count,
        total_entries,
        elapsed_ms = elapsed.as_millis() as u64,
        "spatial warmup: loaded location geoproperty"
    );
    debug!("spatial warmup complete in {:?}", elapsed);

    Ok(WarmupStats {
        partition_count,
        total_entries,
    })
}
