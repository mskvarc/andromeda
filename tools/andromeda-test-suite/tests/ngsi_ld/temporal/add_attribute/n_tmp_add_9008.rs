//! A fragment naming a new type adds it to the evolution.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
    type_matcher::{lists_type, type_count},
    typed_fragment::naming_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: an Add Attributes to Temporal Evolution whose Fragment names Car, on a Vehicle evolution, answers 204, and
/// the Temporal Evolution then holds both types and is listed by a temporal query on Vehicle and on Car
/// Clause: 5.6.12.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_add_9008_a_fragment_naming_a_new_type_adds_it_to_the_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2024-06-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(car_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    // The typed operation writes no `type` beside the instances, so the Fragment names it on the written document.
    let appended = broker.send_unchecked(naming_type(&broker, &vehicle_id, &instances, &car_type)?).await;
    expect_that!(&appended, answered(StatusCode::NO_CONTENT));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(retrieved.body(), all!(type_count(2), lists_type(&vehicle_type), lists_type(&car_type)));
    for entity_type in [&vehicle_type, &car_type] {
        let listed = json
            .temporal()
            .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
            .entity_type(entity_type)
            .send()
            .await?;
        expect_that!(listed.items(), len(eq(1)));
    }
    Ok(())
}
