//! ETSI `D012_01_exc`: a batch create of the `speed` an exclusive registration holds for two broker entities is sent to
//! the context source (clauses 4.3.3, 4.3.6.3 and 5.6.7).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::batch_sources::{delete_vehicles, exclusive_batch_registration},
    },
    suite_keywords::{batch_create, ld_json_body_client},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Entity, Input, Property};

/// Pins: Check that if one requests the Context Broker to create a batch of entities that match an exclusive
/// registration, these are created on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.7
/// Corrected: `fix(D012_01_exc)`: exclusive batch registration names entities by id (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d012_01_exc_batch_create_entities_with_exclusive_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &exclusive_batch_registration(&namespace, &source_endpoint(&source, &namespace, "/broker1")?)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/create"));
    stub(&source, rule(), StatusCode::CREATED, None).await?;
    let speed = |n: u32| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(namespace.urn("Vehicle", n)?)
            .entity_type("Vehicle")
            .property("speed", Property::builder(56).property("source", "Speedometer"))
            .build()?)
    };
    batch_create(&ld_json_body_client(&broker, &vocabulary)?, &[speed(1)?, speed(2)?]).await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
