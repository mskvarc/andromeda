//! No overwrite adds the fragment type while keeping the held attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
    spelling_matcher::{listing, listing_count},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Append Entity Attributes with `options=noOverwrite` and a fragment typed Car naming the held speed answers
/// 207, keeps speed 80, and still adds Car to the entity types
/// Clause: 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_att_app_9002_no_overwrite_adds_the_fragment_type_while_keeping_the_held_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 80)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().entity_type(car_type.clone()).property("speed", 120).build()?;
    broker
        .entities()
        .append_attributes(&vehicle_id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .partial()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(&entity.entity_type, listing_count(2));
    expect_that!(&entity.entity_type, listing(&vehicle_type));
    expect_that!(&entity.entity_type, listing(&car_type));
    expect_that!(entity, has_attribute("speed", any_attribute().value(80)));
    Ok(())
}
