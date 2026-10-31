//! A value declared as a time value is held to it by its type and its value type.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property, Value, VocabTerm},
};

/// Pins: Create Entity answers 400 `BadRequestData` for a number typed `DateTime` and for yesterday beside
/// `valueType` `DateTime`, and a plain value beside `valueType` `DateTime` reads back plain in its one spelling
/// beside its `valueType`
/// Clause: 4.5.2, 4.6.3, 5.6.1, C.6
#[gtest]
#[tokio::test]
async fn n_ent_cre_7616_a_value_declared_as_a_time_value_is_held_to_it_by_its_type_and_its_value_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_numeric_id = namespace.urn("Vehicle", "numeric")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_yesterday_id = namespace.urn("Vehicle", "yesterday")?;
    let vehicle_declared_id = namespace.urn("Vehicle", "declared")?;
    let vehicle_numeric = Entity::builder()
        .id(vehicle_numeric_id)
        .entity_type(vehicle_type.clone())
        .property("testedAt", object([("@type", Value::from("DateTime")), ("@value", Value::from(123))]))
        .build()?;
    let answer = broker.with_responses().entities().create(&vehicle_numeric).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_yesterday = Entity::builder()
        .id(vehicle_yesterday_id)
        .entity_type(vehicle_type.clone())
        .property("testedAt", Property::builder("yesterday").value_type("DateTime".parse()?))
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&vehicle_yesterday).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let vehicle_declared = Entity::builder()
        .id(vehicle_declared_id.clone())
        .entity_type(vehicle_type.clone())
        .property("testedAt", Property::builder("2018-12-04T12:00:00.000Z").value_type("DateTime".parse()?))
        .build()?;
    broker.entities().create(&vehicle_declared).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_declared_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "testedAt",
            any_attribute().value("2018-12-04T12:00:00Z").value_type("DateTime".parse::<VocabTerm>()?)
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_declared_id)));
    Ok(())
}
