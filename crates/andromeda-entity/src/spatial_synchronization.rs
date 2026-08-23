//! Incremental synchronization policy for the resident entity spatial index.

use crate::port::reader::EntityReader;
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{expanded::geo_property::geo_property_instance_geometries, notification::NotificationTrigger};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_spatial_index::{entity::index::SpatialEntityIndex, key::SpatialKey};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
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
    pub async fn apply(&self, change: ContextChangeEvent) {
        if change.target.snapshot().is_some() {
            return;
        }
        let ContextChangeEvent { target, event } = change;
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
            | NotificationTrigger::AttributeUpdated => self.synchronize_entity(&target, &urn, &event).await,
            NotificationTrigger::AttributeDeleted => {
                self.index.remove(&urn);
                self.synchronize_entity(&target, &urn, &event).await;
            }
        }
    }

    async fn synchronize_entity(&self, target: &ContextTarget, urn: &Urn, event: &ChangeEvent) {
        let payload = match &event.current_payload {
            Some(payload) => Arc::clone(payload),
            None => {
                let identifier = Uri::from(urn.clone()); // clone: the fallback read owns a URI while the index retains the URN.
                match self.reader.get(&target, &identifier).await {
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
        self.index.remove(urn);
        // clone: the loop below hands each partition key its own copy
        let mut candidates: Vec<AttributeName> = event.changed_attributes.to_vec();
        match expanded_properties::LOCATION.as_str().parse::<AttributeName>() {
            Ok(location) if !candidates.contains(&location) => candidates.push(location),
            Ok(_already_present) => {}
            Err(error) => debug!(%error, "spatial synchronization could not name the default location geoproperty"),
        }
        for geoproperty in candidates {
            // Clause 4.10: any instance of the GeoProperty meeting a restriction
            // matches, so every `datasetId` instance is indexed. Reading them
            // through the model's one reader is what keeps the index and the
            // residual filter agreeing on what an entity's geometries are.
            let geometries: Vec<Geometry> = geo_property_instance_geometries(&payload, geoproperty.as_str()).collect();
            if geometries.is_empty() {
                continue;
            }
            for entity_type in &event.entity_types {
                let Ok(type_iri) = IriBuf::new(entity_type.as_str().to_owned()) else {
                    continue;
                };
                self.index.insert_all(
                    SpatialKey::new(self.tenant.clone(), type_iri, geoproperty.clone()), // clone: each partition key independently owns its tenant and geoproperty.
                    urn.clone(),                                                         // clone: each type partition owns the entity identifier.
                    geometries.to_vec(),                                                 // clone: each type partition owns its geometries.
                );
            }
        }
    }
}
