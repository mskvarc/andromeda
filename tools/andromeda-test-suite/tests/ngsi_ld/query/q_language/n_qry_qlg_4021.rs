//! An unencoded greater than in a q string is refused with 400.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    raw_call::target,
    raw_http::send_bytes,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::Entity;

/// Pins: Query Entities whose request line carries `q=speed>50` with the `>` unencoded is answered 400, and the same
/// query sent as `q=speed%3E50` selects the Vehicle whose speed is above 50
/// Clause: 4.9, 5.7.2, 6.3.2, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_4021_an_unencoded_greater_than_in_a_q_string_is_refused_with_400() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fast_id = namespace.urn("Vehicle", 1)?;
    let vehicles = [
        Entity::builder()
            .id(fast_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 60)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Vehicle", 2)?)
            .entity_type(vehicle_type.clone())
            .property("speed", 40)
            .build()?,
    ];
    broker.entities().batch_create(&vehicles).send().await?.done()?;
    let type_text = vehicle_type.to_string();
    let typed_read = target("entities", &[("type", type_text.as_str())]);
    // An HTTP client percent-encodes `>` in a query, so this request line is written to the socket as is. RFC 3986
    // leaves `>` out of a URI, and RFC 9112 section 3.2 answers an invalid request-line with 400.
    let refused = send_bytes(
        BrokerRole::Front,
        &Method::GET,
        &format!("{typed_read}&q=speed>50"),
        &[("Accept", "application/json")],
        b"",
    )
    .await?;
    expect_that!(refused.status(), eq(StatusCode::BAD_REQUEST));
    // The typed query writes the q string itself, so this read goes out as written.
    let read = format!("{typed_read}&q=speed%3E50");
    let answer = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/json")], None)?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let selected = entities(&answer?)?;
    expect_that!(selected, len(eq(1)));
    expect_that!(selected, nth(0, has_id(&fast_id)));
    Ok(())
}
