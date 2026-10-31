//! Geo properties rank first and entities lacking the attribute last.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{BatchCreate, QueryEntities},
    },
    model::{
        Entity,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        builders::SelectorTypeBuilder,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: a distance ordering ranks the entities whose attribute is a `GeoProperty` first (nearest or farthest first
/// by direction), then a non-geo value of that attribute, then the entities lacking it, which are ranked last and
/// never dropped
/// Clause: 4.23.2, 4.23.3, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_0011_geo_properties_rank_first_and_entities_lacking_the_attribute_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ids = [
        namespace.urn("Building", 1)?,
        namespace.urn("Building", 2)?,
        namespace.urn("Building", 3)?,
        namespace.urn("Building", 4)?,
    ];
    let building_type = namespace.entity_type("Building")?;
    let entities = [
        Entity::builder()
            .id(ids[0].clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(2.3522, 48.8566)?))
            .build()?,
        Entity::builder()
            .id(ids[1].clone())
            .entity_type(building_type.clone())
            .property("name", "NoLocation")
            .build()?,
        Entity::builder().id(ids[2].clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(ids[3].clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
    ];
    let location: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let documents = [
        UncheckedDocument::from_entity(&entities[0], Lossless::Normalized),
        UncheckedDocument::from_entity(&entities[1], Lossless::Normalized),
        UncheckedDocument::from_entity(&entities[2], Lossless::Normalized).set_member("location", &location),
        UncheckedDocument::from_entity(&entities[3], Lossless::Normalized),
    ];
    // The third entity holds `location` as a Property, which the entity model refuses, so it is written as text.
    let created = broker.unchecked(&BatchCreate::builder().entities(&entities).build())?.documents(&documents);
    expect_that!(&broker.send_unchecked(created).await, answered(StatusCode::CREATED));
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for (direction, order) in [("asc", [3, 0, 2, 1]), ("desc", [0, 3, 2, 1])] {
        let ordering = EntityOrderingBuilder::from(format!("location;dist-{direction}").parse::<OrderBy>()?)
            .origin(Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?;
        let query = QueryEntities::builder()
            .entity_type(&selector)
            .locality(Locality::Local)
            .ordering(&ordering)
            .format(Lossless::Normalized)
            .build();
        // The entity reader refuses `location` as a Property, so the page is read as received.
        let answer = json.send_unchecked(json.unchecked(&query)?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        let body = answer?.into_body();
        expect_that!(&body, json_count("$", 4));
        for (position, index) in order.into_iter().enumerate() {
            expect_that!(&body, json_equals(&format!("$[{position}].id"), ids[index].to_string()));
        }
    }
    Ok(())
}
