//! Omit of one attribute of a thirty attribute entity answers every other attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, OneOrMany},
    query::projection::Projection,
};

/// Pins: Retrieve Entity with omit naming one of the thirty Attributes an entity holds answers its id, its type and
/// the twenty-nine others, and omit naming an Attribute the entity lacks answers every Attribute
/// Clause: 4.21, 5.7.1.5, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0036_omit_of_one_attribute_of_a_thirty_attribute_entity_answers_every_other_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("a00", 0)
        .property("a01", 1)
        .property("a02", 2)
        .property("a03", 3)
        .property("a04", 4)
        .property("a05", 5)
        .property("a06", 6)
        .property("a07", 7)
        .property("a08", 8)
        .property("a09", 9)
        .property("a10", 10)
        .property("a11", 11)
        .property("a12", 12)
        .property("a13", 13)
        .property("a14", 14)
        .property("a15", 15)
        .property("a16", 16)
        .property("a17", 17)
        .property("a18", 18)
        .property("a19", 19)
        .property("a20", 20)
        .property("a21", 21)
        .property("a22", 22)
        .property("a23", 23)
        .property("a24", 24)
        .property("a25", 25)
        .property("a26", 26)
        .property("a27", 27)
        .property("a28", 28)
        .property("a29", 29)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .omit("a07".parse::<Projection>()?)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(31));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, lacks_attribute("a07"));
    expect_that!(entity, has_attribute("a00", any_attribute().value(0)));
    expect_that!(entity, has_attribute("a06", any_attribute().value(6)));
    expect_that!(entity, has_attribute("a08", any_attribute().value(8)));
    expect_that!(entity, has_attribute("a29", any_attribute().value(29)));
    // a99 is held by no entity, so nothing is omitted
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .omit("a99".parse::<Projection>()?)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(32));
    expect_that!(entity_2, has_attribute("a07", any_attribute().value(7)));
    Ok(())
}
