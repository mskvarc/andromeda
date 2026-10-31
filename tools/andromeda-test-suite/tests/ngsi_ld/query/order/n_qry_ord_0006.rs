//! A distance term without a reference is bad request data.

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

/// Pins: a distance ordering with no `orderFrom` and no geo-query is refused with `BadRequestData`, and the same
/// request with an `orderFrom` is answered in distance order
/// Clause: 5.7.2.4, 6.4.3.2, 4.23.3
#[gtest]
#[tokio::test]
async fn n_qry_ord_0006_a_distance_term_without_a_reference_is_bad_request_data() -> Result<()> {
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
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&local=true&orderBy=location%3Bdist-asc"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The identical request with a reference is admitted, so the refusal above is the missing reference and nothing else.
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
    Ok(())
}
