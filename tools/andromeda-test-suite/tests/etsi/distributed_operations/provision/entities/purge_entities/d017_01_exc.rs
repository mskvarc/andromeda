//! ETSI `D017_01_exc`: purging the Vehicles, one of which an exclusive registration holds the `speed` of, purges them
//! on the broker and on the context source (clauses 4.3.3, 4.3.6.3 and 5.6.21).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::{
            entity_sources::{Coverage, vehicle_id_registration},
            purge_entities::purge_request::{purge_rule, purge_vehicles},
        },
    },
    suite_keywords::send_create_entity,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, Property},
    query::Locality,
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Verify that, when one has an exclusive registration on a Context Broker, one is able to purge entities based
/// on type on both Context Broker and Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.21
/// Corrected: `fix(D017_01_exc)`: register the exclusive source for the created entity (4.3.6.3, 5.6.1.4, 5.5.13)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d017_01_exc_purge_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let coverage = Coverage {
        operations: OperationGroup::RedirectionOps,
        properties: &["speed"],
    };
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_id_registration(&namespace, 1, 1, &endpoint, Some(RegistrationMode::Exclusive), coverage)?,
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entities")),
        StatusCode::CREATED,
        None,
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(56).property("source", "Speedometer"))
        .build()?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    send_create_entity(&writer, &speed).await.any_answer();
    stub(&source, purge_rule(&namespace, "/broker1"), StatusCode::NO_CONTENT, None).await?;
    purge_vehicles(&broker, &vocabulary).await?;
    expect_that!(stub_count(&source, &purge_rule(&namespace, "/broker1")).await?, gt(0));
    let retrieved = broker.with_responses().entities().retrieve(&id).locality(Locality::Local).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await
}
