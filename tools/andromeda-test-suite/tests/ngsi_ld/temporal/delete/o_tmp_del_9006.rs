//! Deleting the evolution of a deleted entity leaves nothing to read.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: after Delete Entity and then Delete Temporal Evolution of an Entity created with speed 30, both the Entity
/// and its Temporal Evolution are 404 `ResourceNotFound`, and Create Entity with the same id and speed 40 answers
/// 201 with a Temporal Evolution holding speed 40 alone
/// Clause: 5.6.6, 5.6.16, 5.6.1, 5.7.1, 5.7.3, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so the deleted Entity has a Temporal Evolution to
/// delete
#[gtest]
#[tokio::test]
async fn o_tmp_del_9006_deleting_the_evolution_of_a_deleted_entity_leaves_nothing_to_read() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_gone_twice_id = namespace.urn("Vehicle", "gone-twice")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_gone_twice = Entity::builder()
        .id(vehicle_gone_twice_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(30).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_gone_twice).send().await?.done()?;
    broker.entities().delete(&vehicle_gone_twice_id).send().await?.done()?;
    broker.temporal().delete(&vehicle_gone_twice_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_gone_twice_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_gone_twice_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_3 = broker.with_responses().temporal().delete(&vehicle_gone_twice_id).send().await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    let vehicle_gone_twice_2 = Entity::builder()
        .id(vehicle_gone_twice_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2021-02-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_gone_twice_2).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_gone_twice_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(40)))));
    Ok(())
}
