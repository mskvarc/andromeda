//! One entity of an inbound notification, read into the typed model under the
//! sender's context and re-spelled under the local subscription's.
//!
//! The sender spelled its entity under the context of the copy it holds, so the
//! entity is expanded once under that context into the written document the
//! local filters read, and that document is compacted under the local
//! subscription's context for delivery (clause 5.8.6). A written document,
//! unlike an entity state, holds the `urn:ngsi-ld:null` a source notifies a
//! deleted attribute with (clause 5.8.6), so the deletion travels on.
//!
//! The entity is read as a notification spells it: "only the name of the
//! attribute as key and the URI "urn:ngsi-ld:null" as value" report a deleted
//! attribute of any family (clause 5.8.6), so a first-level Null stays the
//! deletion of the member rather than becoming the concise value of a
//! Property, and is relayed as the bare Null it arrived as.

use andromeda_ngsi_ld_compaction::{compaction_error::CompactionError, entity_compactor::EntityCompactor};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, error::ExpansionError, normalization::Normalization};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

/// Why one entity of an inbound notification cannot be re-assembled.
#[derive(Debug, Error)]
pub(crate) enum ReceivedEntityRefusal {
    /// The entity is no NGSI-LD entity in the compact model.
    #[error("the entity does not decode: {0}")]
    Decode(#[source] serde_json::Error),
    /// The entity does not expand under the sender's context.
    #[error("the entity does not expand under the sender's context: {0}")]
    Expansion(#[source] ExpansionError),
    /// The entity does not compact under the local subscription's context.
    #[error("the entity does not compact under the local context: {0}")]
    Compaction(#[source] CompactionError),
}

/// The entity-level clause 4.8 system attributes a source notified, which a
/// written document does not carry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SystemTimes {
    /// The entity's `createdAt`.
    created: Option<DateTime<Utc>>,
    /// The entity's `modifiedAt`.
    modified: Option<DateTime<Utc>>,
    /// The entity's `deletedAt`, on a deletion notification.
    deleted: Option<DateTime<Utc>>,
}

impl SystemTimes {
    /// The system attributes `entity` carries.
    const fn of(entity: &Entity) -> SystemTimes {
        SystemTimes {
            created: entity.created_at,
            modified: entity.modified_at,
            deleted: entity.deleted_at,
        }
    }

    /// Put the system attributes back on the re-spelled `entity`.
    pub(crate) const fn restore(self, entity: &mut Entity) {
        entity.created_at = self.created;
        entity.modified_at = self.modified;
        entity.deleted_at = self.deleted;
    }
}

/// One received entity in the typed model.
#[derive(Debug)]
pub(crate) struct ReceivedEntity {
    /// The entity as the written document it expands to.
    pub(crate) document: EntityDocument,
    /// The entity-level system attributes it was notified with.
    pub(crate) system_times: SystemTimes,
}

impl ReceivedEntity {
    /// Read one entity of an inbound notification under the sender's context.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedEntityRefusal::Decode`] for a value that is no
    /// compact entity and [`ReceivedEntityRefusal::Expansion`] for one that
    /// does not expand.
    pub(crate) async fn read(value: &Value, inbound: &EntityExpansion<'_>) -> Result<ReceivedEntity, ReceivedEntityRefusal> {
        let received: Entity = deserialize_exact(value).map_err(ReceivedEntityRefusal::Decode)?;
        let document = inbound
            .entity(&received, Normalization::MemberDeletions)
            .await
            .map_err(ReceivedEntityRefusal::Expansion)?;
        Ok(ReceivedEntity {
            document,
            system_times: SystemTimes::of(&received),
        })
    }

    /// The entity spelled under the local subscription's context, the one
    /// `local` compacts against.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedEntityRefusal::Compaction`] when the document does not
    /// compact and [`ReceivedEntityRefusal::Decode`] when the compact result is
    /// no entity.
    pub(crate) async fn respelled(&self, local: &EntityCompactor<'_>) -> Result<Entity, ReceivedEntityRefusal> {
        let compacted = local.document(&self.document).await.map_err(ReceivedEntityRefusal::Compaction)?;
        let mut entity: Entity = deserialize_exact(&compacted).map_err(ReceivedEntityRefusal::Decode)?;
        self.system_times.restore(&mut entity);
        Ok(entity)
    }
}

#[cfg(test)]
mod tests {
    use crate::received_entity::ReceivedEntity;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_render::entity::EntityRenderer;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// `entity` read as a source notified it and rendered for a local
    /// subscriber in `format`, both under the core context alone.
    async fn relayed(entity: &Value, format: ResponseFormat) -> Value {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let inbound = EntityExpansion::prepare(&engine, &context).await.expect("the core context processes");
        let received = ReceivedEntity::read(entity, &inbound).await.expect("the notified entity reads");
        let context_compactor = engine.compactor(&context);
        let respelled = received
            .respelled(&EntityCompactor::new(&context_compactor))
            .await
            .expect("the entity compacts");
        EntityRenderer::new(&respelled).with_mode(format).render().expect("the entity renders")
    }

    /// Clause 5.8.6: a source reports a deleted attribute of any family as
    /// "only the name of the attribute as key and the URI "urn:ngsi-ld:null"
    /// as value", and the relayed copy reports it the same way in every
    /// representation, never as a Property whose value is the Null.
    #[tokio::test]
    async fn a_notified_deletion_is_relayed_as_the_bare_null() {
        let notified = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "owner": "urn:ngsi-ld:null",
            "speed": {"type": "Property", "value": 5}
        });
        for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
            let relayed = relayed(&notified, format).await;
            pretty_assertions::assert_eq!(relayed["owner"], json!("urn:ngsi-ld:null"), "{format:?}: {relayed}");
        }
        let normalized = relayed(&notified, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(normalized["speed"], json!({"type": "Property", "value": 5}));
    }
}
