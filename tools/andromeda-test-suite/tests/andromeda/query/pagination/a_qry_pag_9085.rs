//! A cursor replayed with another type is refused.

use andromeda_test_suite::{
    broker,
    followed_link::request_replacing,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    paged_buildings::PagedBuildings,
    problem::problem,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::{Entity, ErrorType},
};

/// Pins: Query Entities with a cursor taken from a walk over another type answers 400 `BadRequestData`
/// Clause: 5.5.9.1, 6.3.20
#[gtest]
#[tokio::test]
async fn a_qry_pag_9085_a_cursor_replayed_with_another_type_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let car_type = namespace.entity_type("Car")?;
    let car = Entity::builder()
        .id(namespace.urn("Car", "0000")?)
        .entity_type(car_type.clone())
        .property("height", 50)
        .build()?;
    broker.entities().create(&car).send().await?.done()?;
    let first = buildings.first_page(&broker, "2", &[("q", "height>5")]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?.as_slice(), ids_in_order(&buildings.ids[..2]));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let replayed = request_replacing(&broker, &link, &RelationType::Next, "type", car_type.as_str())?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(replayed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
