//! ETSI `D011_01_02_inc`: a query by type and an attribute no entity holds answers no entity, though an inclusive
//! context source is registered for the type (clauses 4.3.3, 4.3.6.2 and 5.7.2).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, type_registration, unregister},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::AttributeName};
use serde_json::json;

/// Pins: Check that if one queries for type and an attribute that is missing, no entity gets returned. Contrast with
/// `D011_01_04`, where a local entity also exists but also lacks the queried attribute.
/// Clause: 4.3.3, 4.3.6.2, 5.7.2
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_01_02_inc_query_the_context_broker_with_type_and_missing_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/ngsi-ld/v1/entities"),
            &["attrs", "speed", "type", "vehicle"],
            Some(&[]),
        ),
        StatusCode::OK,
        Some(&json!([])),
    )
    .await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .attrs(vec!["speed".parse::<AttributeName>()?])
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_array(), some(is_empty()));
    unregister(&broker, &namespace, 1).await
}
