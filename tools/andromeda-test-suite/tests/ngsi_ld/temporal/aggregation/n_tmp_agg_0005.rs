//! An aggregation method outside the list is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, raw_call::target, unchecked_call::exchange};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{ErrorType, Property, evolution::TemporalEntity};

/// Pins: Retrieve Temporal Evolution with `aggrMethods` bogus answers 400 `BadRequestData`
/// Clause: 4.5.19.1, 6.3.12, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_0005_an_aggregation_method_outside_the_list_is_bad_request_data() -> Result<()> {
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
    // `bogus` is no aggregation method the temporal builder names, so the query is sent as written.
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &target(
                &format!("temporal/entities/{agg_id}"),
                &[("format", "aggregatedValues"), ("aggrMethods", "bogus")],
            ),
            &[("Accept", "application/json")],
            None,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
