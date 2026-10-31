//! A query by type, by `GET /entities` or by `POST /entityOperations/query`, forwarded to the exclusive context source
//! of the registered attribute answers the one merged entity (clauses 4.3.3, 4.3.6.3 and 5.7.2).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, listed_ids, register, serve, source_endpoint, source_path, stub, unregister, vehicle_registration},
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
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
const TERMS: [&str; 6] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"];

/// The setup of the cases: registers the vocabulary, creates the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` locally,
/// registers its `speed` exclusively at the source's `/broker1` for the redirection operations, starts the source and
/// stubs the `speed` it answers for the Vehicle; answers the vocabulary and the source.
async fn exclusive_speed_source(broker: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(broker, &vocabulary, &vehicle_parked(namespace, 1)?).await?;
    let registration = vehicle_registration(
        namespace,
        1,
        &source_endpoint(&source, namespace, "/broker1")?,
        RegistrationMode::Exclusive,
        &["speed"],
    )?;
    let registrations = register(broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    let reply = json!([{
        "id": namespace.urn_text("Vehicle", 1),
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 56, "source": {"type": "Property", "value": "Speedometer"}},
    }]);
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(namespace, "/broker1/ngsi-ld/v1/entities"),
            &["type", "vehicle", "id", &namespace.urn_text("Vehicle", 1), "attrs", "speed"],
            Some(&["id", "speed", "type"]),
        ),
        StatusCode::OK,
        Some(&reply),
    )
    .await?;
    Ok((vocabulary, source, registrations))
}

/// The expectations of the cases on the answer `body`: one entity, the Vehicle, with its `speed`.
fn expect_merged_vehicle(body: &Value, namespace: &Namespace) {
    expect_that!(body.pointer("/0/speed"), some(anything()));
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(listed_ids(body), contains(eq(&namespace.urn_text("Vehicle", 1).as_str())));
}

/// The teardown of the cases: deletes the registration (204) and the Vehicle.
async fn torn_down(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    unregister(broker, namespace, 1).await?;
    delete_entities(broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}

/// Pins: Check that if one queries the Context Broker for type, and the registration only allows the queryEntity
/// operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
/// Expected: the broker forwards the query to the source as a downgraded GET (clause 4.3.6.1)
#[gtest]
#[tokio::test]
async fn d011_02_01_exc_query_the_context_broker_with_type_with_query_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _source, _registrations) = exclusive_speed_source(&broker, &namespace).await?;
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
    expect_merged_vehicle(&json_body(&received?)?, &namespace);
    torn_down(&broker, &namespace).await
}

/// Pins: Check that if one queries the Context Broker for type, and the registration only allows the queryBatch
/// operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.3, 5.7.2
/// Expected: the broker forwards the query to the source as a downgraded GET (clause 4.3.6.1)
#[gtest]
#[tokio::test]
async fn d011_02_02_exc_query_the_context_broker_with_type_with_query_batch() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let (vocabulary, _source, _registrations) = exclusive_speed_source(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .ids(vec![namespace.urn("Vehicle", 1)?])
        .method(QueryMethod::Post)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    expect_merged_vehicle(&json_body(&received?)?, &namespace);
    torn_down(&broker, &namespace).await
}
