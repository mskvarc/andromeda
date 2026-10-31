//! A literal written to the temporal evolution keeps its language and datatype.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Datatype, NgsiLdMediaType, Property, TypedValue, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: after Create or Update Temporal Evolution of an Entity with two greeting instances, {"@value": "hola",
/// "@language": "es"} observed at 2024-01-01 and {"@value": "42", "@type": xsd:integer} observed at 2024-01-02,
/// Retrieve Temporal Evolution of an Entity answers both instances with the @language or type each was written with
/// Clause: 4.5.2, 4.5.7, 5.6.11, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9027_a_literal_written_to_the_temporal_evolution_keeps_its_language_and_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let label_temporal_literals_id = namespace.urn("Label", "temporal-literals")?;
    let label_type = namespace.entity_type("Label")?;
    let label_temporal_literals_evolution = TemporalEntity::builder()
        .id(label_temporal_literals_id.clone())
        .entity_type(label_type)
        .property(
            "greeting",
            Property::builder(object([("@value", Value::from("hola")), ("@language", Value::from("es"))])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            "greeting",
            Property::builder(Value::Typed(TypedValue::try_new(
                Datatype::Other("http://www.w3.org/2001/XMLSchema#integer".parse()?),
                "42",
            )?))
            .observed_at("2024-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&label_temporal_literals_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&label_temporal_literals_id)
        .temporal_q(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "greeting",
            all!(
                len(eq(2)),
                contains(any_attribute().observed_at("2024-01-01T00:00:00Z".parse()?).value_member(["@language"], "es")),
                contains(any_attribute().observed_at("2024-01-01T00:00:00Z".parse()?)).times(eq(1)),
                contains(
                    any_attribute()
                        .observed_at("2024-01-02T00:00:00Z".parse()?)
                        .value_member(["type"], "http://www.w3.org/2001/XMLSchema#integer")
                ),
                contains(any_attribute().observed_at("2024-01-02T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2024-01-02T00:00:00Z".parse()?).value_member(["@value"], "42")),
                contains(any_attribute().observed_at("2024-01-02T00:00:00Z".parse()?)).times(eq(1))
            )
        )
    );
    Ok(())
}
