//! A query by type merges the broker's entity and the inclusive context source's entity of the same id into one entity
//! holding the attributes of both (clauses 4.3.3, 4.3.6.2 and 5.7.2).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{listed_ids, register, serve, source_endpoint, source_path, stub, type_registration, unregister, vehicle_reply},
        vehicle_payloads::vehicle_speed_parked2,
    },
    entity_operations::{create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::headers::Accept;
use serde_json::json;

/// Pins: Check that entities on the Context Broker and Context Source with the same id get merged and returned as one
/// entity
/// Clause: 4.3.3, 4.3.6.2, 5.7.2
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_01_05_inc_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(
        &broker,
        &namespace,
        &["Vehicle", "speed", "source", "isParked2", "providedBy", "brandName", "isParked"],
    )
    .await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_speed_parked2(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &type_registration(&namespace, 1, "Vehicle", &endpoint, None)?).await?;
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
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(listed_ids(&body), contains(eq(&namespace.urn_text("Vehicle", 1).as_str())));
    expect_that!(body.pointer("/0/brandName"), some(anything()));
    expect_that!(body.pointer("/0/speed"), some(anything()));
    expect_that!(body.pointer("/0/isParked"), some(anything()));
    expect_that!(body.pointer("/0/isParked2"), some(anything()));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
