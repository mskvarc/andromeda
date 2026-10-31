//! Deleting the window holding the string makes sum eligible again.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::{answered, problem},
    temporal_delete_by_query::delete_request,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalAttributeName, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: a reading holding a number and a string instance refuses sum with 400 `InvalidRequest`, and answers the
/// sum of the number once a temporal delete-by-query removes the window holding the string instance
/// Clause: 5.7.4.4, 4.11, 6.18.3
/// Recorded: Andromeda extension, not v1.9.1: clause 6.18.3 binds no DELETE to the temporal entities collection;
/// the `temporal_delete_by_query` extension adds it, and the attributes left are judged over the instances that
/// exist
#[gtest]
#[tokio::test]
async fn a_ext_dbq_0006_deleting_the_window_holding_the_string_makes_sum_eligible_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let reading = namespace.suffixed("reading");
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(reading.as_str(), Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property(reading.as_str(), Property::builder("high").observed_at("2024-03-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let refused = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![TemporalAttributeName::Attribute(namespace.attribute("reading")?)])
        .send()
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // No operation of the specification deletes temporal evolutions by a query, so the request is sent as written.
    let request = delete_request(
        &broker,
        &format!(
            "id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&drop={reading}&timerel=between&timeAt=2024-02-01T00%3A00%3A00Z&endTimeAt=2024-04-01T00%3A00%3A00Z"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&vehicle_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Sum]))
        .attrs(vec![TemporalAttributeName::Attribute(namespace.attribute("reading")?)])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(&reading, AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(1))))
    );
    Ok(())
}
