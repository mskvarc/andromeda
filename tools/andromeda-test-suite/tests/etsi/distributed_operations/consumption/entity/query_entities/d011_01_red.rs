//! ETSI `D011_01_red`: a query by type forwarded to a redirect context source answers the source's matching entity
//! (clauses 4.3.3, 4.3.6.3 and 5.7.2).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{listed_ids, register, serve, source_endpoint, source_path, stub, type_registration, unregister, vehicle_reply},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};
use serde_json::json;

/// Pins: Check that if one queries the Context Broker for type, entity with matching type on a Context Source gets
/// returned
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_01_red_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let registration = type_registration(&namespace, 1, "Vehicle", &endpoint, Some(RegistrationMode::Redirect))?;
    let _registration = register(&broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"),
            &["type", "vehicle"],
            Some(&["brandname", "id", "isparked", "type"]),
        ),
        StatusCode::OK,
        Some(&json!([vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")])),
    )
    .await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(listed_ids(&body), contains(eq(&namespace.urn_text("Vehicle", 1).as_str())));
    unregister(&broker, &namespace, 1).await
}
