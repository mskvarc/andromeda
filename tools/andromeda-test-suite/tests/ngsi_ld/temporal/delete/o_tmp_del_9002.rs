//! Delete temporal evolution leaves the current entity unchanged.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Delete Temporal Evolution of an Entity created with speed 55 observed at 2021-01-01 answers 204; its
/// Temporal Evolution is then 404 `ResourceNotFound` and the Entity still holds speed 55
/// Clause: 5.6.16, 4.3.1, 5.6.6
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_del_9002_delete_temporal_evolution_leaves_the_current_entity_unchanged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_evolution_deleted_id = namespace.urn("Vehicle", "evolution-deleted")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution_deleted = Entity::builder()
        .id(vehicle_evolution_deleted_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(55).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_evolution_deleted).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_evolution_deleted_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    broker.temporal().delete(&vehicle_evolution_deleted_id).send().await?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_evolution_deleted_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_evolution_deleted_id)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
