//! Aggregated values without aggr methods is a bad request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::answered, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Property, evolution::TemporalEntity};

/// Pins: Retrieve Temporal Evolution with format `aggregatedValues` and no `aggrMethods` answers 400
/// Clause: 4.5.19.1, 6.3.12, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0004_aggregated_values_without_aggr_methods_is_a_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type)
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    // The typed aggregated retrieval refuses to send without its methods, so the query is sent as written.
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &target(&format!("temporal/entities/{agg_id}"), &[("format", "aggregatedValues")]),
            &[("Accept", "application/json")],
            None,
        )?)
        .await;
    expect_that!(&refused, answered(StatusCode::BAD_REQUEST).with_title());
    Ok(())
}
