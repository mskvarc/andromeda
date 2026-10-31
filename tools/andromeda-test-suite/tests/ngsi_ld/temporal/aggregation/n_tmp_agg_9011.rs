//! The methods an attribute admits follow its own values not its name.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        Value,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: avg, min, max and sum over a JSON Array Property observed as arrays of 2 and 4 elements answer 3, 2, 4 and
/// 6 while an Entity of another type holds a String Property of the same name
/// Clause: Table 4.5.19.1-1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9011_the_methods_an_attribute_admits_follow_its_own_values_not_its_name() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_own_values_id = namespace.urn("AggregationSubject", "own-values")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let aggregation_subject_other_values_id = namespace.urn("AggregationSubject", "other-values")?;
    let other_aggregation_subject_type = namespace.entity_type("OtherAggregationSubject")?;
    let aggregation_subject_own_values_evolution = TemporalEntity::builder()
        .id(aggregation_subject_own_values_id.clone())
        .entity_type(aggregation_subject_type)
        .property(
            namespace.attribute("share")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2)])),
            ]))
            .observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("share")?,
            Property::builder(object([
                ("@type", Value::from("@json")),
                ("@value", Value::Array(vec![Value::from(1), Value::from(2), Value::from(3), Value::from(4)])),
            ]))
            .observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_own_values_evolution)
        .send()
        .await?
        .created()?;
    let aggregation_subject_other_values_evolution = TemporalEntity::builder()
        .id(aggregation_subject_other_values_id)
        .entity_type(other_aggregation_subject_type)
        .property(
            namespace.attribute("share")?,
            Property::builder("beta").observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("share")?,
            Property::builder("alpha").observed_at("2024-01-01T01:00:00Z".parse()?),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_other_values_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_own_values_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
        .attrs(vec![format!("share_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate(
            &format!("share_{namespace}"),
            AggregationMethod::Avg,
            all!(len(eq(1)), nth(0, period().value(3)))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(&aggregation_subject_own_values_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([
            AggregationMethod::Min,
            AggregationMethod::Max,
            AggregationMethod::Sum,
        ]))
        .attrs(vec![format!("share_{namespace}").parse()?])
        .send()
        .await?;
    let aggregated_2 = retrieved_2.body();
    expect_that!(
        aggregated_2,
        has_aggregate(
            &format!("share_{namespace}"),
            AggregationMethod::Min,
            all!(len(eq(1)), nth(0, period().value(2)))
        )
    );
    expect_that!(
        aggregated_2,
        has_aggregate(
            &format!("share_{namespace}"),
            AggregationMethod::Max,
            all!(len(eq(1)), nth(0, period().value(4)))
        )
    );
    expect_that!(
        aggregated_2,
        has_aggregate(
            &format!("share_{namespace}"),
            AggregationMethod::Sum,
            all!(len(eq(1)), nth(0, period().value(6)))
        )
    );
    Ok(())
}
