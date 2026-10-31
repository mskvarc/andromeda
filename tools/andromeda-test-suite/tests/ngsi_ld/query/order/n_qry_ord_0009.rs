//! A geometry collection reference type is bad request data.

use andromeda_test_suite::{
    broker,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{
    Entity,
    ErrorType,
    geometry::{Position, Shape},
};

/// Pins: `orderGeometry=GeometryCollection` is refused with `BadRequestData`, while `orderGeometry=Point` on the
/// same request is admitted
/// Clause: 6.4.3.2, 4.23.3, 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ord_0009_a_geometry_collection_reference_type_is_bad_request_data() -> Result<()> {
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
    // The client writes orderGeometry from the origin it is given and has none for another type, so each read goes out as written.
    let fields = [("Accept", "application/json")];
    let type_text = building_type.to_string();
    let refused = target(
        "entities",
        &[
            ("type", type_text.as_str()),
            ("local", "true"),
            ("orderBy", "location;dist-asc"),
            ("orderFrom", "[8,40]"),
            ("orderGeometry", "GeometryCollection"),
        ],
    );
    let answer = broker.send_unchecked(exchange(&broker, Method::GET, &refused, &fields, None)?).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let admitted = target(
        "entities",
        &[
            ("type", type_text.as_str()),
            ("local", "true"),
            ("orderBy", "location;dist-asc"),
            ("orderFrom", "[8,40]"),
            ("orderGeometry", "Point"),
        ],
    );
    let answer_2 = broker.send_unchecked(exchange(&broker, Method::GET, &admitted, &fields, None)?).await;
    expect_that!(&answer_2, answered(StatusCode::OK));
    expect_that!(entities(&answer_2?)?.as_slice(), ids_in_order(&[building_id]));
    Ok(())
}
