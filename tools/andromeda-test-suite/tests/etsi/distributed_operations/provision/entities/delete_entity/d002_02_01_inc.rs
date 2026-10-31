//! Deleting an entity that exists on the broker alone, while an inclusive registration matches it, deletes it on the
//! broker and reports the source's 404, answered 207 (clauses 4.3.3, 4.3.6.2, 5.6.6 and 6.3.3).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::query::Locality;
use serde_json::{Value, json};

/// Pins: Verify that, when one has an inclusive registration on a Context Broker, one is able to delete entities
/// locally and should get a `BatchOperationResult` structure
/// Clause: 4.3.3, 4.3.6.2, 5.6.6, 6.3.3
#[gtest]
#[tokio::test]
async fn d002_02_01_inc_delete_entity_on_the_context_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    stub(
        &source,
        ForkRule::resource(Method::DELETE, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}"))),
        StatusCode::NOT_FOUND,
        None,
    )
    .await?;
    let deleted = broker.with_responses().entities().delete(&id).send().await;
    verify_that!(&deleted, answered(StatusCode::MULTI_STATUS))?;
    let body = serde_json::from_slice::<Value>(&deleted?.body)?;
    expect_that!(body.pointer("/errors/0/error/status"), some(any![eq(&json!(404)), eq(&json!("404"))]));
    broker.temporal().delete(&id).send().await.any_answer();
    let retrieved = broker.with_responses().entities().retrieve(&id).locality(Locality::Local).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
