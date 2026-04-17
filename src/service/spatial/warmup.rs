//! Startup warm-up for the entity spatial index.
//!
//! Eagerly bulk-loads the `location` geoproperty for every active entity in
//! the tenant so the very first georel=… query against `location` already
//! hits the R-tree. Other geoproperties (e.g. `observationSpace`) remain
//! cold and are populated lazily on first use.

use crate::{
    core::error::AndromedaError,
    jsonld::vocab,
    service::spatial::{
        SpatialKey,
        entity_index::{SpatialEntityIndex, SpatialEntry},
    },
    tenancy::types::TenantRef,
};
use iref::IriBuf;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tracing::{debug, info, warn};
use urn::Urn;

/// Load every entity with a non-null `location` into the spatial index,
/// grouped and bulk-loaded per `(tenant, type, location)` partition.
pub async fn warmup_location(index: &Arc<SpatialEntityIndex>, pool: &PgPool, tenant: &TenantRef) -> Result<WarmupStats, AndromedaError> {
    let started = Instant::now();
    let rows = sqlx::query_as::<_, (String, Vec<String>, String)>(
        "SELECT id, types, ST_AsGeoJSON(location)::TEXT AS geojson FROM entities WHERE location IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AndromedaError::internal(format!("spatial warmup query failed: {}", e)))?;

    let Ok(location_iri) = IriBuf::new(vocab::expanded::LOCATION.as_str().to_string()) else {
        return Err(AndromedaError::internal("spatial warmup: LOCATION IRI invalid"));
    };

    let mut per_type: HashMap<IriBuf, Vec<SpatialEntry>> = HashMap::new();

    for (id_raw, types, geojson_raw) in rows {
        let Ok(urn) = id_raw.parse::<Urn>() else {
            continue;
        };
        let Some(geom) = parse_geojson(&geojson_raw) else {
            continue;
        };
        for t in &types {
            let Ok(type_iri) = IriBuf::new(t.clone()) else {
                continue;
            };
            let Some(entry) = SpatialEntry::new(urn.clone(), geom.clone()) else {
                continue;
            };
            per_type.entry(type_iri).or_default().push(entry);
        }
    }

    let partition_count = per_type.len();
    let mut total_entries = 0usize;
    for (type_iri, entries) in per_type {
        total_entries += entries.len();
        let key = SpatialKey::new(tenant.clone(), type_iri, location_iri.clone());
        index.bulk_load(key, entries);
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

/// Periodic full rebuild of every known partition from the `entities` table.
///
/// Swaps each `Arc<RwLock<RTree>>` atomically via bulk-load, healing any
/// drift introduced by dropped [`tokio::sync::broadcast`] events or worker
/// restarts. Interval default: 1800s.
pub async fn periodic_rebuild(index: Arc<SpatialEntityIndex>, pool: PgPool, tenant: TenantRef, interval: std::time::Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await; // fire immediately -> skip the leading tick
    loop {
        ticker.tick().await;
        match warmup_location(&index, &pool, &tenant).await {
            Ok(stats) => debug!(
                "spatial periodic rebuild: {} partitions / {} entries",
                stats.partition_count, stats.total_entries
            ),
            Err(e) => warn!("spatial periodic rebuild failed: {}", e),
        }
    }
}

/// Result of a warmup or rebuild pass.
#[derive(Debug, Clone, Copy)]
pub struct WarmupStats {
    /// Number of `(tenant, type, location)` partitions populated.
    pub partition_count: usize,
    /// Total number of entries bulk-loaded across all partitions.
    pub total_entries: usize,
}

fn parse_geojson(raw: &str) -> Option<geo::Geometry<f64>> {
    let parsed: geojson::GeoJson = raw.parse().ok()?;
    match parsed {
        geojson::GeoJson::Geometry(g) => geo::Geometry::try_from(g).ok(),
        geojson::GeoJson::Feature(_) | geojson::GeoJson::FeatureCollection(_) => None,
    }
}
