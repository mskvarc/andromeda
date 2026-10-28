//! ETSI `D015_01_exc`: a batch delete of two broker entities an exclusive registration matches is sent to the context
//! source and deletes them on the broker (clauses 4.3.3, 4.3.6.3 and 5.6.10).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::batch_sources::exclusive_batch_registration,
    },
    suite_keywords::expect_batch_done,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::headers::Accept;

/// Pins: Check that if one requests the Context Broker to delete a batch of entities that match an exclusive
/// registration, this is deleted on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.10
/// Corrected: `fix(D014_15_exc)`: exclusive batch by id with no local covered attribute (4.3.6.3, 5.9.2.4)
/// Corrected: `fix(D012_01_exc)`: exclusive batch registration names entities by id (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the query is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d015_01_exc_delete_batch_of_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed"]).await?;
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
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/delete"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let ids = [namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?];
    expect_batch_done(broker.with_responses().entities().batch_delete(&ids).send().await)?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .send()
        .await?;
    expect_that!(json_body(&received)?.as_array(), some(is_empty()));
    unregister(&broker, &namespace, 1).await
}
