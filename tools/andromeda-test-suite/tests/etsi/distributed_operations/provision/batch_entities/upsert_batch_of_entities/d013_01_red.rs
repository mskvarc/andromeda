//! ETSI `D013_01_red`: a batch upsert replacing two entities two redirect registrations match is sent to both context
//! sources once, without the `@context` (clauses 4.3.3, 4.3.6.3 and 5.6.8).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::batch_registration,
            forwarded_documents::parked2_forwarded,
            upsert_batch_of_entities::upsert_request::upsert,
        },
        redirected_request::{forwarded_body, wait_for_redirected},
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::ld_json_body_client,
};
use andromeda_test_suite::{broker, comparison::matches_document, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that if one requests the Context Broker to replace a batch of entities that match a redirect
/// registration, these are replaced on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.8
/// Corrected: `fix(D013_red)`: distinct registration id and strip `@context` from the forwarded body (6.3.5)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Wait for redirected request replies 200 to a request the stub has already answered, so the server keeps
/// that reply for the next request no stub matches; the port queues it on the source as one reply of 200 and no body.
/// Changed: Get Request Body is parsed as a Python `'''...'''` literal; the port refuses a backslash, a `'''` and a
/// final `'` and then requires the body to be JSON.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d013_01_red_batch_upsert_entities_with_redirect_registration_without_update_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &batch_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect))?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entityoperations/upsert")));
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    upsert(&writer, &[vehicle_parked2(&namespace, 1)?, vehicle_parked2(&namespace, 2)?], "replace").await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    let body = forwarded_body(&forwarded)?;
    let entities = body.as_array().or_fail()?;
    expect_that!(entities, contains(matches_document(parked2_forwarded(&namespace, 1))));
    expect_that!(entities, contains(matches_document(parked2_forwarded(&namespace, 2))));
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
