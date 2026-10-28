//! ETSI `D018_03`: the `Via` header of a forwarded delete holds the `Via` the broker sent with its forwarded create
//! (clauses 4.3.3, 5.6.6 and 6.3.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister, vehicle_registration},
        loop_detection::via_header::{forwarded_via, forwarded_via_after},
    },
    suite_keywords::create_entity_with_context_string,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that the request contains the Via header from both the Context Broker and the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one more than an earlier wait
/// took.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: `Should Contain ${delete_via_header}[Via]\[value] ${create_via_header}` reads the delete's Via followed by
/// the literal text [value] (the backslash escapes the bracket), so the port asserts that the delete's Via contains the
/// create's; the two differ only for a create Via that ends in a prefix of [value]
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_03_check_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_registration(&namespace, 1, &endpoint, RegistrationMode::Inclusive, &[])?,
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
    let vehicle = namespace.urn("Vehicle", 1)?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let create_via = forwarded_via(&source).await?.or_fail()?;
    let path = format!("/broker1/ngsi-ld/v1/entities/{vehicle}");
    stub(
        &source,
        ForkRule::resource(Method::DELETE, &source_path(&namespace, &path)),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    let deleted = broker.with_responses().entities().delete(&vehicle).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    broker.temporal().delete(&vehicle).send().await.any_answer();
    expect_that!(forwarded_via_after(&source, 2).await?, some(contains_substring(create_via)));
    unregister(&broker, &namespace, 1).await
}
