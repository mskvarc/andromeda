//! A temporal cursor replayed with another window is refused.

use andromeda_test_suite::{
    broker,
    followed_link::request_replacing,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    raw_call::{call, target},
    temporal_page_body::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::{ErrorType, Property, evolution::TemporalEntity},
};

/// Pins: Query Temporal Evolution of Entities with a cursor taken from a walk over another temporal window answers 400
/// `BadRequestData`
/// Clause: 5.5.9.1, 6.3.20
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9084_a_temporal_cursor_replayed_with_another_window_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    for number in 0..3 {
        let sensor = TemporalEntity::builder()
            .id(namespace.urn("Sensor", format!("{number:04}"))?)
            .entity_type(sensor_type.clone())
            .property("temperature", Property::builder(20).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&sensor).send().await?.created()?;
    }
    // `local` is a parameter the query model does not write, so the query is sent as written.
    let sensor_type_text = sensor_type.to_string();
    let first = call(
        &broker,
        Method::GET,
        &target(
            "temporal/entities",
            &[
                ("type", sensor_type_text.as_str()),
                ("local", "true"),
                ("timerel", "after"),
                ("timeAt", "2023-01-01T00:00:00Z"),
                ("limit", "2"),
            ],
        ),
        &[("Accept", "application/ld+json")],
        None,
    )
    .await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?, len(eq(2)));
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().valid().cloned().or_fail()?;
    let replayed = request_replacing(&broker, &link, &RelationType::Next, "timeAt", "2025-01-01T00:00:00Z")?
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.send_unchecked(replayed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
