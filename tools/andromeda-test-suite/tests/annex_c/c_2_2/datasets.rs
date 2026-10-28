//! The Vehicle of Annex C.2.2 with two instances of `speed` told apart by `datasetId`, normalized and as the
//! simplified dataset map (clauses 4.5.4 and 4.5.5).

use crate::{
    printed_document::{expect_with_any_context, sort_list_by},
    served_context::{ServedContexts, core_context, linked, ok_body, retrieved_ld},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, ContextEntry, Entity, InlineContext, Input, NgsiLdMediaType, Property, RawJson, Uri, Value},
};
use serde_json::{Value as Json, json};
use url::Url;

/// The terms of the example's multi-attribute `@context`.
fn terms(namespace: &Namespace) -> Json {
    let vehicle = namespace.suffixed("Vehicle");
    json!({
        vehicle.as_str(): format!("http://example.org/{vehicle}"),
        "speed": "http://example.org/speed",
        "source": "http://example.org/hasSource",
    })
}

/// Serves the example's multi-attribute `@context` and answers its URL.
async fn multi_attribute_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    served.serve("multiAttribute.jsonld", &json!({"@context": terms(namespace)})).await
}

/// The `@context` the example's create writes in its body: the terms, then the core `@context`.
fn inline_context(namespace: &Namespace) -> Result<Context> {
    let inline = InlineContext::try_from(terms(namespace).to_string().parse::<RawJson>()?)?;
    let core = IriRefBuf::new(core_context()?.to_string())?;
    Ok(Context::Multiple(vec![ContextEntry::Inline(inline), ContextEntry::Reference(core)]))
}

/// The dataset id `urn:ngsi-ld:Property:<ns>-<source>-speed`.
fn dataset(namespace: &Namespace, source: &str) -> Result<Uri> {
    Ok(namespace.urn_text("Property", format!("{source}-speed")).parse()?)
}

/// Creates the Vehicle A4567 with its speedometer and GPS speeds as `application/ld+json`, the `@context` in the body.
async fn created_vehicle(namespace: &Namespace) -> Result<Entity<Input>> {
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", "A4567")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property(
            "speed",
            Property::builder(55)
                .property("source", "Speedometer")
                .dataset_id(dataset(namespace, "speedometerA4567")?),
        )
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", "GPS")
                .dataset_id(dataset(namespace, "gpsBxyz123")?),
        )
        .build()?;
    broker::front()?
        .with_context(inline_context(namespace)?)?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&vehicle)
        .send()
        .await?;
    Ok(vehicle)
}

/// Pins: A Vehicle with two speed instances told apart by datasetId, created with the inline @context C.2.2 prints,
/// is retrieved with both instances, in any order, as printed
/// Clause: C.2.2, 4.5.5, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0013_two_instances_of_speed_round_trip_by_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &multi_attribute_context(&served, &namespace).await?)?;
    let vehicle = created_vehicle(&namespace).await?;
    let mut body = retrieved_ld(&broker, &vehicle.id).await?;
    sort_list_by(&mut body, "/speed", "datasetId");
    let gps = dataset(&namespace, "gpsBxyz123")?;
    let speedometer = dataset(&namespace, "speedometerA4567")?;
    let expected = json!({
        "id": vehicle.id.to_string(),
        "type": namespace.suffixed("Vehicle"),
        "speed": [
            {"type": "Property", "value": 54.5, "source": {"type": "Property", "value": "GPS"}, "datasetId": gps.as_str()},
            {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}, "datasetId": speedometer.as_str()},
        ],
    });
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=simplified answers the two speed instances as one dataset map keyed by
/// datasetId as C.2.2 prints it
/// Clause: C.2.2, 4.5.4, 4.5.5, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_2_0014_the_simplified_multi_attribute_is_a_dataset_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &multi_attribute_context(&served, &namespace).await?)?;
    let vehicle = created_vehicle(&namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve_simplified(&vehicle.id)
            .send()
            .await,
    )?;
    let expected = json!({
        "id": vehicle.id.to_string(),
        "type": namespace.suffixed("Vehicle"),
        "speed": {"dataset": {
            dataset(&namespace, "speedometerA4567")?.as_str(): 55,
            dataset(&namespace, "gpsBxyz123")?.as_str(): 54.5,
        }},
    });
    expect_with_any_context(body, &expected);
    served.stop();
    Ok(())
}
