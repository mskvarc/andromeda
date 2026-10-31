//! Attrs of a thirty attribute entity answers only the listed attributes it holds.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, member_count},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, OneOrMany},
};

/// Pins: Retrieve Entity with attrs naming two of the thirty Attributes an entity holds answers its id, its type
/// and the two listed Attributes alone; a listed Attribute the entity lacks is left out, and a list naming none it
/// holds is 404 `ResourceNotFound`
/// Clause: 5.7.1.4, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_0034_attrs_of_a_thirty_attribute_entity_answers_only_the_listed_attributes_it_holds() -> Result<()> {
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
        .attrs(vec!["a07".parse()?, "a21".parse()?])
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(member_count(entity), eq(4));
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, lacks_attribute("a00"));
    expect_that!(entity, lacks_attribute("a29"));
    expect_that!(entity, has_attribute("a07", any_attribute().value(7)));
    expect_that!(entity, has_attribute("a21", any_attribute().value(21)));
    // a99 is held by no entity, a07 still answers
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["a07".parse()?, "a99".parse()?])
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(member_count(entity_2), eq(3));
    expect_that!(entity_2, lacks_attribute("a99"));
    expect_that!(entity_2, has_attribute("a07", any_attribute().value(7)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .attrs(vec!["a99".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
