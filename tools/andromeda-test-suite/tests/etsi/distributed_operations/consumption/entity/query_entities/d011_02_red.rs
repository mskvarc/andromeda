//! A query by type, by `GET /entities` or by `POST /entityOperations/query`, forwarded to a redirect context source
//! answers the source's matching entity (clauses 4.3.3, 4.3.6.3 and 5.7.2).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{listed_ids, register, serve, source_endpoint, source_path, stub, type_registration, unregister, vehicle_reply},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::NgsiLdMediaType,
    registration::RegistrationMode,
};
use serde_json::{Value, json};

/// The terms of the `@context` the cases register.
const TERMS: [&str; 4] = ["Vehicle", "brandName", "isParked", "providedBy"];

/// The setup of the cases in the private tenant `broker`: registers the vocabulary, registers every Vehicle in the
/// redirect mode at the source's `/broker1`, starts the source and stubs the Vehicle it answers to requests `rule`
/// matches; answers the vocabulary and the source.
async fn redirect_source(broker: &BrokerClient, namespace: &Namespace, rule: impl FnOnce() -> ForkRule) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, namespace, "/broker1")?;
    let registration = type_registration(namespace, 1, "Vehicle", &endpoint, Some(RegistrationMode::Redirect))?;
    let registrations = register(broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    let reply = json!([vehicle_reply(namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")]);
    stub(&source, rule(), StatusCode::OK, Some(&reply)).await?;
    Ok((vocabulary, source, registrations))
}

/// The expectations of the cases on the answer `body`: the one Vehicle of the source.
fn expect_source_vehicle(body: &Value, namespace: &Namespace) {
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(listed_ids(body), contains(eq(&namespace.urn_text("Vehicle", 1).as_str())));
}

/// Pins: Verify that when querying by entity type with a redirect registration that supports queryEntity, the request
/// is forwarded to the Context Source and entities are retrieved correctly
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
/// Expected: the request is application/json with the @context in a Link header, not application/ld+json (clause 6.3.5)
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_02_01_red_query_the_context_broker_with_type_with_query_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let (vocabulary, _source, _registrations) = redirect_source(&broker, &namespace, || {
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"),
            &["type", "vehicle"],
            Some(&["brandname", "id", "isparked", "type"]),
        )
    })
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
    expect_source_vehicle(&json_body(&received?)?, &namespace);
    unregister(&broker, &namespace, 1).await
}

/// Pins: Verify that when querying by entity type with a redirect registration that supports queryBatch, the request
/// is forwarded to the Context Source and entities are retrieved correctly
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
/// Expected: the request is application/json with the @context in a Link header, not application/ld+json (clause 6.3.5)
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d011_02_02_red_query_the_context_broker_with_type_with_query_batch() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let (vocabulary, _source, _registrations) = redirect_source(&broker, &namespace, || {
        ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/query"))
    })
    .await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    expect_source_vehicle(&json_body(&received?)?, &namespace);
    unregister(&broker, &namespace, 1).await
}
