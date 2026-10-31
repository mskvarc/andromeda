//! A between window closing at or before its opening is bad request data by get and by post.

use andromeda_test_suite::{
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with timerel between and an `endTimeAt` equal to `timeAt` (also
/// spelled with a zero fraction), before it, or a microsecond before it answers 400 `BadRequestData` by GET and by
/// POST, and a window one microsecond wide reads the instance at its opening instant
/// Clause: 4.11, 4.6.3, Table 6.18.3.2-1, Table 5.2.21-1, 5.7.4.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9069_a_between_window_closing_at_or_before_its_opening_is_bad_request_data_by_get_and_by_post() -> Result<()> {
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
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-01T00:00:00.000Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-02-01T00:00:00Z".parse()?, "2026-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between(
            "2026-01-01T00:00:00.000002Z".parse()?,
            "2026-01-01T00:00:00.000001Z".parse()?,
        ))
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-02-01T00:00:00Z".parse()?, "2026-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .send()
        .await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    // Clause 4.11: the lower bound is inclusive and the upper exclusive, so the narrowest window still holds its opening instant.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-01T00:00:00.000001Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), nth(0, all!(has_id(&vehicle_id), has_evolution("speed", len(eq(1))))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2026-01-01T00:00:00Z".parse()?, "2026-01-01T00:00:00.000001Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_id)));
    Ok(())
}
