//! An end time at beside before or after is bad request data by get and by post.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    written_temporal_query::{get_parameters, post},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with timerel before or after and an `endTimeAt` answers 400
/// `BadRequestData` by GET and by POST, and the same windows without the `endTimeAt` answer 200
/// Clause: 4.11, Table 6.18.3.2-1, Table 5.2.21-1, 5.7.4.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9068_an_end_time_at_beside_before_or_after_is_bad_request_data_by_get_and_by_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // An `endTimeAt` beside `before` or `after` is no window the temporal query writes, so the requests are sent as written.
    let vehicle_type_text = vehicle_type.to_string();
    for timerel in ["before", "after"] {
        let answer = get_parameters(
            &broker,
            &[
                ("type", vehicle_type_text.as_str()),
                ("timerel", timerel),
                ("timeAt", "2026-01-01T00:00:00Z"),
                ("endTimeAt", "2026-02-01T00:00:00Z"),
            ],
            "application/json",
        )
        .await?;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
        );
    }
    for timerel in ["before", "after"] {
        let answer = post(
            &broker,
            "",
            &format!(
                r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type_text}"}}], "temporalQ": {{"timerel": "{timerel}", "timeAt": "2026-01-01T00:00:00Z", "endTimeAt": "2026-02-01T00:00:00Z"}}}}"#
            ),
            "application/json",
            "application/json",
        )
        .await?;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
        );
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let after = json
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(after.items(), nth(0, has_id(&vehicle_id)));
    let before = json
        .temporal()
        .query(temporal_query::before("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(before.items(), is_empty());
    Ok(())
}
