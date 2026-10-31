//! A zero last n is bad request data by get and by post.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
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

/// Pins: Query Temporal Evolution of Entities with `lastN` 0 answers 400 `BadRequestData` by GET and by POST, and
/// with `lastN` 1 answers the one most recent instance
/// Clause: Table 6.18.3.2-1, Table 5.2.21-1, 5.5.4, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9063_a_zero_last_n_is_bad_request_data_by_get_and_by_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_evolution = TemporalEntity::builder()
        .id(device_id)
        .entity_type(device_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(20).observed_at("2020-08-01T13:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&device_evolution).send().await?.created()?;
    // A `lastN` of 0 is no count the temporal query writes, so the requests are sent as written.
    let device_type_text = device_type.to_string();
    let answer = get_parameters(
        &broker,
        &[
            ("type", device_type_text.as_str()),
            ("timerel", "after"),
            ("timeAt", "2020-07-01T00:00:00Z"),
            ("lastN", "0"),
        ],
        "application/json",
    )
    .await?;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = post(
        &broker,
        "",
        &format!(
            r#"{{"type": "Query", "entities": [{{"type": "{device_type_text}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2020-07-01T00:00:00Z", "lastN": 0}}}}"#
        ),
        "application/json",
        "application/json",
    )
    .await?;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?).last_n(1))
        .entity_type(&device_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items().first(),
        some(has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(20)))))
    );
    Ok(())
}
