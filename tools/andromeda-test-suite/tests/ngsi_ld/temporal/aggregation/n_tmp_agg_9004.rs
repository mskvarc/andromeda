//! Avg over a time property is the average time of day.

use andromeda_test_suite::{aggregation_matcher::has_aggregate, broker, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        Time,
        Value,
        evolution::{AggregatedValue, AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg over a Time Property observed as 08:00:00Z and 10:00:00Z answers a time starting 09:00:00
/// Clause: Table 4.5.19.1-2, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9004_avg_over_a_time_property_is_the_average_time_of_day() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subject_id = namespace.urn("AggregationSubject", "time-avg")?;
    let subject_type = namespace.entity_type("AggregationSubject")?;
    let entry_time = namespace.attribute("entryTime")?;
    let subject = TemporalEntity::builder()
        .id(subject_id.clone())
        .entity_type(subject_type)
        .property(
            entry_time.clone(),
            Property::builder(Value::from("08:00:00Z".parse::<Time>()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            entry_time.clone(),
            Property::builder(Value::from("10:00:00Z".parse::<Time>()?)).observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&subject).send().await?.created()?;
    let aggregated = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&subject_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![entry_time.to_string().parse()?])
        .send()
        .await?;
    let time_of_day = |period: &AggregatedValue| match &period.value {
        Value::Text(text) => text.starts_with("09:00:00"),
        Value::Typed(typed) => typed.lexical().starts_with("09:00:00"),
        Value::Boolean(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) | Value::JsonNull => false,
    };
    expect_that!(
        aggregated.body(),
        has_aggregate(entry_time.as_ref(), AggregationMethod::Avg, all!(len(eq(1)), nth(0, predicate(time_of_day))))
    );
    Ok(())
}
