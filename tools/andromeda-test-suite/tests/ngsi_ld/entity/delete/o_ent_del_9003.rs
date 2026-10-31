//! An entity that never recorded an instance is deleted.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: An Entity created without Attributes answers 204 to Delete Temporal Evolution and then to Delete Entity,
/// and is gone
/// Clause: 5.6.6.4, 5.6.16.4
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default, so an Entity created through the Core API has a
/// Temporal Evolution to delete
#[gtest]
#[tokio::test]
async fn o_ent_del_9003_an_entity_that_never_recorded_an_instance_is_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_attributeless_id = namespace.urn("Building", "attributeless")?;
    let building_type = namespace.entity_type("Building")?;
    let building_attributeless = Entity::builder().id(building_attributeless_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building_attributeless).send().await?.done()?;
    broker.temporal().delete(&building_attributeless_id).send().await?;
    broker.entities().delete(&building_attributeless_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_attributeless_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    Ok(())
}
