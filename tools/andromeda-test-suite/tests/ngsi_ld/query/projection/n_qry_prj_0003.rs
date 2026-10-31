//! Attrs of a thirty attribute entity answers only the two listed.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute, member_count},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with attrs naming two of the thirty Attributes an entity holds answers that entity with its
/// id, its type and the two listed Attributes alone
/// Clause: 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_prj_0003_attrs_of_a_thirty_attribute_entity_answers_only_the_two_listed() -> Result<()> {
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .attrs(vec!["a07".parse()?, "a21".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items().first().map(member_count), some(eq(4)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute("a07", any_attribute().value(7)),
                has_attribute("a21", any_attribute().value(21)),
                lacks_attribute("a00"),
                lacks_attribute("a29"),
                has_type(&building_type)
            )
        )
    );
    Ok(())
}
