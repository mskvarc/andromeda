//! A multi target relationship keeps every target.

use andromeda_test_suite::{
    attribute_matcher::relationship,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    simplified_pairs::object_pairs,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{DateTime, NgsiLdMediaType, OneOrMany, Relationship, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A temporal Relationship observed with two targets and then three answers both target arrays whole, as the
/// object of each instance and as the first element of each objects pair under temporalValues
/// Clause: 4.5.3.2, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9011_a_multi_target_relationship_keeps_every_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_multi_target_id = namespace.urn("AggregationSubject", "multi-target")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let device_14142_id = namespace.urn("Device", 14142)?;
    let device_13562_id = namespace.urn("Device", 13562)?;
    let device_37309_id = namespace.urn("Device", 37309)?;
    let two = vec![device_14142_id.clone(), device_13562_id.clone()];
    let three = vec![device_14142_id, device_13562_id, device_37309_id];
    let midnight = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let one = "2024-01-01T01:00:00Z".parse::<DateTime>()?;
    let aggregation_subject_multi_target_evolution = TemporalEntity::builder()
        .id(aggregation_subject_multi_target_id.clone())
        .entity_type(aggregation_subject_type)
        .relationship("activeDevices", Relationship::builder(two.clone()).observed_at(midnight))
        .relationship("activeDevices", Relationship::builder(three.clone()).observed_at(one))
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_multi_target_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&aggregation_subject_multi_target_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "activeDevices",
            all!(
                len(eq(2)),
                contains(relationship().object(OneOrMany::try_from(two.clone())?).observed_at(midnight)),
                contains(relationship().object(OneOrMany::try_from(three.clone())?).observed_at(one))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&aggregation_subject_multi_target_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let pairs = object_pairs(retrieved_2.body(), "activeDevices").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    let first = pairs.iter().find(|pair| pair.time.timestamp() == midnight.timestamp()).or_fail()?;
    expect_that!(first.value.as_slice(), eq(two.as_slice()));
    let second = pairs.iter().find(|pair| pair.time.timestamp() == one.timestamp()).or_fail()?;
    expect_that!(second.value.as_slice(), eq(three.as_slice()));
    Ok(())
}
