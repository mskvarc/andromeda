//! A distance sorting style is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
    written_temporal_query::{get, post},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{ErrorType, Property, evolution::TemporalEntity};

/// Pins: Query Temporal Evolution of Entities with `orderBy` id;dist-asc or id;dist-desc answers 400
/// `BadRequestData` on the GET and the POST binding
/// Clause: 4.23.1, 5.7.4.3, 5.7.4.4, Table 6.18.3.2-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9024_a_distance_sorting_style_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let ids = ["collation2", "collation10", "collation30"]
        .into_iter()
        .map(|suffix| namespace.urn("CollationReading", suffix))
        .collect::<Result<Vec<_>, _>>()?;
    for id in &ids {
        let reading = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(collation_reading_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&reading).send().await?.created()?;
    }
    // A distance ordering is refused by the temporal query before sending, so the requests are sent as written.
    let body = |order_by: &str| {
        format!(
            r#"{{"type": "Query", "entities": [{{"type": "CollationReading_{namespace}", "id": ["{}", "{}", "{}"]}}], "temporalQ": {{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}, "ordering": {{"orderBy": "{order_by}"}}}}"#,
            ids[0], ids[1], ids[2]
        )
    };
    let answer_get_1 = get(
        &broker,
        &format!("timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&type=CollationReading_{namespace}&local=true&orderBy=id%3Bdist-asc"),
        "application/json",
    )
    .await?;
    expect_that!(&answer_get_1, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_post_1 = post(&broker, "local=true", &body("id;dist-asc"), "application/json", "application/json").await?;
    expect_that!(&answer_post_1, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_get_2 = get(
        &broker,
        &format!("timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&type=CollationReading_{namespace}&local=true&orderBy=id%3Bdist-desc"),
        "application/json",
    )
    .await?;
    expect_that!(&answer_get_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_post_2 = post(&broker, "local=true", &body("id;dist-desc"), "application/json", "application/json").await?;
    expect_that!(&answer_post_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
