//! Background worker that keeps [`SpatialEntityIndex`] in sync with
//! entity mutations broadcast on the `ChangeEvent` channel.
//!
//! Each event triggers a small targeted update to the R-tree: insert or
//! replace the new geometry, or remove the entity outright on delete. The
//! worker complements (does not replace) the periodic rebuild task, which
//! heals drift from missed events.
//!
//! The worker is payload-aware: when `ChangeEvent::current_payload` is set
//! (every service write path populates it), the post-mutation expanded
//! entity is read directly off the event and no database round-trip is
//! performed. This keeps high-throughput batch ingest free of the
//! per-event refetch that would otherwise contend with the batch writer
//! itself. Events without a payload (background expiration sweeps) still
//! fall back to fetching from the entity store.

use crate::{
    repository::entity::repo::EntityRepository,
    service::spatial::{SpatialKey, entity_index::SpatialEntityIndex},
    tenancy::types::TenantRef,
};
use andromeda_domain::notification::{ChangeEvent, TriggerReason};
use andromeda_geometry::geometry::Geometry;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use urn_rs::Urn;

/// Event subscriber that applies incremental updates to the entity spatial index.
pub struct SpatialSyncWorker {
    rx: broadcast::Receiver<ChangeEvent>,
    index: Arc<SpatialEntityIndex>,
    repo: EntityRepository,
    tenant: TenantRef,
    lag_log_interval: Duration,
}

impl SpatialSyncWorker {
    /// Construct a new worker listening on `rx` and writing into `index`.
    ///
    /// `lag_log_interval` is the minimum spacing between aggregated lag-warning
    /// log lines: skipped events are counted between summaries so a single
    /// `WARN` per interval carries the total drift, instead of amplifying the
    /// lag with one log per overrun.
    pub fn new(
        rx: broadcast::Receiver<ChangeEvent>,
        index: Arc<SpatialEntityIndex>,
        pool: &PgPool,
        tenant: TenantRef,
        lag_log_interval: Duration,
    ) -> SpatialSyncWorker {
        SpatialSyncWorker {
            rx,
            index,
            repo: EntityRepository::new(pool),
            tenant,
            lag_log_interval,
        }
    }

    /// Drive the worker until the broadcast channel is closed.
    ///
    /// After each successful `recv().await` the loop drains any further events
    /// available on the broadcast channel via `try_recv()` and applies them as
    /// one logical batch. This lets the consumer catch up after a burst
    /// without yielding to the executor between events. R-tree updates are
    /// idempotent so repeated events for the same id within one drain are
    /// safe.
    pub async fn run(mut self) {
        let mut lag_throttle = LagThrottle::new(self.lag_log_interval);
        loop {
            match self.rx.recv().await {
                Ok(event) => {
                    self.handle(event).await;
                    loop {
                        match self.rx.try_recv() {
                            Ok(event) => self.handle(event).await,
                            Err(broadcast::error::TryRecvError::Empty) => break,
                            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                lag_throttle.record(n);
                            }
                            Err(broadcast::error::TryRecvError::Closed) => return,
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    lag_throttle.record(n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn handle(&self, event: ChangeEvent) {
        let urn = match event.entity_id.to_string().parse::<Urn>() {
            Ok(u) => u,
            Err(e) => {
                debug!("spatial_sync: entity id '{}' is not a URN: {}", event.entity_id, e);
                return;
            }
        };

        match event.trigger_reason {
            TriggerReason::EntityDeleted => {
                self.index.remove(&urn);
            }
            TriggerReason::EntityCreated | TriggerReason::EntityUpdated | TriggerReason::AttributeCreated | TriggerReason::AttributeUpdated => {
                self.sync_entity(&urn, &event).await;
            }
            TriggerReason::AttributeDeleted => {
                // An attribute delete may remove the geoproperty entirely. The
                // simplest sound response is to remove all partitions the
                // entity participated in; a subsequent fetch during sync_entity
                // will reinsert any surviving geoproperties.
                self.index.remove(&urn);
                self.sync_entity(&urn, &event).await;
            }
        }
    }

    async fn sync_entity(&self, urn: &Urn, event: &ChangeEvent) {
        // Prefer the in-memory post-mutation payload carried on the event
        // (populated by every service write path). Falling back to a DB
        // fetch is reserved for events emitted by background workers that
        // only know the entity id (e.g. expiration sweeps), keeping the
        // hot batch-ingest path query-free.
        let payload: Arc<Value> = match &event.current_payload {
            Some(p) => Arc::clone(p),
            None => {
                let id_str = urn.to_string();
                match self.repo.get(&id_str).await {
                    Ok(Some(v)) => Arc::new(v),
                    Ok(None) => {
                        self.index.remove(urn);
                        return;
                    }
                    Err(e) => {
                        debug!("spatial_sync: fetch failed for {}: {}", urn, e);
                        return;
                    }
                }
            }
        };

        // Extract every geoproperty present on the entity. We use the default
        // `location` IRI plus any expanded IRI explicitly listed in
        // `changed_attributes` that carries a GeoJSON value.
        let payload_obj = unwrap_to_object(&payload);
        let Some(obj) = payload_obj else {
            return;
        };

        // Remove existing entries before reinserting the latest ones.
        self.index.remove(urn);

        let candidate_iris: Vec<String> = {
            // clone: the index keys by owned IRI strings and the event is borrowed
            let mut v: Vec<String> = event.changed_attributes.iter().map(ToString::to_string).collect();
            let location = expanded_properties::LOCATION.as_str().to_string();
            if !v.contains(&location) {
                v.push(location);
            }
            v
        };

        for iri in candidate_iris {
            let Some(geojson_str) = extract_geoproperty_geojson(obj, &iri) else {
                continue;
            };
            let Some(wkb) = parse_geometry(&geojson_str) else {
                continue;
            };
            let Ok(geoproperty_iri) = IriBuf::new(iri) else {
                continue;
            };

            for type_iri in &event.entity_types {
                let Ok(type_iri) = IriBuf::new(type_iri.clone()) else {
                    continue;
                };
                let key = SpatialKey::new(self.tenant.clone(), type_iri, geoproperty_iri.clone());
                self.index.insert(key, urn.clone(), wkb.clone());
            }
        }
    }
}

/// Aggregates broadcast-receiver `Lagged(n)` overruns and emits at most one
/// warning per `interval`, carrying the running total of skipped events
/// since the previous summary. Tracing emissions during a burst would
/// themselves contend on the worker thread and amplify the lag they
/// describe — this collapses every overrun into one summary line per
/// window.
struct LagThrottle {
    interval: Duration,
    pending: u64,
    last_emit: Instant,
}

impl LagThrottle {
    fn new(interval: Duration) -> LagThrottle {
        // Backdate the anchor so the first overrun emits immediately.
        let last_emit = Instant::now().checked_sub(interval).unwrap_or_else(Instant::now);
        LagThrottle {
            interval,
            pending: 0,
            last_emit,
        }
    }

    fn record(&mut self, skipped: u64) {
        self.pending = self.pending.saturating_add(skipped);
        let now = Instant::now();
        if now.duration_since(self.last_emit) >= self.interval {
            warn!(
                skipped = self.pending,
                interval_secs = self.interval.as_secs(),
                "spatial_sync lagged -- periodic rebuild will heal drift"
            );
            self.pending = 0;
            self.last_emit = now;
        }
    }
}

fn unwrap_to_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        Value::Object(obj) => Some(obj),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn extract_geoproperty_geojson(obj: &serde_json::Map<String, Value>, iri: &str) -> Option<String> {
    let prop_value = obj.get(iri)?;
    let geojson_value = prop_value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|inst| inst.get(expanded_properties::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"));

    match geojson_value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(val) if val.is_object() => Some(val.to_string()),
        Some(_) | None => None,
    }
}

fn parse_geometry(raw: &str) -> Option<Geometry> {
    let value: Value = serde_json::from_str(raw).ok()?;
    Geometry::from_geojson_value(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::spatial::SpatialKey;
    use andromeda_geometry::geo_rel::GeoRel;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use geo::{Coord, LineString, Polygon};
    use iri_rs::iri;
    use serde_json::json;
    use sqlx::PgPool;

    const LOCATION_IRI: &str = "https://uri.etsi.org/ngsi-ld/location";
    const TYPE_IRI: &str = "https://example.org/Sensor";
    const VALUE_IRI: &str = "https://uri.etsi.org/ngsi-ld/hasValue";

    fn lazy_pool() -> PgPool {
        // connect_lazy never opens a connection; the test exercises only the
        // payload-aware path, which never reaches the repository.
        PgPool::connect_lazy("postgres://localhost/nonexistent").expect("connect_lazy should not fail")
    }

    fn entity_payload(id: &str, lon: f64, lat: f64) -> Value {
        json!({
            "@id": id,
            "@type": [TYPE_IRI],
            LOCATION_IRI: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                VALUE_IRI: [{
                    "@type": "https://purl.org/geojson/vocab#Feature",
                    "@value": format!("{{\"type\":\"Point\",\"coordinates\":[{lon},{lat}]}}")
                }]
            }]
        })
    }

    fn create_event(id: &str, lon: f64, lat: f64) -> ChangeEvent {
        let uri: Uri = id.parse().expect("valid URI");
        let payload = entity_payload(id, lon, lat);
        ChangeEvent::new(uri, vec![TYPE_IRI.parse().expect("valid test type name")], TriggerReason::EntityCreated)
            .with_changed_attributes(vec![LOCATION_IRI.parse().expect("valid test attribute name")])
            .with_current_payload(Arc::new(payload))
    }

    fn delete_event(id: &str) -> ChangeEvent {
        let uri: Uri = id.parse().expect("valid URI");
        ChangeEvent::new(uri, vec![TYPE_IRI.parse().expect("valid test type name")], TriggerReason::EntityDeleted)
    }

    fn world_bbox() -> geo::Geometry<f64> {
        let ring = LineString::from(vec![
            Coord { x: -180.0, y: -90.0 },
            Coord { x: 180.0, y: -90.0 },
            Coord { x: 180.0, y: 90.0 },
            Coord { x: -180.0, y: 90.0 },
            Coord { x: -180.0, y: -90.0 },
        ]);
        geo::Geometry::Polygon(Polygon::new(ring, Vec::new()))
    }

    #[tokio::test]
    async fn worker_applies_bursted_events_and_honours_delete_after_create() {
        let index = Arc::new(SpatialEntityIndex::new());
        let (tx, rx) = broadcast::channel::<ChangeEvent>(16);

        let worker = SpatialSyncWorker::new(rx, Arc::clone(&index), &lazy_pool(), TenantRef::UnnamedDefault, Duration::from_secs(60));
        let driver = tokio::spawn(worker.run());

        for i in 0..10 {
            let id = format!("urn:ngsi-ld:Sensor:{i}");
            tx.send(create_event(&id, f64::from(i), 0.0)).expect("create event sent");
        }
        // Delete one of the created entities; the worker must process the
        // delete after the create even when both arrive within a single drain
        // iteration.
        tx.send(delete_event("urn:ngsi-ld:Sensor:3")).expect("delete event sent");

        drop(tx);
        driver.await.expect("worker terminates cleanly");

        let key = SpatialKey::new(
            TenantRef::UnnamedDefault,
            iri_rs::IriBuf::from(iri!("https://example.org/Sensor")),
            iri_rs::IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/location")),
        );
        let candidates = index
            .query(&key, &GeoRel::Intersects, &world_bbox())
            .expect("partition should be warm after creates");
        pretty_assertions::assert_eq!(candidates.len(), 9);
        let deleted: Urn = "urn:ngsi-ld:Sensor:3".parse().expect("valid URN");
        assert!(!candidates.contains(&deleted), "delete event should remove urn:ngsi-ld:Sensor:3");
    }
}
