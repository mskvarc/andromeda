//! Distance ascending and descending from a reference point.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: orderBy=location;dist-asc with an `orderFrom` Point (`orderGeometry` left to its Point default) ranks the
/// nearest entity first, and dist-desc ranks the farthest first
/// Clause: 4.23.1, 4.23.3, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_0001_distance_ascending_and_descending_from_a_reference_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Gamma")
            .geo_property("location", Shape::Point(Position::try_new(2.3522, 48.8566)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Alpha")
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Beta")
            .geo_property("location", Shape::Point(Position::try_new(8.6821, 50.1109)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-desc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_2_id)));
    Ok(())
}
