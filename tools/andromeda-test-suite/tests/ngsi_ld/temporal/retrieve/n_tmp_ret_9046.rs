//! A zero last n is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with lastN 0 answers 400 `BadRequestData`, and with lastN 1 answers the
/// one most recent instance
/// Clause: Table 6.19.3.1-1, 5.5.4, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9046_a_zero_last_n_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_evolution = TemporalEntity::builder()
        .id(device_id.clone())
        .entity_type(device_type)
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2020-08-01T13:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The client refuses a `lastN` of 0, so the query string is sent as written.
    let request = request_to(
        &client,
        Method::GET,
        &format!("temporal/entities/{device_id}?timerel=after&timeAt=2020-07-01T00:00:00Z&lastN=0"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = client.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = client
        .temporal()
        .retrieve(&device_id)
        .temporal_q(temporal_query::after("2020-07-01T00:00:00Z".parse()?).last_n(1))
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20)))));
    Ok(())
}
