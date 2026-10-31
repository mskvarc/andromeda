//! Q reads every dataset and the dataset step narrows the answer.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    values_matcher::{has_values, pair},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{DatasetId, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over v observed 10 by default and 200 in a named dataset selects the
/// Entity for v>100 with no `datasetId`, with `datasetId=@none` and with the named dataset, nothing for v>300 with
/// `datasetId=@none`, and with q=v>100 and `datasetId=@none` answers v holding the default pair 10 alone
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9038_q_reads_every_dataset_and_the_dataset_step_narrows_the_answer() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_dataset_id = namespace.urn("TqDataset", 1)?;
    let tq_dataset_type = namespace.entity_type("TqDataset")?;
    let sensor = namespace.dataset_id("Dataset", "sensor")?;
    let tq_dataset_evolution = TemporalEntity::builder()
        .id(tq_dataset_id.clone())
        .entity_type(tq_dataset_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property(
            "v",
            Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?).dataset_id(sensor.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&tq_dataset_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let window = || -> Result<_> { Ok(temporal_query::after("2019-01-01T00:00:00Z".parse()?)) };
    let any_dataset = json
        .temporal()
        .query(window()?)
        .entity_type(&tq_dataset_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(any_dataset.items(), len(eq(1)));
    expect_that!(any_dataset.items(), nth(0, has_id(&tq_dataset_id)));
    let default_instance = json
        .temporal()
        .query(window()?)
        .entity_type(&tq_dataset_type)
        .q(q::attr("v").gt(100))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(default_instance.items(), len(eq(1)));
    expect_that!(default_instance.items(), nth(0, has_id(&tq_dataset_id)));
    let named = json
        .temporal()
        .query(window()?)
        .entity_type(&tq_dataset_type)
        .q(q::attr("v").gt(100))
        .dataset_ids(vec![sensor])
        .send()
        .await?;
    expect_that!(named.items(), len(eq(1)));
    expect_that!(named.items(), nth(0, has_id(&tq_dataset_id)));
    let beyond = json
        .temporal()
        .query(window()?)
        .entity_type(&tq_dataset_type)
        .q(q::attr("v").gt(300))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(beyond.items(), is_empty());
    let values = json
        .temporal()
        .query_values(window()?)
        .entity_type(&tq_dataset_type)
        .q(q::attr("v").gt(100))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(values.items(), len(eq(1)));
    expect_that!(
        values.items().first(),
        some(has_values(
            "v",
            all!(len(eq(1)), nth(0, pair().value(10).time("2020-01-01T00:00:00Z".parse()?)))
        ))
    );
    Ok(())
}
