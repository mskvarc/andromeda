//! Aggregation parameters without the aggregated format are invalid request.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
};

/// Pins: `aggrMethods` or `aggrPeriodDuration` without `aggregatedValues` in format or options answers 400
/// `InvalidRequest` on Retrieve and Query Temporal Evolution, while `options=aggregatedValues` with
/// `aggrMethods=sum` answers 200 with the sum
/// Clause: 6.3.20, Table 6.18.3.2-1, Table 6.19.3.1-1
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9023_aggregation_parameters_without_the_aggregated_format_are_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let agg_id = namespace.urn("Agg", 1)?;
    let agg_type = namespace.entity_type("Agg")?;
    let agg_evolution = TemporalEntity::builder()
        .id(agg_id.clone())
        .entity_type(agg_type.clone())
        .property("speed", Property::builder(100).observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("speed", Property::builder(200).observed_at("2024-01-01T01:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&agg_evolution).send().await?.created()?;
    let path = format!("temporal/entities/{agg_id}");
    let aggr_type = agg_type.to_string();
    let window = [("type", aggr_type.as_str()), ("timerel", "after"), ("timeAt", "2023-12-31T00:00:00Z")];
    // The temporal builders write the aggregation parameters only beside the aggregated format, so each query is sent as written.
    let refused_targets = [
        target(&path, &[("aggrMethods", "sum")]),
        target(&path, &[("aggrPeriodDuration", "PT1H")]),
        target("temporal/entities", &[window.as_slice(), &[("aggrMethods", "sum")]].concat()),
        target(
            "temporal/entities",
            &[
                window.as_slice(),
                &[("format", "temporalValues"), ("aggrMethods", "sum"), ("aggrPeriodDuration", "PT1H")],
            ]
            .concat(),
        ),
    ];
    for refused_target in &refused_targets {
        let refused = broker
            .send_unchecked(exchange(&broker, Method::GET, refused_target, &[("Accept", "application/json")], None)?)
            .await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    }
    let summed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("sum".parse()?)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(summed.body(), has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(300))));
    Ok(())
}
