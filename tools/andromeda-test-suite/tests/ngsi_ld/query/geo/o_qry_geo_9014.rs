//! Geoproperty names its geo property by absolute iri too.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: geoproperty given as the absolute IRI a term expands to selects what the term selects, on the URL of a GET and in the geoQ of a POST query
/// Clause: 4.10, 5.2.13, 5.7.2.4, 6.4.3.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn o_qry_geo_9014_geoproperty_names_its_geo_property_by_absolute_iri_too() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let quay_inside_id = namespace.urn("Quay", "inside")?;
    let quay_type = namespace.entity_type("Quay")?;
    let quay_outside_id = namespace.urn("Quay", "outside")?;
    let entities = [
        Entity::builder()
            .id(quay_inside_id.clone())
            .entity_type(quay_type.clone())
            .geo_property("entrance", Shape::Point(Position::try_new(13.0, 52.0)?))
            .build()?,
        Entity::builder()
            .id(quay_outside_id)
            .entity_type(quay_type.clone())
            .geo_property("entrance", Shape::Point(Position::try_new(20.0, 60.0)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let area = Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
        Position::try_new(12.9, 51.9)?,
        Position::try_new(13.1, 51.9)?,
        Position::try_new(13.1, 52.1)?,
        Position::try_new(12.9, 52.1)?,
        Position::try_new(12.9, 51.9)?,
    ])?])?);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(area.clone()).geoproperty("entrance"))
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&quay_inside_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(area.clone()).geoproperty("https://uri.etsi.org/ngsi-ld/default-context/entrance"))
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&quay_inside_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .geo_q(geoquery::within(area).geoproperty("https://uri.etsi.org/ngsi-ld/default-context/entrance"))
        .entity_type(&quay_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&quay_inside_id)));
    Ok(())
}
