//! ETSI `D013_01_exc`: a batch upsert replacing the `speed` an exclusive registration holds for two broker entities
//! forwards only that attribute to the context source (clauses 4.3.3, 4.3.6.3 and 5.6.8).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::{delete_vehicles, exclusive_batch_registration},
            forwarded_documents::speed_forwarded,
            upsert_batch_of_entities::upsert_request::upsert,
        },
        redirected_request::{forwarded_body, wait_for_redirected},
        vehicle_payloads::{vehicle_parked2, vehicle_speed_parked2},
    },
    suite_keywords::ld_json_body_client,
};
use andromeda_test_suite::{broker, comparison::matches_document, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Check that if one requests the Context Broker to replace a batch of entities that match an exclusive
/// registration, these are replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.8
/// Corrected: `fix(D013_exc)`: exclusive batch by id; forward only the covered attribute (4.3.6.3, 5.6.8.4)
/// Corrected: `fix(D012_01_exc)`: exclusive batch registration names entities by id (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Wait for redirected request replies 200 to a request the stub has already answered, so the server keeps
/// that reply for the next request no stub matches; the port queues it on the source as one reply of 200 and no body.
/// Changed: Get Request Body is parsed as a Python `'''...'''` literal; the port refuses a backslash, a `'''` and a
/// final `'` and then requires the body to be JSON.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d013_01_exc_batch_upsert_entities_with_exclusive_registration_without_update_flag() -> Result<()> {
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
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/upsert"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    upsert(
        &writer,
        &[vehicle_speed_parked2(&namespace, 1)?, vehicle_speed_parked2(&namespace, 2)?],
        "replace",
    )
    .await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    let body = forwarded_body(&forwarded)?;
    let entities = body.as_array().or_fail()?;
    expect_that!(entities, contains(matches_document(speed_forwarded(&namespace, 1))));
    expect_that!(entities, contains(matches_document(speed_forwarded(&namespace, 2))));
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
