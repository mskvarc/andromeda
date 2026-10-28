//! ETSI `D018_01`: a request whose `Via` header already names the broker, as the broker's own forwarded request does,
//! is refused as a loop (clauses 4.3.3, 5.6.6 and 6.3.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister, vehicle_registration},
        loop_detection::via_header::forwarded_via,
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::VIA};
use ngsi_ld::{http::operation::DeleteEntity, registration::RegistrationMode};

/// Pins: Verify that a loop is detected when the Via header contains the broker's identifier.
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Corrected: `fix(D018_01)`: make the registration redirect so the via loopback is 508 (6.3.17)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_01_loop_detection_with_via_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_registration(&namespace, 1, &endpoint, RegistrationMode::Redirect, &[])?,
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entities")),
        StatusCode::CREATED,
        None,
    )
    .await?;
    let vehicle = namespace.urn("Vehicle", 1)?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let via = forwarded_via(&source).await?.or_fail()?;
    let request = broker
        .unchecked(&DeleteEntity {
            id: &vehicle,
            entity_type: None,
        })?
        .append_header(VIA, HeaderValue::from_str(&via)?);
    let deleted = broker.send_unchecked(request).await;
    expect_that!(&deleted, answered(StatusCode::LOOP_DETECTED));
    broker.temporal().delete(&vehicle).send().await.any_answer();
    delete_entities(&broker, &[&vehicle]).await;
    unregister(&broker, &namespace, 1).await
}
