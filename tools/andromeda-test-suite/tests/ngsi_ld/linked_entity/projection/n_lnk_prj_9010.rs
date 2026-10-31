//! Only the returned relationships name the flat array.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::projection::Projection,
};

/// Pins: Retrieve Entity with join=flat and `joinLevel=2` answers the root, the City and the Country when the pick
/// keeps both Relationships, the root and the City when it keeps only the first, and the root alone when it keeps
/// none
/// Clause: 4.21, 4.5.23, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_lnk_prj_9010_only_the_returned_relationships_name_the_flat_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_flat_membership_id = namespace.urn("Building", "flat-membership")?;
    let building_type = namespace.entity_type("Building")?;
    let city_flat_membership_id = namespace.urn("City", "flat-membership")?;
    let city_type = namespace.entity_type("City")?;
    let country_flat_membership_id = namespace.urn("Country", "flat-membership")?;
    let country_type = namespace.entity_type("Country")?;
    let building_flat_membership = Entity::builder()
        .id(building_flat_membership_id.clone())
        .entity_type(building_type)
        .property("name", "Depot")
        .relationship("linkedTo", city_flat_membership_id.clone())
        .geo_property("location", Shape::Point(Position::try_new(1.5, 2.5)?))
        .build()?;
    broker.entities().create(&building_flat_membership).send().await?.done()?;
    let city_flat_membership = Entity::builder()
        .id(city_flat_membership_id.clone())
        .entity_type(city_type)
        .property("name", "Paris")
        .relationship("linkedTo", country_flat_membership_id.clone())
        .build()?;
    broker.entities().create(&city_flat_membership).send().await?.done()?;
    let country_flat_membership = Entity::builder()
        .id(country_flat_membership_id.clone())
        .entity_type(country_type)
        .property("name", "France")
        .build()?;
    broker.entities().create(&country_flat_membership).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_flat_membership_id)
        .join_level(2.try_into()?)
        .pick("id,linkedTo{id,linkedTo{id}}".parse::<Projection>()?)
        .send()
        .await?;
    let entities = retrieved.body();
    expect_that!(entities.as_slice(), len(eq(3)));
    expect_that!(entities.as_slice(), nth(0, has_id(&building_flat_membership_id)));
    expect_that!(entities.as_slice(), nth(1, has_id(&city_flat_membership_id)));
    expect_that!(entities.as_slice(), nth(2, has_id(&country_flat_membership_id)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_flat_membership_id)
        .join_level(2.try_into()?)
        .pick("id,linkedTo{id,name}".parse::<Projection>()?)
        .send()
        .await?;
    let entities_2 = retrieved_2.body();
    expect_that!(entities_2.as_slice(), len(eq(2)));
    expect_that!(entities_2.as_slice(), nth(0, has_id(&building_flat_membership_id)));
    expect_that!(entities_2.as_slice(), nth(1, has_id(&city_flat_membership_id)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_projected(&building_flat_membership_id)
        .join_level(2.try_into()?)
        .pick("id,name".parse::<Projection>()?)
        .send()
        .await?;
    let entities_3 = retrieved_3.body();
    expect_that!(entities_3.as_slice(), len(eq(1)));
    Ok(())
}
