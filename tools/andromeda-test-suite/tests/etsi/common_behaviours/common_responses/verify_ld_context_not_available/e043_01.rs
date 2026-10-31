//! A create whose `@context` cannot be retrieved is refused with 504 `LdContextNotAvailable` (clause 5.2.2).

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    teardown::AnyAnswer,
    wire::send_to,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{BatchCreate, CreateEntity, CreateOrUpdateTemporalEvolution, CreateSubscription, RegisterContextSource},
    },
    model::{Context, ContextEntry, Entity, ErrorType, NgsiLdMediaType, Property, Representation, evolution::TemporalEntity},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// The `@context` every case names: a document under the unreachable origin, in the namespace of the case.
fn unretrievable_context(namespace: &Namespace) -> Result<Context> {
    let url = SuiteConfig::get()?
        .unreachable_origin()
        .join(&format!("contexts/{namespace}/ngsi-ld-core-context-non-existing.jsonld"))?;
    let iri: IriBuf = url.as_str().parse()?;
    Ok(Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::from(iri))]))
}

/// The client of `broker` sending `context` in the body, as the tests send their unretrievable `@context`.
fn in_body(broker: &BrokerClient, context: Context) -> Result<BrokerClient> {
    Ok(broker.with_context(context)?.with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Whether a `title` holds a string, array or object with at least one member.
fn non_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Pins: Verify receiving 503 – `LdContextNotAvailable` error if remote JSON-LD @context cannot be retrieved (Create
/// entity)
/// Clause: 5.2.2
/// Expected: `LdContextNotAvailable` answers 504, not 503 (Table 6.3.2-1)
/// Vocabulary: none (the @context under test is unretrievable by design)
#[gtest]
#[tokio::test]
async fn e043_01_01_create_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = in_body(&broker, unretrievable_context(&namespace)?)?;
    let entity = Entity::builder().id(namespace.urn("Building", 1)?).entity_type("Building").build()?;
    let create = CreateEntity::builder().entity(&entity).build();
    let answer = send_to(&client, &create, "entities/").await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_title());
    broker.entities().delete(&entity.id).send().await.any_answer();
    broker.temporal().delete(&entity.id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify receiving 503 – `LdContextNotAvailable` error if remote JSON-LD @context cannot be retrieved (Create
/// subscription)
/// Clause: 5.2.2
/// Expected: `LdContextNotAvailable` answers 504, not 503 (Table 6.3.2-1)
/// Vocabulary: none (the @context under test is unretrievable by design)
#[gtest]
#[tokio::test]
async fn e043_01_02_create_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = in_body(&broker, unretrievable_context(&namespace)?)?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(format!("http://notifications.invalid/notify/{namespace}/1"))
        .accept(NgsiLdMediaType::Json)
        .build()?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities(["Building"])
        .notification(NotificationParams::builder(endpoint).format(Representation::Simplified).build()?)
        .build()?;
    let _subscription = Cleanup::of(&broker).subscription_of(&subscription);
    let create = CreateSubscription { subscription: &subscription };
    let answer = send_to(&client, &create, "subscriptions/").await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_title());
    broker.subscriptions().delete(&id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify receiving 503 – `LdContextNotAvailable` error if remote JSON-LD @context cannot be retrieved (Create
/// Temporal Representation of Entities)
/// Clause: 5.2.2
/// Expected: `LdContextNotAvailable` answers 504, not 503 (Table 6.3.2-1)
/// Vocabulary: none (the @context under test is unretrievable by design)
#[gtest]
#[tokio::test]
async fn e043_01_03_create_temporal_representation_of_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = in_body(&broker, unretrievable_context(&namespace)?)?;
    let evolution = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Bus")
        .property("speed", Property::builder(45).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(25).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(67).observed_at("2020-08-01T12:07:00Z".parse()?))
        .property("fuelLevel", Property::builder(210).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(145).observed_at("2020-08-01T13:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(124).observed_at("2020-08-01T14:07:00Z".parse()?))
        .build()?;
    let create = CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build();
    let answer = send_to(&client, &create, "temporal/entities/").await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable).with_title());
    broker.temporal().delete(&evolution.id).send().await.any_answer();
    Ok(())
}

/// Pins: Verify receiving 503 – `LdContextNotAvailable` error if remote JSON-LD @context cannot be retrieved (Batch
/// entity create)
/// Clause: 5.2.2
/// Vocabulary: none (the @context under test is unretrievable by design)
#[gtest]
#[tokio::test]
async fn e043_01_04_batch_entity_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let client = in_body(&broker, unretrievable_context(&namespace)?)?;
    let entities = [
        Entity::builder().id(namespace.urn("Building", 1)?).entity_type("Building").build()?,
        Entity::builder().id(namespace.urn("Building", 2)?).entity_type("Building").build()?,
    ];
    let create = BatchCreate::builder().entities(&entities).build();
    let answer = send_to(&client, &create, "entityOperations/create").await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let body: Value = serde_json::from_slice(answer?.body())?;
    let unavailable = json!("https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable");
    expect_that!(body.pointer("/errors/0/error/type"), some(eq(&unavailable)));
    expect_that!(body.pointer("/errors/1/error/type"), some(eq(&unavailable)));
    expect_that!(body.pointer("/errors/0/error/title").is_some_and(non_empty), is_true());
    expect_that!(body.pointer("/errors/1/error/title").is_some_and(non_empty), is_true());
    let ids = entities.map(|entity| entity.id);
    broker.entities().batch_delete(&ids).send().await.any_answer();
    Ok(())
}

/// Pins: Verify receiving 503 – `LdContextNotAvailable` error if remote JSON-LD @context cannot be retrieved (Create
/// context source registration)
/// Clause: 5.2.2
/// Expected: `LdContextNotAvailable` answers 504, not 503 (Table 6.3.2-1)
/// Vocabulary: none (the @context under test is unretrievable by design)
#[gtest]
#[tokio::test]
async fn e043_01_05_create_context_source_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let client = in_body(&broker, unretrievable_context(&namespace)?)?;
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(id.clone())
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .build()?;
    let _registration = Cleanup::of(&broker).registration_of(&registration);
    let register = RegisterContextSource { registration: &registration };
    let answer = send_to(&client, &register, "csourceRegistrations").await;
    expect_that!(&answer, answered(StatusCode::GATEWAY_TIMEOUT));
    broker.registrations().delete(&id).send().await.any_answer();
    Ok(())
}
