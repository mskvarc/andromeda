//! Adding or modifying an instance past the limit is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, ErrorType, NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Add Attributes to a Temporal Evolution, and Modify Attribute Instance of its one instance, with an
/// observation of p nesting 33 levels answer 400 `BadRequestData` naming the limit, and the evolution keeps its one
/// instance of one level
/// Clause: 5.6.12.4, 5.6.14.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9004_adding_or_modifying_an_instance_past_the_limit_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe_evolution = TemporalEntity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property(
            "p",
            Property::builder(object([("k", Value::from("leaf"))])).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&nesting_probe_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property(
            "p",
            Property::builder((0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?
        .attributes;
    let answer = broker.with_responses().temporal().add_attributes(&nesting_probe_id, &instances).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("p", len(eq(1))));
    let instance = instance_id_at(evolution, &"p".parse()?, 0)?;
    let instance_2 = Attribute::from(Property::builder((0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))).build()?);
    let answer_2 = broker
        .with_responses()
        .temporal()
        .modify_instance(&nesting_probe_id, &"p".parse::<AttributeName>()?, &instance, &instance_2)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&nesting_probe_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "p",
            all!(len(eq(1)), nth(0, any_attribute().value_member(["k"], "leaf").without_value_member(["k", "k"])))
        )
    );
    Ok(())
}
