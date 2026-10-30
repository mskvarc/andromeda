//! Relaying one notified entity the way the inbound side does: read under
//! the context it was notified in, compacted under the local subscription's
//! and written in the format it asks for.

use crate::received_entity::read_notified;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, received_state::ReceivedDocument};
use andromeda_ngsi_ld_model::received::{ReceivedElement, ReceivedEntities};
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::{
    notification_entity::NotificationEntity,
    notification_language::NotificationLanguage,
    notified_changes::{HeldAttributes, NotifiedChanges},
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde_json::Value;

/// An engine that resolves only what is embedded, never the network.
pub(crate) fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
}

/// `entity` read as a source notified it and written for a local subscriber
/// in `format`, both under the core context alone.
pub(crate) async fn relayed(entity: &Value, format: ResponseFormat) -> Value {
    relayed_under(entity, format, None).await
}

/// `entity` relayed in `format` for a subscription naming `language`.
pub(crate) async fn relayed_under(entity: &Value, format: ResponseFormat, language: Option<&NotificationLanguage>) -> Value {
    relay_outcome(entity, &ContextSpec::core_only(), format, language)
        .await
        .expect("the notified entity is relayed")
}

/// `entity`, notified under `inbound`, relayed in `format` for a subscription
/// naming `language` under the core context, or the refusal of it.
pub(crate) async fn relay_outcome(
    entity: &Value,
    inbound: &ContextSpec,
    format: ResponseFormat,
    language: Option<&NotificationLanguage>,
) -> Result<Value, String> {
    let engine = offline_engine();
    let expansion = EntityExpansion::prepare(&engine, inbound).await.expect("the inbound context processes");
    let elements = ReceivedEntities::from_slice(entity.to_string().as_bytes()).expect("an entity");
    let contextual = match elements.0.as_slice() {
        [ReceivedElement::Read(contextual)] => contextual,
        [ReceivedElement::Refused(refusal)] => return Err(refusal.to_string()),
        other => panic!("one element was notified: {other:?}"),
    };
    let received = read_notified(&engine, &expansion, contextual).await.map_err(|refusal| refusal.to_string())?;
    written(&engine, &received, format, language).await
}

/// `received` compacted under the core context and written in `format` for a
/// subscription naming `language`, or the refusal of it.
pub(crate) async fn written(
    engine: &JsonLdEngine,
    received: &ReceivedDocument,
    format: ResponseFormat,
    language: Option<&NotificationLanguage>,
) -> Result<Value, String> {
    let context = ContextSpec::core_only();
    let context_compactor = engine.compactor(&context);
    let respelled = EntityCompactor::new(&context_compactor)
        .document_entity(&received.document)
        .await
        .map_err(|error| error.to_string())?;
    let projection = Projection::default();
    serde_json::to_value(NotificationEntity::relayed(
        &respelled,
        format,
        StructuralProjection::from(&projection),
        language,
    ))
    .map_err(|error| error.to_string())
}

/// `notified` read as a source notified it under the core context.
pub(crate) async fn received(engine: &JsonLdEngine, notified: &Value) -> ReceivedDocument {
    let inbound = EntityExpansion::prepare(engine, &ContextSpec::core_only())
        .await
        .expect("the core context processes");
    let elements = ReceivedEntities::from_slice(notified.to_string().as_bytes()).expect("an entity");
    let [ReceivedElement::Read(contextual)] = elements.0.as_slice() else {
        panic!("the notified entity reads");
    };
    read_notified(engine, &inbound, contextual).await.expect("the notified entity expands")
}

/// `received` written in `format` for a local subscriber asking for
/// `showChanges`, under the core context.
pub(crate) async fn relayed_with_changes(engine: &JsonLdEngine, received: &ReceivedDocument, format: ResponseFormat) -> Value {
    let context = ContextSpec::core_only();
    let context_compactor = engine.compactor(&context);
    let compactor = EntityCompactor::new(&context_compactor);
    let entity = compactor.document_entity(&received.document).await.expect("the entity compacts");
    let previous = compactor
        .document_entity(received.previous.as_ref().expect("previous values were reported"))
        .await
        .expect("the previous values compact");
    let projection = Projection::default();
    let changes = NotifiedChanges {
        held: HeldAttributes::Current,
        deleted: None,
        previous: Some(&previous),
    };
    serde_json::to_value(NotificationEntity::relayed(&entity, format, StructuralProjection::from(&projection), None).with_changes(changes))
        .expect("the entity writes")
}
