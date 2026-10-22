//! The temporal entities a Context Source answers with, expanded as the
//! broker reads them, shared by every test that reads an answered part.

use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_model::{
    received::ReceivedElement,
    temporal::{received_temporal::ReceivedTemporalEntities, temporal_body::TemporalBody, temporal_document::TemporalDocument},
};
use serde_json::{Value, json};

/// The context every answer is written in: each term is
/// `https://example.com/<term>`.
fn context() -> ContextSpec {
    ContextSpec::from_body(
        JsonLiteral::admit(
            &json!({
                "speed": "https://example.com/speed",
                "brand": "https://example.com/brand",
                "says": "https://example.com/says",
                "settings": "https://example.com/settings",
                "gender": "https://example.com/gender",
            })
            .to_string(),
        )
        .expect("an admissible literal"),
    )
}

/// The temporal entity a source answered with, expanded under the context.
pub(crate) async fn answered(answer: &Value) -> TemporalDocument {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
    let context = context();
    let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
    let ReceivedTemporalEntities(mut elements) = ReceivedTemporalEntities::from_slice(answer.to_string().as_bytes()).expect("an answer");
    let Some(ReceivedElement::Read(contextual)) = elements.pop() else {
        panic!("the answer is a temporal entity");
    };
    let TemporalBody::Evolution(entity) = contextual.body else {
        panic!("the answer is a Temporal Evolution");
    };
    expansion.temporal(entity).await.expect("the answer expands")
}
