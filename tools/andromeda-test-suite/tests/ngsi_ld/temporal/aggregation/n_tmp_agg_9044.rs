//! Format wins over options for the aggregation parameters in retrieve query and post.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
    values_matcher::has_values,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, RepresentationSpelling, RepresentationToken},
    },
    model::{
        ErrorType,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: format wins over options when both name a representation: format=aggregatedValues beside
/// options=temporalValues aggregates with aggrMethods, and format=temporalValues beside options=aggregatedValues and
/// aggrMethods answers 200 temporal values on Retrieve, Query and the POST query
/// Clause: 6.3.12, Table 6.18.3.2-1, Table 6.19.3.1-1, 6.3.20
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9044_format_wins_over_options_for_the_aggregation_parameters_in_retrieve_query_and_post() -> Result<()> {
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
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let aggregated = json
        .temporal()
        .retrieve_aggregated(&agg_id)
        .aggregation_methods("sum".parse()?)
        .spelling(RepresentationSpelling::default().format_over_options(RepresentationToken::TemporalValues))
        .send()
        .await?;
    expect_that!(aggregated.body(), has_aggregate("speed", AggregationMethod::Sum, nth(0, period().value(300))));
    let values = json
        .temporal()
        .retrieve_values(&agg_id)
        .aggregation_methods("sum".parse()?)
        .spelling(RepresentationSpelling::default().format_over_options(RepresentationToken::AggregatedValues))
        .send()
        .await?;
    expect_that!(values.body(), has_values("speed", len(eq(2))));
    let queried = json
        .temporal()
        .query_values(
            temporal_query::after("2023-12-31T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type(&agg_type)
        .spelling(RepresentationSpelling::default().format_over_options(RepresentationToken::AggregatedValues))
        .send()
        .await?;
    expect_that!(queried.items(), len(eq(1)));
    expect_that!(queried.items().first(), some(has_values("speed", len(eq(2)))));
    let posted = json
        .temporal()
        .query_values(
            temporal_query::after("2023-12-31T00:00:00Z".parse()?)
                .aggregation_methods([AggregationMethod::Sum])
                .aggregation_period("PT1H".parse()?),
        )
        .entity_type(&agg_type)
        .method(QueryMethod::Post)
        .spelling(RepresentationSpelling::default().format_over_options(RepresentationToken::AggregatedValues))
        .send()
        .await?;
    expect_that!(posted.items(), len(eq(1)));
    expect_that!(posted.items().first(), some(has_values("speed", len(eq(2)))));
    let refused = json
        .with_responses()
        .temporal()
        .retrieve_values(&agg_id)
        .aggregation_methods("sum".parse()?)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
