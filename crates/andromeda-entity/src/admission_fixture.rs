//! Admitting write payloads the way the transport layer does, for tests that
//! drive a write path directly.
//!
//! Every write path accepts only a payload the clause 5.5.4 admission
//! produced, so a test reaches it through the same admission a request does.

use crate::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    batch::admitted_batch::{AdmittedBatch, RenderedEntity},
    write_kind::{AttributeWriteKind, BatchWriteKind, EntityWriteKind},
};
use andromeda_jsonld::{batch_contexts::BatchContexts, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use serde_json::Value;

/// Admit an entity-shaped payload for the write `W`, failing the test when it
/// is refused.
pub(crate) async fn admitted_entity<W: EntityWriteKind>(service: &EntityService, payload: impl WritePayload, context: &ContextSpec) -> AdmittedEntity<W> {
    service.admit_entity_write(payload, context).await.expect("the payload is admitted")
}

/// Admit an Attribute Fragment written to `attribute` for the write `W`,
/// failing the test when it is refused.
pub(crate) async fn admitted_attribute<'a, W: AttributeWriteKind>(
    service: &EntityService,
    attribute: &'a AttributeName,
    payload: Value,
    context: &ContextSpec,
) -> AdmittedAttribute<'a, W> {
    service
        .admit_attribute_write(attribute, payload, context)
        .await
        .expect("the fragment is admitted")
}

/// Admit a batch's entities for `kind`, failing the test when any is refused.
pub(crate) async fn admitted_batch<W: BatchWriteKind>(service: &EntityService, kind: W, entities: Vec<Entity>, context: &ContextSpec) -> AdmittedBatch<W> {
    let (admitted, refused) = service.admit_batch_write(kind, rendered(entities, context)).await;
    assert!(refused.is_empty(), "every entity is admitted: {refused:?}");
    admitted
}

/// Render each entity back to its compact JSON, as the transport layer does
/// before it admits a batch, every entity read against `context`.
pub(crate) fn rendered(entities: Vec<Entity>, context: &ContextSpec) -> Vec<RenderedEntity> {
    // clone: every entity of the fixture batch shares one context
    let mut contexts = BatchContexts::shared(context.clone());
    entities
        .into_iter()
        .map(|entity| {
            let representation = serde_json::to_value(&entity).expect("the entity renders");
            RenderedEntity {
                entity,
                representation,
                context: contexts.context_of(None),
            }
        })
        .collect()
}
