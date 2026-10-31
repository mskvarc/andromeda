//! An aggregation over an ineligible attribute is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities as `aggregatedValues` with `aggrMethods` sum over a string attribute
/// is refused 400 `InvalidRequest`
/// Clause: 5.7.4.4, 4.5.19
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7417_an_aggregation_over_an_ineligible_attribute_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("BMW").observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .query_aggregated(temporal_query::before("2030-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .entity_type(&vehicle_type)
        .attrs(vec!["brandName".parse()?])
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
