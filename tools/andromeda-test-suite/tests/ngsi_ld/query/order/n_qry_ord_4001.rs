//! A descending distance ordering without order from is bad request data under every georel.

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

/// Pins: orderBy=location;dist-desc without `orderFrom` is refused with `BadRequestData` beside a within geo-query,
/// an intersects geo-query and a near geo-query with `minDistance`, none of which stands in for the missing
/// reference
/// Clause: 5.7.2.4, 6.4.3.2, 4.23.3
#[gtest]
#[tokio::test]
async fn n_qry_ord_4001_a_descending_distance_ordering_without_order_from_is_bad_request_data_under_every_georel() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(13.509_818, 52.363_457)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&local=true&georel=within&geometry=Polygon&coordinates=%5B%5B%5B13.0%2C52.0%5D%2C%5B14.0%2C52.0%5D%2C%5B14.0%2C53.0%5D%2C%5B13.0%2C53.0%5D%2C%5B13.0%2C52.0%5D%5D%5D&orderBy=location%3Bdist-desc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&local=true&georel=intersects&geometry=Polygon&coordinates=%5B%5B%5B13.0%2C52.0%5D%2C%5B14.0%2C52.0%5D%2C%5B14.0%2C53.0%5D%2C%5B13.0%2C53.0%5D%2C%5B13.0%2C52.0%5D%5D%5D&orderBy=location%3Bdist-desc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Building_{namespace}&local=true&georel=near%3BminDistance%3D%3D100&geometry=Point&coordinates=%5B13.3986%2C52.5547%5D&orderBy=location%3Bdist-desc"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // With the reference given, the same request is admitted, farthest first.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(100, Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-desc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.3986, 52.5547)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    Ok(())
}
