//! A batch merge of the `speed` an exclusive registration holds for two broker entities is sent to the context source
//! and leaves no `speed` on the broker (clauses 4.3.3, 4.3.6.3 and 5.6.20).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
    provision::batch_entities::{
        batch_sources::{delete_vehicles, exclusive_batch_registration},
        merge_batch_of_entities::merge_request::merge_vehicles,
    },
    vehicle_payloads::vehicle_parked2,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{model::NgsiLdMediaType, query::Locality};

/// Pins: Check that if one requests the Context Broker to merge a batch of entities that match an exclusive
/// registration, they are merged on the Context Source too.
/// Clause: 4.3.3, 4.3.6.3, 5.6.20
/// Expected: no returned entity carries speed (clauses 4.3.6.3, 5.6.20.4)
/// Expected: an exclusive registration names its entities by id (clause 4.3.6.3)
/// Expected: an exclusive registration names its entities by id (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d016_01_exc_merge_batch_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked2(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &exclusive_batch_registration(&namespace, &source_endpoint(&source, &namespace, "/broker1")?)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/merge"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    merge_vehicles(&broker, &vocabulary, &namespace).await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await?;
    let body = json_body(&received)?;
    let entities = body.as_array().or_fail()?;
    expect_that!(entities, len(eq(2)));
    expect_that!(entities.iter().map(|entity| entity.get("speed")).collect::<Vec<_>>(), each(none()));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
