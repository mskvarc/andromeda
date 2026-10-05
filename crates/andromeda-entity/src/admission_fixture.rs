//! Admitting write payloads the way the transport layer does, for tests that
//! drive a write path directly.
//!
//! Every write path accepts only a payload the clause 5.5.4 admission
//! produced, so a test reaches it through the same admission a request does.

use crate::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    batch::admitted_batch::{AdmittedBatch, SubmittedEntity},
    write_kind::{AttributeWriteKind, BatchWriteKind, EntityWriteKind},
};
use andromeda_jsonld::{
    batch_contexts::BatchContexts,
    context_spec::ContextSpec,
    user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use serde_json::Value;

/// Admit `context` as a request handler admits the context it was submitted,
/// failing the test when it is refused.
pub(crate) async fn admitted_context(service: &EntityService, context: &ContextSpec) -> AdmittedContext {
    // clone: the admission consumes the spec, and the test keeps its own
    admit_user_context(&service.engine, SubmittedContext::from(context.clone()))
        .await
        .expect("the context is admitted")
}

/// Admit an entity-shaped payload for the write `W`, failing the test when it
/// is refused.
pub(crate) async fn admitted_entity<W: EntityWriteKind>(service: &EntityService, payload: impl WritePayload, context: &ContextSpec) -> AdmittedEntity<W> {
    let context = admitted_context(service, context).await;
    service.admit_entity_write(payload, &context).await.expect("the payload is admitted")
}

/// Admit an Attribute Fragment written to `attribute` for the write `W`,
/// failing the test when it is refused.
pub(crate) async fn admitted_attribute<'a, W: AttributeWriteKind>(
    service: &EntityService,
    attribute: &'a AttributeName,
    payload: Value,
    context: &ContextSpec,
) -> AdmittedAttribute<'a, W> {
    let context = admitted_context(service, context).await;
    service
        .admit_attribute_write(attribute, payload, &context)
        .await
        .expect("the fragment is admitted")
}

/// Admit a batch's entities for `kind`, failing the test when any is refused.
pub(crate) async fn admitted_batch<W: BatchWriteKind>(service: &EntityService, kind: W, entities: Vec<Entity>, context: &ContextSpec) -> AdmittedBatch<W> {
    let (admitted, refused) = service.admit_batch_write(kind, submitted(service, entities, context).await).await;
    assert!(refused.is_empty(), "every entity is admitted: {refused:?}");
    admitted
}

/// Submit each entity as the transport layer does before it admits a batch,
/// every entity read against `context`, admitted once.
pub(crate) async fn submitted(service: &EntityService, entities: Vec<Entity>, context: &ContextSpec) -> Vec<SubmittedEntity> {
    // clone: every entity of the fixture batch shares one context
    let shared = BatchContexts::shared(context.clone()).context_of(None);
    let admitted = shared.admit(&service.engine).await.expect("the context is admitted");
    entities
        .into_iter()
        .map(|entity| SubmittedEntity {
            entity,
            context: admitted.clone(), // clone: every entity holds the shared handle
        })
        .collect()
}
