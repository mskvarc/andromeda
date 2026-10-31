//! A line string reference measures from the line.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LineString, Position, Shape},
    },
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: orderBy=location;dist-asc with `orderGeometry=LineString` measures each entity's distance to the
/// `LineString` given in `orderFrom`
/// Clause: 4.23.3, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_0002_a_line_string_reference_measures_from_the_line() -> Result<()> {
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
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The line runs through Paris; Beta lies south of Alpha, so nearer to it.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(
            EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::LineString(LineString::try_new(vec![
                Position::try_new(2.3, 48.8)?,
                Position::try_new(2.4, 48.9)?,
            ])?)),
        )
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_2_id)));
    Ok(())
}
