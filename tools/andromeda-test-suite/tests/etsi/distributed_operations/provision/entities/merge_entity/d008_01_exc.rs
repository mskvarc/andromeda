//! ETSI `D008_01_exc`: merging an entity whose `speed` an exclusive registration holds sends the merge to the context
//! source and leaves no `speed` on the broker (clauses 4.3.3, 4.3.6.3 and 5.6.17).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
        vehicle_payloads::{vehicle_parked2, vehicle_speed_parked2_by_bob},
    },
    suite_keywords::{delete_entities, ld_json_body_client},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    query::Locality,
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Check that if one requests the Context Broker to merge an entity that matches an exclusive registration,
/// this is merged on the Context Source.
/// Clause: 4.3.3, 4.3.6.3, 5.6.17
/// Corrected: `fix(D008_01_exc)`: assert speed absence on the response body, not the Response object (4.3.6.3)
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
async fn d008_01_exc_merge_entity_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
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
    let id = namespace.urn("Vehicle", 1)?;
    let rule = || ForkRule::resource(Method::PATCH, &source_path(&namespace, &format!("/broker1/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let fragment = vehicle_speed_parked2_by_bob(&namespace, 1)?.into_fragment();
    let merged = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .merge(&id, &fragment)
        .send()
        .await;
    expect_that!(&merged, answered(StatusCode::NO_CONTENT));
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
    delete_entities(&broker, &[&id]).await;
    unregister(&broker, &namespace, 1).await
}
