//! Background worker that keeps [`SpatialEntityIndex`] in sync with
//! entity mutations broadcast on the `ChangeEvent` channel.
//!
//! Each event triggers a small targeted update to the R-tree: insert or
//! replace the new geometry, or remove the entity outright on delete. The
//! worker complements (does not replace) the periodic rebuild task, which
//! heals drift from missed events.

use crate::{
    domain::notification::{ChangeEvent, TriggerReason},
    jsonld::vocab,
    repository::entity::repo::EntityRepository,
    service::spatial::{SpatialKey, entity_index::SpatialEntityIndex},
    tenancy::types::TenantRef,
};
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};
use urn_rs::Urn;

/// Event subscriber that applies incremental updates to the entity spatial index.
pub struct SpatialSyncWorker {
    rx: broadcast::Receiver<ChangeEvent>,
    index: Arc<SpatialEntityIndex>,
    repo: EntityRepository,
    tenant: TenantRef,
}

impl SpatialSyncWorker {
    /// Construct a new worker listening on `rx` and writing into `index`.
    pub fn new(rx: broadcast::Receiver<ChangeEvent>, index: Arc<SpatialEntityIndex>, pool: &PgPool, tenant: TenantRef) -> SpatialSyncWorker {
        SpatialSyncWorker {
            rx,
            index,
            repo: EntityRepository::new(pool),
            tenant,
        }
    }

    /// Drive the worker until the broadcast channel is closed.
    pub async fn run(mut self) {
        loop {
            match self.rx.recv().await {
                Ok(event) => self.handle(event).await,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("spatial_sync lagged by {} events -- periodic rebuild will heal drift", n);
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
        // Fetch the current expanded payload to read the entity's geometries.
        let id_str = urn.to_string();
        let payload = match self.repo.get(&id_str).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                self.index.remove(urn);
                return;
            }
            Err(e) => {
                debug!("spatial_sync: fetch failed for {}: {}", urn, e);
                return;
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
            let mut v: Vec<String> = event.changed_attributes.clone();
            let location = vocab::expanded::LOCATION.as_str().to_string();
            if !v.contains(&location) {
                v.push(location);
            }
            v
        };

        for iri in candidate_iris {
            let Some(geojson_str) = extract_geoproperty_geojson(obj, &iri) else {
                continue;
            };
            let Some(wkb) = parse_wkb(&geojson_str) else {
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
        .and_then(|inst| inst.get(vocab::expanded::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"));

    match geojson_value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(val) if val.is_object() => Some(val.to_string()),
        Some(_) | None => None,
    }
}

fn parse_wkb(raw: &str) -> Option<crate::domain::geometry::Wkb> {
    let value: Value = serde_json::from_str(raw).ok()?;
    crate::domain::geometry::Wkb::from_geojson_value(&value).ok()
}
