//! A reference position of four ordinates is bad request data.

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
        ordering::{OrderBy, builders::EntityOrderingBuilder},
    },
};

/// Pins: an `orderFrom` position of four ordinates is refused with `BadRequestData`, while a two-ordinate position
/// on the same request is admitted
/// Clause: 5.7.2.4, 4.23.3, 6.4.3.2
/// Recorded: RFC 7946 section 3.1.1 says a position should not carry more than three elements and lets parsers
/// ignore extra ones, so the spec does not require the refusal; Andromeda refuses the position
#[gtest]
#[tokio::test]
async fn o_qry_ord_0008_a_reference_position_of_four_ordinates_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(8.0, 40.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&local=true&orderBy=location%3Bdist-asc&orderFrom=%5B13.4%2C52.52%2C0%2C9%5D"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("location;dist-asc".parse::<OrderBy>()?).origin(Shape::Point(Position::try_new(13.4, 52.52)?)))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    Ok(())
}
