//! ETSI `D001_02_exc`: an entity whose `speed` an exclusive registration holds for the retrieval operations alone is
//! refused as existing, and nothing reaches the context source (clauses 4.3.3, 4.3.6.3 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
    },
    suite_keywords::{delete_entities, send_create_entity},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, Property},
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Check that the entity cannot be created in the Context Source. The creation shall raise a conflict error.
/// Clause: 4.3.3, 4.3.6.3, 5.6.1
/// Corrected: `fix(D001_02_exc)`: declare attributes and retrieve-only ops on the exclusive registration (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d001_02_exc_create_entity_without_redirection_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let coverage = Coverage {
        operations: OperationGroup::RetrieveOps,
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
    stub(&source, rule(), StatusCode::CONFLICT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(56).property("source", "Speedometer"))
        .build()?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    expect_that!(&send_create_entity(&writer, &speed).await, answered(StatusCode::CONFLICT));
    expect_that!(stub_count(&source, &rule()).await?, eq(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
