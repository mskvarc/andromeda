//! Annex C.6: Date, Time and `DateTime` values: a temporal property admits no reification, a Property names its
//! datatype with `valueType` or as a typed value, a coerced term compacts only where nothing is reified, and only a
//! structural timestamp is a time property of a temporal query (clauses 4.6.3, 4.8 and 4.11).

use crate::{
    served_context::{ServedContexts, linked, ok_body, retrieved_ld},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    wire,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{CoreDatatype, Entity, Input, Lossless, NgsiLdMediaType, Property, RawJson, TypedValue, Value, VocabTerm},
    query::temporal_query::{self, TemporalProperty},
};
use serde_json::json;
use url::Url;

/// The Vehicle B9211 whose `testedAt` is `tested`.
fn tested_vehicle(namespace: &Namespace, tested: Property<Input>) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", "B9211")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("testedAt", tested)
        .build()?)
}

/// Serves the `@context` coercing `testedAt` to `DateTime` and answers its URL.
async fn tested_at_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let vehicle = namespace.suffixed("Vehicle");
    let document = json!({"@context": {
        vehicle.as_str(): format!("http://example.org/vehicle/{vehicle}"),
        "testedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"},
    }});
    served.serve("tested-at-context.jsonld", &document).await
}

/// Creates B9211 with the plain string `testedAt` of 2018-12-04 under the coercing `@context`, and answers the client
/// sending that `@context`.
async fn created_under_coercion(served: &ServedContexts, namespace: &Namespace) -> Result<(BrokerClient, Entity<Input>)> {
    let broker = linked(&broker::front()?, &tested_at_context(served, namespace).await?)?;
    let vehicle = tested_vehicle(namespace, Property::builder("2018-12-04T12:00:00Z").build()?)?;
    broker.entities().create(&vehicle).send().await?;
    Ok((broker, vehicle))
}

/// Pins: A speed Property whose observedAt carries sub-Properties is refused with 400, a `TemporalProperty`
/// admitting no reification
/// Clause: C.6, 4.8, 4.5.2.2, 5.6.1
#[gtest]
#[tokio::test]
async fn c_6_0001_a_temporal_property_cannot_be_reified() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", "B9211")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 55)
        .build()?;
    // The builder types observedAt as a DateTime, so the reified observedAt is written into the document.
    let speed: RawJson = json!({
        "type": "Property",
        "value": 55,
        "observedAt": {
            "type": "Property",
            "value": "2018-12-04T12:00:00Z",
            "accuracy": {"type": "Property", "value": 0.9},
        },
    })
    .to_string()
    .parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("speed", &speed);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, answered(StatusCode::BAD_REQUEST));
    served.stop();
    Ok(())
}

/// Pins: A testedAt Property written with valueType `DateTime`, as C.6 prints it, is retrieved with its value and its
/// valueType
/// Clause: C.6, 4.5.2.2, 4.6.3, 5.7.1
#[gtest]
#[tokio::test]
async fn c_6_0002_a_value_type_carries_the_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let tested = Property::builder("2018-12-04T12:00:00Z").value_type("DateTime".parse::<VocabTerm>()?).build()?;
    let vehicle = tested_vehicle(&namespace, tested)?;
    broker.entities().create(&vehicle).send().await?;
    let body = retrieved_ld(&broker, &vehicle.id).await?;
    expect_that!(body.pointer("/testedAt/value"), some(eq(&json!("2018-12-04T12:00:00Z"))));
    expect_that!(body.pointer("/testedAt/valueType"), some(eq(&json!("DateTime"))));
    served.stop();
    Ok(())
}

/// Pins: A testedAt Property whose value is an @value with @type `DateTime`, as C.6 prints it, is retrieved with the
/// type rendered as type, the core @context mapping type to @type
/// Clause: C.6, 4.5.2.2, 4.6.3, 5.7.1
#[gtest]
#[tokio::test]
async fn c_6_0003_a_typed_value_compacts_its_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let typed = Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2018-12-04T12:00:00Z")?);
    let vehicle = tested_vehicle(&namespace, Property::builder(typed).build()?)?;
    broker.entities().create(&vehicle).send().await?;
    let body = retrieved_ld(&broker, &vehicle.id).await?;
    expect_that!(
        body.pointer("/testedAt/value"),
        some(eq(&json!({"type": "DateTime", "@value": "2018-12-04T12:00:00Z"})))
    );
    served.stop();
    Ok(())
}

/// Pins: Retrieved under the @context coercing testedAt to `DateTime`, the normalized testedAt Property keeps its
/// Fully Qualified Name, a coerced term matching no reified Property
/// Clause: C.6, 5.5.7, 4.5.2.2, 5.7.1
#[gtest]
#[tokio::test]
async fn c_6_0004_a_coerced_term_does_not_compact_a_reified_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let (broker, vehicle) = created_under_coercion(&served, &namespace).await?;
    let body = retrieved_ld(&broker, &vehicle.id).await?;
    expect_that!(body.get("http://example.org/test/testedAt"), some(anything()));
    expect_that!(body.get("testedAt"), none());
    served.stop();
    Ok(())
}

/// Pins: Retrieved under the @context coercing testedAt to `DateTime` with format=simplified, testedAt compacts to
/// its short name with the bare `DateTime` string, there being no reification
/// Clause: C.6, 5.5.7, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn c_6_0005_a_coerced_term_compacts_in_the_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let (broker, vehicle) = created_under_coercion(&served, &namespace).await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve_simplified(&vehicle.id)
            .send()
            .await,
    )?;
    expect_that!(body.get("testedAt"), some(eq(&json!("2018-12-04T12:00:00Z"))));
    served.stop();
    Ok(())
}

/// Pins: Query Temporal Evolution of Entities admits observedAt, createdAt, modifiedAt and deletedAt as timeproperty
/// and refuses the user Property testedAt with 400
/// Clause: C.6, 4.11, 5.7.4
#[gtest]
#[tokio::test]
async fn c_6_0006_only_a_structural_timestamp_is_a_time_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let tested = Property::builder("2018-12-04T12:00:00Z").value_type("DateTime".parse::<VocabTerm>()?).build()?;
    broker.entities().create(&tested_vehicle(&namespace, tested)?).send().await?;
    for property in [
        TemporalProperty::ObservedAt,
        TemporalProperty::CreatedAt,
        TemporalProperty::ModifiedAt,
        TemporalProperty::DeletedAt,
    ] {
        let answer = broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .temporal()
            .query(temporal_query::after("2018-01-01T00:00:00Z".parse()?).time_property(property))
            .entity_type(namespace.suffixed("Vehicle").as_str())
            .send()
            .await;
        expect_that!(&answer, answered(StatusCode::OK));
    }
    // The temporal query types timeproperty, so the user Property is written into the target.
    let target = format!(
        "temporal/entities?type={}&timerel=after&timeAt=2018-01-01T00:00:00Z&timeproperty=testedAt",
        namespace.suffixed("Vehicle")
    );
    let request = wire::request_to(&broker, Method::GET, &target)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, answered(StatusCode::BAD_REQUEST));
    served.stop();
    Ok(())
}
