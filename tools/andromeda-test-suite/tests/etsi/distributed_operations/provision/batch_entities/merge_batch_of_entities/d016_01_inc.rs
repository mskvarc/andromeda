//! ETSI `D016_01_inc`: a batch merge of two entities an inclusive registration matches is sent to the context source
//! and merges the `speed` on the broker as well (clauses 4.3.3, 4.3.6.2 and 5.6.20).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::{batch_registration, delete_vehicles},
            merge_batch_of_entities::merge_request::merge_vehicles,
        },
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::create_entity_with_context_string,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::NgsiLdMediaType;

/// Pins: Check that if one requests the Context Broker to merge a batch of entities that match an inclusive
/// registration, they are merged on the Context Source too.
/// Clause: 4.3.3, 4.3.6.2, 5.6.20
/// Corrected: `fix(D016_01_inc)`: assert the merged speed on the returned entity (4.3.6.2)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the query is held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d016_01_inc_merge_batch_entities_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &batch_registration(&namespace, 1, &source_endpoint(&source, &namespace, "")?, None)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/merge"));
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
        .send()
        .await?;
    expect_that!(json_body(&received)?.pointer("/0/speed"), some(anything()));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
