//! A literal keeps its language and datatype in the temporal evolution.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Datatype, Entity, NgsiLdMediaType, Property, TypedValue, Value},
    query::temporal_query,
};

/// Pins: after Create Entity of a Label whose greeting is the value object {"@value": "hola", "@language": "es"},
/// whose size is {"@value": "42", "@type": xsd:integer} and whose weight is {"@value": 7, "@type": xsd:double},
/// each observed at 2024-01-01, Retrieve Temporal Evolution of an Entity renders each instance's value with the
/// @language or type it was written with, as Retrieve Entity renders it
/// Clause: 4.3.1, 4.5.2, 4.5.7, 5.7.3
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9029_a_literal_keeps_its_language_and_datatype_in_the_temporal_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let label_literals_id = namespace.urn("Label", "literals")?;
    let label_type = namespace.entity_type("Label")?;
    let label_literals = Entity::builder()
        .id(label_literals_id.clone())
        .entity_type(label_type)
        .property(
            "greeting",
            Property::builder(object([("@value", Value::from("hola")), ("@language", Value::from("es"))])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "size",
            Property::builder(Value::Typed(TypedValue::try_new(
                Datatype::Other("http://www.w3.org/2001/XMLSchema#integer".parse()?),
                "42",
            )?))
            .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "weight",
            Property::builder(object([
                ("@value", Value::from(7)),
                ("@type", Value::from("http://www.w3.org/2001/XMLSchema#double")),
            ]))
            .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&label_literals).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&label_literals_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("greeting", any_attribute().value_member(["@language"], "es")));
    expect_that!(
        entity,
        has_attribute(
            "size",
            any_attribute().value_member_as_written(
                [],
                Value::Typed(TypedValue::try_new(Datatype::Other("http://www.w3.org/2001/XMLSchema#integer".parse()?), "42")?)
            )
        )
    );
    expect_that!(
        entity,
        has_attribute("weight", any_attribute().value_member(["type"], "http://www.w3.org/2001/XMLSchema#double"))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&label_literals_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "greeting",
            nth(0, any_attribute().value_member(["@value"], "hola").value_member(["@language"], "es"))
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "size",
            nth(
                0,
                any_attribute().value_member_as_written(
                    [],
                    Value::Typed(TypedValue::try_new(Datatype::Other("http://www.w3.org/2001/XMLSchema#integer".parse()?), "42")?)
                )
            )
        )
    );
    expect_that!(
        evolution,
        has_evolution(
            "weight",
            nth(
                0,
                any_attribute()
                    .value_member(["@value"], 7)
                    .value_member(["type"], "http://www.w3.org/2001/XMLSchema#double")
            )
        )
    );
    Ok(())
}
