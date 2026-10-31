//! An aggregated query keeps the entity the dataset step empties.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_exists},
    broker,
    namespace::Namespace,
    outcome::Upserted,
    written_temporal_query::get_parameters,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        OneOrMany,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with `aggregatedValues` and max answers the Entity with both
/// Attributes, and with a `datasetId` no instance carries answers it with its id and type and no Attribute
/// Clause: 5.7.4.4, 4.5.19
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9046_an_aggregated_query_keeps_the_entity_the_dataset_step_empties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tqds_aggr_id = namespace.urn("TqdsAggr", 1)?;
    let tqds_aggr_type = namespace.entity_type("TqdsAggr")?;
    let tqds_aggr_evolution = TemporalEntity::builder()
        .id(tqds_aggr_id.clone())
        .entity_type(tqds_aggr_type.clone())
        .property(
            namespace.attribute("speed")?,
            Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .property(
            namespace.attribute("fuel")?,
            Property::builder(40)
                .observed_at("2020-08-01T12:00:00Z".parse()?)
                .dataset_id(namespace.dataset_id("Dataset", "named")?),
        )
        .build()?;
    broker.temporal().create_or_update(&tqds_aggr_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The aggregated reader refuses the `datasetId` member of an aggregated attribute, so this answer is read as JSON.
    let tqds_aggr_type_text = tqds_aggr_type.to_string();
    let both = get_parameters(
        &broker,
        &[
            ("type", tqds_aggr_type_text.as_str()),
            ("timerel", "before"),
            ("timeAt", "2030-01-01T00:00:00Z"),
            ("format", "aggregatedValues"),
            ("aggrMethods", "max"),
        ],
        "application/json",
    )
    .await?
    .or_fail()?;
    expect_that!(both.status(), eq(StatusCode::OK));
    expect_that!(both.body(), json_count("$", 1));
    expect_that!(both.body(), json_exists(&format!("$[0].speed_{namespace}")));
    expect_that!(both.body(), json_exists(&format!("$[0].fuel_{namespace}")));
    expect_that!(both.body(), json_count("$[0].*", 4));
    expect_that!(both.body(), json_equals(&format!("$[0].speed_{namespace}.max[0][0]"), 10));
    expect_that!(both.body(), json_equals(&format!("$[0].fuel_{namespace}.max[0][0]"), 40));
    let emptied = json
        .temporal()
        .query_aggregated(temporal_query::before("2030-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Max]))
        .entity_type(&tqds_aggr_type)
        .dataset_ids(vec![namespace.dataset_id("Dataset", "absent")?])
        .send()
        .await?;
    expect_that!(emptied.items(), len(eq(1)));
    let emptied_entity = emptied.items().first().or_fail()?;
    expect_that!(emptied_entity.id, eq(&tqds_aggr_id));
    expect_that!(emptied_entity.entity_type, eq(&OneOrMany::one(tqds_aggr_type)));
    expect_that!(emptied_entity.attributes.len(), eq(0));
    Ok(())
}
