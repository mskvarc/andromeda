//! A geometry collection refusal names order geometry without quoting it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
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
};

/// Pins: the `BadRequestData` answering `orderGeometry=GeometryCollection` names the `orderGeometry` parameter and
/// the rule broken in its detail, without quoting the refused value
/// Clause: 6.3.2, 4.23.3, 5.7.2.4
/// Recorded: 6.3.2 leaves the detail of a `ProblemDetails` to the implementation (it "should convey more
/// information about the error"); Andromeda names the parameter and the rule, never the refused value
#[gtest]
#[tokio::test]
async fn o_qry_ord_0010_a_geometry_collection_refusal_names_order_geometry_without_quoting_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(8.0, 40.0)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Building_{namespace}&local=true&orderBy=location%3Bdist-asc&orderFrom=%5B8%2C40%5D&orderGeometry=GeometryCollection"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("invalid query parameter orderGeometry: not one of the values the parameter defines")
    );
    Ok(())
}
