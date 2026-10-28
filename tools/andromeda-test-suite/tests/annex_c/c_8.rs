//! Annex C.8: one `Link` naming a wrapper `@context` that lists two vocabularies conveys both, with or without the
//! core `@context` at its end (clauses 6.3.5 and 6.3.6).

use crate::{
    printed_document::expect_context_only,
    served_context::{ServedContexts, core_context, linked, ok_body, retrieved_ld},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    wire::json_body,
};
use googletest::prelude::*;
use http::{StatusCode, header::LINK};
use ngsi_ld::model::{DateTime, Entity, EntityId, NgsiLdMediaType, Property};
use serde_json::json;
use url::Url;

/// Serves the vehicle vocabulary and the schema stand-in and answers their URLs.
async fn vocabularies(served: &ServedContexts, namespace: &Namespace) -> Result<[Url; 2]> {
    let vehicle = namespace.suffixed("Vehicle");
    let vehicle_vocabulary = served
        .serve(
            "vehicle-v1-context.jsonld",
            &json!({"@context": {
                vehicle.as_str(): format!("http://example.org/vehicle/{vehicle}"),
                "builtYear": "http://example.org/vehicle/builtYear",
                "speed": "http://example.org/vehicle/speed",
            }}),
        )
        .await?;
    let schema = served
        .serve(
            "schema-stand-in.jsonld",
            &json!({"@context": {
                "schema": "https://schema.org/",
                "location": "https://schema.org/location",
                "name": "https://schema.org/name",
            }}),
        )
        .await?;
    Ok([vehicle_vocabulary, schema])
}

/// Serves the wrapper listing both vocabularies and then the core `@context`, and answers its URL.
async fn wrapper(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let [vehicle, schema] = vocabularies(served, namespace).await?;
    served
        .serve(
            "wrapper-context.jsonld",
            &json!({"@context": [vehicle.as_str(), schema.as_str(), core_context()?.as_str()]}),
        )
        .await
}

/// Creates the Vehicle V1, built in 2014 and driving at 121, through `client`, and answers its id.
async fn created_vehicle(client: &BrokerClient, namespace: &Namespace) -> Result<EntityId> {
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", "V1")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("builtYear", "2014")
        .property("speed", Property::builder(121).observed_at("2017-07-29T12:05:02Z".parse::<DateTime>()?))
        .build()?;
    client.entities().create(&vehicle).send().await?;
    Ok(vehicle.id)
}

/// Pins: A Vehicle created with one Link naming a wrapper of two vocabularies is retrieved under that wrapper with
/// the terms of both in force
/// Clause: C.8, 6.3.5, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn c_8_0001_one_wrapper_link_conveys_a_compound_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let client = linked(&broker::front()?, &wrapper(&served, &namespace).await?)?;
    let id = created_vehicle(&client, &namespace).await?;
    let body = retrieved_ld(&client, &id).await?;
    expect_that!(body.get("builtYear"), some(eq(&json!({"type": "Property", "value": "2014"}))));
    expect_that!(
        body.get("speed"),
        some(eq(&json!({"type": "Property", "value": 121, "observedAt": "2017-07-29T12:05:02Z"})))
    );
    served.stop();
    Ok(())
}

/// Pins: Retrieved as JSON-LD under the wrapper, the answer @context is the wrapper alone, already ending with the
/// Core @context, and no Link header carries a @context
/// Clause: C.8, 5.2.3, 6.3.6, 5.7.1
#[gtest]
#[tokio::test]
async fn c_8_0002_the_answer_carries_the_wrapper_and_no_context_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let wrapper = wrapper(&served, &namespace).await?;
    let client = linked(&broker::front()?, &wrapper)?;
    let id = created_vehicle(&client, &namespace).await?;
    let received = client
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .retrieve(&id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    let body = json_body(&received)?;
    expect_context_only(&body, wrapper.as_str());
    expect_that!(received.head.headers().get(LINK), none());
    served.stop();
    Ok(())
}

/// Pins: A wrapper without the Core @context is processed with the core terms implicit, and the answer @context is
/// the wrapper followed by the Core @context
/// Clause: C.8, 4.4, 5.2.3, 6.3.6, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn c_8_0003_a_wrapper_without_the_core_context_still_works() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let [vehicle, schema] = vocabularies(&served, &namespace).await?;
    let without_core = served
        .serve("wrapper-context-without-core.jsonld", &json!({"@context": [vehicle.as_str(), schema.as_str()]}))
        .await?;
    let client = linked(&broker::front()?, &without_core)?;
    let id = created_vehicle(&client, &namespace).await?;
    let body = retrieved_ld(&client, &id).await?;
    expect_that!(body.get("builtYear"), some(eq(&json!({"type": "Property", "value": "2014"}))));
    expect_that!(body.get("@context"), some(eq(&json!([without_core.as_str(), core_context()?.as_str()]))));
    served.stop();
    Ok(())
}

/// Pins: Retrieved with no @context, the Vehicle created under the wrapper shows builtYear under the IRI the wrapper
/// vehicle vocabulary gave it
/// Clause: C.8, 5.5.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_8_0004_the_wrapper_decided_how_terms_expanded() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let front = broker::front()?;
    let id = created_vehicle(&linked(&front, &wrapper(&served, &namespace).await?)?, &namespace).await?;
    let body = ok_body(
        front
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&id)
            .send()
            .await,
    )?;
    expect_that!(body.pointer("/http:~1~1example.org~1vehicle~1builtYear/value"), some(eq(&json!("2014"))));
    served.stop();
    Ok(())
}
