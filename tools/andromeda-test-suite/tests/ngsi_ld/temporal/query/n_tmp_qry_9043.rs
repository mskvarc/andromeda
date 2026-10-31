//! A dataset no instance carries keeps the entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, NgsiLdMediaType, OneOrMany, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with a `datasetId` no instance carries answers the Entity with its id
/// and type and no Attribute, with the dataset of fuel answers fuel alone, and with @none answers speed alone
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9043_a_dataset_no_instance_carries_keeps_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tqds_keep_id = namespace.urn("TqdsKeep", 1)?;
    let tqds_keep_type = namespace.entity_type("TqdsKeep")?;
    let named = namespace.dataset_id("Dataset", "named")?;
    let tqds_keep_evolution = TemporalEntity::builder()
        .id(tqds_keep_id.clone())
        .entity_type(tqds_keep_type.clone())
        .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property(
            "fuel",
            Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?).dataset_id(named.clone()),
        )
        .build()?;
    broker.temporal().create_or_update(&tqds_keep_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let absent = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_keep_type)
        .dataset_ids(vec![namespace.dataset_id("Dataset", "absent")?])
        .send()
        .await?;
    expect_that!(absent.items(), len(eq(1)));
    expect_that!(absent.items(), nth(0, has_id(&tqds_keep_id)));
    let kept = absent.items().first().or_fail()?;
    expect_that!(kept.entity_type, eq(&OneOrMany::one(tqds_keep_type.clone())));
    expect_that!(kept.attributes.len(), eq(0));
    expect_that!(kept.scope, is_empty());
    let fuel = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_keep_type)
        .dataset_ids(vec![named])
        .send()
        .await?;
    expect_that!(fuel.items(), len(eq(1)));
    expect_that!(fuel.items().first(), some(has_evolution("fuel", nth(0, any_attribute().value(40)))));
    expect_that!(fuel.items().first().map(|entity| entity.attributes.len()), some(eq(1)));
    let speed = json
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&tqds_keep_type)
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(speed.items(), len(eq(1)));
    expect_that!(speed.items().first(), some(has_evolution("speed", nth(0, any_attribute().value(10)))));
    expect_that!(speed.items().first(), some(lacks_evolution("fuel")));
    expect_that!(speed.items().first().map(|entity| entity.attributes.len()), some(eq(1)));
    Ok(())
}
