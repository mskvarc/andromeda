//! A temporal q reads every instance before a datasetId narrows the representation.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: over a Meter observed inside a 12:00 to 13:00 window with `n` 10 as its default instance and 60 in a dataset
/// of its own, Query Temporal Evolution of Entities with q n>50 and datasetId @none selects the Meter through the 60
/// instance and represents `n` by the default instance 10 alone; q is checked at step S2 against every instance and
/// the datasetId filters the represented instances at step S8
/// Clause: 5.7.4.4, 4.5.5, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9385_a_temporal_q_reads_every_instance_before_a_dataset_id_narrows_the_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_id = namespace.urn("Meter", 1)?;
    let evolution = TemporalEntity::builder()
        .id(meter_id.clone())
        .entity_type(meter_type.clone())
        .property("n", Property::builder(10).observed_at("2018-08-01T12:01:00Z".parse()?))
        .property(
            "n",
            Property::builder(60)
                .observed_at("2018-08-01T12:02:00Z".parse()?)
                .dataset_id(namespace.dataset_id("Dataset", 1)?),
        )
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let answer = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&meter_type)
        .q(q::attr("n").gt(50))
        .dataset_ids(vec!["@none".parse::<DatasetId>()?])
        .send()
        .await?;
    expect_that!(answer.items(), len(eq(1)));
    expect_that!(answer.items(), nth(0, has_id(&meter_id)));
    expect_that!(answer.items(), nth(0, has_evolution("n", all!(len(eq(1)), nth(0, any_attribute().value(10))))));
    Ok(())
}
