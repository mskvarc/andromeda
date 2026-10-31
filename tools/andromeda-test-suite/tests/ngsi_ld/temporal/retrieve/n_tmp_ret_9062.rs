//! An end time at outside a between window is bad request data.

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

/// Pins: Retrieve Temporal Evolution of an Entity with an endTimeAt and no timerel, with timerel after and an endTimeAt,
/// or with a between whose endTimeAt is before its timeAt answers 400 `BadRequestData`, and a well-formed between answers
/// 200
/// Clause: 4.11, Table 6.19.3.1-1, 5.7.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9062_an_end_time_at_outside_a_between_window_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for query in [
        "endTimeAt=2026-02-01T00:00:00Z",
        "timerel=after&timeAt=2025-01-01T00:00:00Z&endTimeAt=2026-02-01T00:00:00Z",
        "timerel=between&timeAt=2026-02-01T00:00:00Z&endTimeAt=2025-01-01T00:00:00Z",
    ] {
        // The client builds only a well-formed window, so the query string is sent as written.
        let request = request_to(&client, Method::GET, &format!("temporal/entities/{vehicle_id}?{query}"))?
            .append_header(ACCEPT, HeaderValue::from_static("application/json"));
        let answer = client.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
        );
    }
    let retrieved = client
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::between("2025-01-01T00:00:00Z".parse()?, "2026-02-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(1)))));
    Ok(())
}
