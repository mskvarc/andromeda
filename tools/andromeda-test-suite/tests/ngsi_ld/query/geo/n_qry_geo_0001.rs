//! Equals with a point selects the entities at that point.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, geo_property},
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Georel=equals with a Point selects exactly the entities located at that point, every one of them, and none
/// located elsewhere
/// Clause: 4.10, 5.7.2, 6.3.13
/// Recorded: The spec names no predicate semantics beyond the OGC Simple Feature Access definitions (06-103r4,
/// 6.1.15.3); georel equals maps to Equals, a.Relate(b, "TFFFTFFFT"), which for two Points holds exactly when the
/// coordinates are the same
#[gtest]
#[tokio::test]
async fn n_qry_geo_0001_equals_with_a_point_selects_the_entities_at_that_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let twin_id = namespace.urn("Twin", 1)?;
    let twin_type = namespace.entity_type("Twin")?;
    let twin_2_id = namespace.urn("Twin", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "Berlin Brandenburg International Airport")
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("name", "Grunewald")
            .geo_property("location", Shape::Point(Position::try_new(13.215_983, 52.470_441)?))
            .build()?,
        Entity::builder()
            .id(twin_id.clone())
            .entity_type(twin_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(twin_2_id.clone())
            .entity_type(twin_type.clone())
            .property("name", "Eiffel Tower")
            .property("subCategory", "tourism")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Two entities at the point, both answered with their attributes.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&twin_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&twin_id),
            has_attribute(
                "location",
                geo_property()
                    .value_member(["type"], "Point")
                    .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                    .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
            ),
            has_attribute("name", any_attribute().value("Eiffel Tower")),
            has_attribute("subCategory", any_attribute().value("tourism"))
        ))
    );
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&twin_2_id),
            has_attribute(
                "location",
                geo_property()
                    .value_member(["type"], "Point")
                    .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                    .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
            ),
            has_attribute("name", any_attribute().value("Eiffel Tower")),
            has_attribute("subCategory", any_attribute().value("tourism"))
        ))
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::equals(Shape::Point(Position::try_new(0.0, 0.0)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
