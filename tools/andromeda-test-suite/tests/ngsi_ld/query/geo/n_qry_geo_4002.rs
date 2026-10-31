//! A near with two distance modifiers is bad request data in any order or repetition.

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
    query::geoquery,
};

/// Pins: Query Entities refuses with `BadRequestData` a near georel that carries two distance modifiers whatever
/// they are: `maxDistance` before `minDistance`, a repeated `maxDistance`, a repeated `minDistance`, and with the
/// geoproperty named
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_4002_a_near_with_two_distance_modifiers_is_bad_request_data_in_any_order_or_repetition() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let spot_band_id = namespace.urn("Spot", "band")?;
    let spot_type = namespace.entity_type("Spot")?;
    let spot_band = Entity::builder()
        .id(spot_band_id.clone())
        .entity_type(spot_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.433, 52.534_473)?))
        .build()?;
    broker.entities().create(&spot_band).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Spot_{namespace}&georel=near%3BmaxDistance%3D%3D10000%3BminDistance%3D%3D500&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Spot_{namespace}&georel=near%3BmaxDistance%3D%3D10000%3BmaxDistance%3D%3D20000&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!(
            "entities?type=Spot_{namespace}&georel=near%3BminDistance%3D%3D500%3BminDistance%3D%3D1000&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("entities?type=Spot_{namespace}&georel=near%3BminDistance%3D%3D500%3BmaxDistance%3D%3D10000&geometry=Point&coordinates=%5B13.38893%2C52.534473%5D&geoproperty=location"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_min(500, Shape::Point(Position::try_new(13.38893, 52.534_473)?)))
        .entity_type(&spot_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&spot_band_id)));
    Ok(())
}
