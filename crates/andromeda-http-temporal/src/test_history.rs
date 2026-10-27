//! The expanded history of an entity, for the tests of what the temporal
//! operations do with the histories they gather.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_model::{
    received::{ReceivedElement, ReceivedEntities},
    temporal::{temporal_body::TemporalBody, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use serde_json::Value;

/// The history of the entity `document` names, in the expanded form, its
/// attributes named by the full IRIs the core context leaves as they are.
pub(crate) async fn held_history(document: Value) -> TemporalHistory {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
    let ReceivedEntities(mut elements) = ReceivedEntities::from_slice(document.to_string().as_bytes()).expect("an answer");
    let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
        panic!("the answer is a temporal entity: {document}");
    };
    let TemporalBody::Evolution(entity) = contextual.temporal(ContextReading::keyword_spellings()).expect("the answer reads") else {
        panic!("the answer is a Temporal Evolution");
    };
    let expansion = EntityExpansion::prepare(&engine, &ContextSpec::core_only())
        .await
        .expect("the context resolves");
    TemporalHistory::Instances(expansion.temporal(entity).await.expect("the answer expands"))
}
