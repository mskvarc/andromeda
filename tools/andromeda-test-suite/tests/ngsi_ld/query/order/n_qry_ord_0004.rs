//! A geo query does not stand in for a missing reference.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        Locality,
        geoquery,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: a distance ordering without `orderFrom` is refused with `BadRequestData` even when the request carries a
/// geo-query, whose geometry does not stand in for the ordering reference
/// Clause: 5.7.2.4, 6.4.3.2, 4.23.3
#[gtest]
#[tokio::test]
async fn n_qry_ord_0004_a_geo_query_does_not_stand_in_for_a_missing_reference() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id)
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
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&local=true&georel=near%3BmaxDistance%3D%3D50000&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location%3Bdist-asc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The same request with orderFrom is admitted, so the refusal above is the missing orderFrom alone.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(50000, Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    Ok(())
}
