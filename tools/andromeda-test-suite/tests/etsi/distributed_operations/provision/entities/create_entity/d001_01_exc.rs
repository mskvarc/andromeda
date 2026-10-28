//! ETSI `D001_01_exc`: an entity created on the broker with the `speed` an exclusive registration holds has that
//! attribute created on the context source alone (clauses 4.3.3, 4.3.6.3 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Property},
    query::Locality,
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Check that the entity is created correctly on both the Context Broker and the Context Source. The entity
/// shall be retrieved correctly.
/// Clause: 4.3.3, 4.3.6.3, 5.6.1
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the retrieval is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d001_01_exc_create_entity_with_exclusive_registration() -> Result<()> {
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
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"));
    stub(&source, rule(), StatusCode::CREATED, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(56).property("source", "Speedometer"))
        .build()?;
    create_entity_with_context_string(&broker, &vocabulary, &speed).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .locality(Locality::Local)
        .send()
        .await?;
    let body = json_body(&retrieved)?;
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.get("speed"), none());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
