//! ETSI `D011_01_aux`: a query by type merges the broker's entity with the auxiliary context source's entity of the
//! same id, the broker's attributes taking precedence (clauses 4.3.3, 4.3.6.2 and 5.7.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register_in_mode, serve, source_endpoint, source_path, stub, type_registration, unregister, vehicle_reply},
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::headers::Accept;
use serde_json::{Map, Value, json};

/// Pins: Check that if one queries the Context Broker for type, entities get merged correctly
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_01_aux_query_entities_that_exists_on_both_the_context_source_and_the_context_broker_from_the_context_broker() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "isParked"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let registration = type_registration(&namespace, 1, "Vehicle", &endpoint, None)?;
    let _registration = register_in_mode(&broker, &vocabulary, &registration, "auxiliary").await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/ngsi-ld/v1/entities"),
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
    expect_that!(body.pointer("/0/brandName").and_then(Value::as_object).map(Map::len), some(eq(2)));
    expect_that!(body.pointer("/0/brandName/type"), some(eq(&json!("Property"))));
    expect_that!(body.pointer("/0/brandName/value"), some(eq(&json!("BMW"))));
    expect_that!(body.pointer("/0/isParked"), some(anything()));
    expect_that!(body.pointer("/0/isParked2"), some(anything()));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
