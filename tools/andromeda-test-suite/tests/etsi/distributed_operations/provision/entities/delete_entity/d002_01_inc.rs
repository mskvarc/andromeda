//! Deleting an entity an inclusive registration matches deletes it on the broker and on the context source (clauses
//! 4.3.3, 4.3.6.2 and 5.6.6).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::query::Locality;

/// Pins: Verify that, when one has an inclusive registration on a Context Broker, one is able to delete entities on
/// both Context Broker and Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.6
#[gtest]
#[tokio::test]
async fn d002_01_inc_delete_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let rule = || ForkRule::resource(Method::DELETE, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    expect_that!(&broker.with_responses().entities().delete(&id).send().await, answered(StatusCode::NO_CONTENT));
    broker.temporal().delete(&id).send().await.any_answer();
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    let retrieved = broker.with_responses().entities().retrieve(&id).locality(Locality::Local).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
