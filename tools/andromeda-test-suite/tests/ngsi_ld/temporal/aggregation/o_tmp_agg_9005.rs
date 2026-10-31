//! Avg over a date or a date time property is refused.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Date,
        DateTime,
        ErrorType,
        NgsiLdMediaType,
        Property,
        Value,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg over a Date or a `DateTime` Property named in attrs answers 400 `InvalidRequest`, while min and max
/// over each answer a string
/// Clause: Table 4.5.19.1-2, 5.7.3.4
/// Recorded: Tables 4.5.19.1-1 to 4.5.19.1-3 mark a method N/A for a value datatype and do not say how N/A is
/// answered; Andromeda refuses a request whose attrs names an attribute of that datatype with 400 `InvalidRequest`
#[gtest]
#[tokio::test]
async fn o_tmp_agg_9005_avg_over_a_date_or_a_date_time_property_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subject_id = namespace.urn("AggregationSubject", "temporal-na")?;
    let subject_type = namespace.entity_type("AggregationSubject")?;
    let entry_date = namespace.suffixed("entryDate");
    let seen_at = namespace.suffixed("seenAt");
    let subject_evolution = TemporalEntity::builder()
        .id(subject_id.clone())
        .entity_type(subject_type)
        .property(
            namespace.attribute("entryDate")?,
            Property::builder(Value::from("2024-03-01".parse::<Date>()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("entryDate")?,
            Property::builder(Value::from("2024-03-03".parse::<Date>()?)).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("seenAt")?,
            Property::builder(Value::from("2024-03-01T05:00:00Z".parse::<DateTime>()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("seenAt")?,
            Property::builder(Value::from("2024-03-01T07:00:00Z".parse::<DateTime>()?)).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&subject_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for attribute in [&entry_date, &seen_at] {
        let refused = json
            .with_responses()
            .temporal()
            .retrieve_aggregated(&subject_id)
            .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
            .attrs(vec![attribute.parse()?])
            .send()
            .await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
        let extremes = json
            .temporal()
            .retrieve_aggregated(&subject_id)
            .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Min, AggregationMethod::Max]))
            .attrs(vec![attribute.parse()?])
            .send()
            .await?;
        for method in [AggregationMethod::Min, AggregationMethod::Max] {
            expect_that!(extremes.body(), has_aggregate(attribute, method, all!(len(eq(1)), nth(0, period().text()))));
        }
    }
    Ok(())
}
