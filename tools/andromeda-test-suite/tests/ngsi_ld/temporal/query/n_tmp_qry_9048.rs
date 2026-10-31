//! A window without data still removes the entity.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{DatasetId, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities before 2000 over instances of 2020 answers nothing, with or without
/// `datasetId=@none`
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9048_a_window_without_data_still_removes_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tqds_window_id = namespace.urn("TqdsWindow", 1)?;
    let tqds_window_type = namespace.entity_type("TqdsWindow")?;
    let tqds_window_evolution = TemporalEntity::builder()
        .id(tqds_window_id.clone())
        .entity_type(tqds_window_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuel",
            Property::builder(40)
                .observed_at("2020-08-01T12:00:00Z".parse()?)
                .dataset_id(namespace.dataset_id("Dataset", "named")?),
        )
        .build()?;
    broker.temporal().create_or_update(&tqds_window_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let before = json
        .temporal()
        .query(temporal_query::before("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_window_type)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(before.items(), is_empty());
    let before_2 = json
        .temporal()
        .query(temporal_query::before("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_window_type)
        .count(ResultsCount::Include)
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(before_2.items(), is_empty());
    Ok(())
}
