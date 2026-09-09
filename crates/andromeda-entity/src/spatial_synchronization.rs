//! Incremental synchronization policy for the resident entity spatial index.

use crate::port::reader::EntityReader;
use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    expanded::geo_property::{geo_property_instance_geometries, geo_property_iris},
    notification::NotificationTrigger,
};
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
        let payload = if let Some(payload) = &event.current_payload {
            Arc::clone(payload)
        } else {
            let identifier = Uri::from(urn.clone()); // clone: the fallback read owns a URI while the index retains the URN.
            match self.reader.get(target, &identifier).await {
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
        };
        self.index.remove(urn);
        // The removal above clears the entity from *every* partition, so every
        // GeoProperty it conveys has to be written back -- not just the ones
        // this event named. Rebuilding only the changed attributes left a hole
        // in each partition the event did not mention, and nothing closed it:
        // `SpatialReconciliation` rebuilds `location` alone, so a partition for
        // any other GeoProperty stayed warm and short an entity for as long as
        // the broker ran. `spatial_prefilter` reads a warm partition as
        // authoritative, so the hole surfaced as a geoquery answering 200 with
        // an empty array for an entity sitting on the reference point.
        for iri in geo_property_iris(&payload) {
            let geoproperty = match iri.parse::<AttributeName>() {
                Ok(geoproperty) => geoproperty,
                Err(error) => {
                    debug!(%iri, %error, "spatial synchronization skipped a geoproperty whose IRI is not an attribute name");
                    continue;
                }
            };
            // Clause 4.10: any instance of the GeoProperty meeting a restriction
            // matches, so every `datasetId` instance is indexed. Reading them
            // through the model's one reader is what keeps the index and the
            // residual filter agreeing on what an entity's geometries are.
            let geometries: Vec<Geometry> = geo_property_instance_geometries(&payload, iri).collect();
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
                    geometries.clone(),                                                  // clone: each type partition owns its geometries.
                );
            }
        }
    }
}
