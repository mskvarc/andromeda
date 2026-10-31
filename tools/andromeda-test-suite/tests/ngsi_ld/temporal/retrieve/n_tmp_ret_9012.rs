//! A dataset no instance carries keeps the entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Retrieve Temporal Evolution of an Entity with a datasetId no instance carries answers 200 with the Entity and
/// no Attribute, with the named datasetId answers only its Attribute, and with a window holding no data answers 404
/// Clause: 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9012_a_dataset_no_instance_carries_keeps_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tds_filter_id = namespace.urn("TdsFilter", 1)?;
    let tds_filter_type = namespace.entity_type("TdsFilter")?;
    let named = namespace.dataset_id("Dataset", "named")?;
    let absent = namespace.dataset_id("Dataset", "absent")?;
    let tds_filter_evolution = TemporalEntity::builder()
        .id(tds_filter_id.clone())
        .entity_type(tds_filter_type)
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuel",
            Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?).dataset_id(named.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&tds_filter_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&tds_filter_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![absent])
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(&evolution.id, eq(&tds_filter_id));
    expect_that!(evolution, lacks_evolution("speed"));
    expect_that!(evolution, lacks_evolution("fuel"));
    let retrieved_2 = client
        .temporal()
        .retrieve(&tds_filter_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![named.clone()])
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, lacks_evolution("speed"));
    expect_that!(evolution_2, has_evolution("fuel", contains(any_attribute().value(40).with_dataset_id(named))));
    let refused = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&tds_filter_id)
        .temporal_q(temporal_query::before("2000-01-01T00:00:00Z".parse()?))
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await;
    expect_that!(&refused, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
