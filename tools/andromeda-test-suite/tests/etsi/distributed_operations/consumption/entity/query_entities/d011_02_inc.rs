//! ETSI `D011_02_inc`: a query by type with `local=true` answers no entity of the inclusive context source and sends
//! it no request (clauses 4.3.3, 4.3.6.2, 5.7.2 and 6.3.18).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, type_registration, unregister, vehicle_reply},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, query::Locality};
use serde_json::json;

/// Pins: Check that if one queries with the local flag, no entity from Context Source gets returned
/// Clause: 4.3.3, 4.3.6.2, 5.7.2, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_02_inc_query_the_context_broker_with_local_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?).await?;
    let source = serve(source).await?;
    let rule = || {
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/ngsi-ld/v1/entities"),
            &["type", "vehicle"],
            Some(&["@context", "brandname", "id", "isparked", "type"]),
        )
    };
    let mut reply = vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob");
    reply
        .as_object_mut()
        .or_fail()?
        .insert("@context".to_owned(), json!([vocabulary.url().as_str()]));
    let reply = reply;
    stub(&source, rule(), StatusCode::OK, Some(&json!([reply]))).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_array(), some(is_empty()));
    expect_that!(stub_count(&source, &rule()).await?, eq(0));
    unregister(&broker, &namespace, 1).await
}
