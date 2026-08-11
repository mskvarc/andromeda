//! Incremental synchronization policy for the resident entity spatial index.

use crate::port::reader::EntityReader;
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;
use urn_rs::Urn;

/// Applies committed entity changes to one tenant's resident spatial index.
pub struct SpatialSynchronizer {
    index: Arc<SpatialEntityIndex>,
    reader: Arc<dyn EntityReader>,
    tenant: TenantRef,
}

impl SpatialSynchronizer {
    /// Construct incremental synchronization over focused entity capabilities.
    pub const fn new(index: Arc<SpatialEntityIndex>, reader: Arc<dyn EntityReader>, tenant: TenantRef) -> SpatialSynchronizer {
        SpatialSynchronizer { index, reader, tenant }
    }

    /// Apply one committed change idempotently.
    pub async fn apply(&self, event: ChangeEvent) {
        let urn = match event.entity_id.to_string().parse::<Urn>() {
            Ok(urn) => urn,
            Err(error) => {
                debug!(entity_id = %event.entity_id, %error, "spatial synchronization skipped a non-URN entity");
                return;
            }
        };
        match event.trigger_reason {
            NotificationTrigger::EntityDeleted => self.index.remove(&urn),
            NotificationTrigger::EntityCreated
            | NotificationTrigger::EntityUpdated
            | NotificationTrigger::AttributeCreated
            | NotificationTrigger::AttributeUpdated => self.synchronize_entity(&urn, &event).await,
            NotificationTrigger::AttributeDeleted => {
                self.index.remove(&urn);
                self.synchronize_entity(&urn, &event).await;
            }
        }
    }

    async fn synchronize_entity(&self, urn: &Urn, event: &ChangeEvent) {
        let payload = match &event.current_payload {
            Some(payload) => Arc::clone(payload),
            None => {
                let identifier = Uri::from(urn.clone()); // clone: the fallback read owns a URI while the index retains the URN.
                match self.reader.get(&identifier).await {
                    Ok(Some(payload)) => Arc::new(payload),
                    Ok(None) => {
                        self.index.remove(urn);
                        return;
                    }
                    Err(error) => {
                        debug!(entity_id = %identifier, %error, "spatial synchronization fallback read failed");
                        return;
                    }
                }
            }
        };
        let Some(object) = expanded_object(&payload) else {
            return;
        };
        self.index.remove(urn);
        let mut candidates = event
            .changed_attributes
            .iter()
            .filter_map(|name| IriBuf::new(name.as_str().to_owned()).ok())
            .collect::<Vec<_>>();
        let location: IriBuf = expanded_properties::LOCATION.to_owned().into();
        if !candidates.contains(&location) {
            candidates.push(location);
        }
        for geoproperty in candidates {
            let Some(geometry) = geometry(object, geoproperty.as_str()) else {
                continue;
            };
            for entity_type in &event.entity_types {
                let Ok(type_iri) = IriBuf::new(entity_type.as_str().to_owned()) else {
                    continue;
                };
                self.index.insert(
                    SpatialKey::new(self.tenant.clone(), type_iri, geoproperty.clone()), // clone: each partition key independently owns its tenant and geoproperty.
                    urn.clone(),                                                         // clone: each type partition owns the entity identifier.
                    geometry.clone(),                                                    // clone: each type partition owns its geometry.
                );
            }
        }
    }
}

fn expanded_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Array(values) => values.first().and_then(Value::as_object),
        Value::Object(object) => Some(object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn geometry(object: &serde_json::Map<String, Value>, iri: &str) -> Option<Geometry> {
    let encoded = object
        .get(iri)?
        .as_array()?
        .first()?
        .get(expanded_properties::VALUE.as_str())?
        .as_array()?
        .first()?
        .get("@value")?;
    let value = match encoded {
        Value::String(encoded) => serde_json::from_str(encoded).ok()?,
        Value::Object(_) => encoded.clone(), // clone: geometry parsing consumes a value borrowed from the event payload.
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => return None,
    };
    // Lenient by design: this reads a payload the write path already validated
    // against clause 5.5.4, so a geometry that will not decode is a broker-side
    // surprise to be recorded and skipped, not a request to refuse. Skipping it
    // leaves the entity out of the spatial prefilter, which costs a store
    // round trip and never a wrong answer.
    match Geometry::from_geojson_value(&value) {
        Ok(geometry) => Some(geometry),
        Err(error) => {
            debug!(geoproperty = iri, %error, "Spatial synchronization skipped an undecodable stored geometry");
            None
        }
    }
}
