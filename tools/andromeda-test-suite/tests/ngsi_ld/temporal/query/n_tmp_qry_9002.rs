//! A temporal entity map sheds a deleted history.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities by type under an `EntityMap` of four Temporal Evolutions, after the
/// first page of two, answers a later page without the Entity whose Temporal Evolution was deleted in between, and
/// the `EntityMap` no longer lists it
/// Clause: 5.14.5, 4.3.6.7, 5.5.9.3
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9002_a_temporal_entity_map_sheds_a_deleted_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let buildings = (1..=4)
        .map(|index| {
            Ok(Entity::builder()
                .id(namespace.urn("Building", index)?)
                .entity_type(building_type.clone())
                .property(
                    "temperature",
                    Property::builder(Value::Number("30.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
                )
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&buildings).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let created = json
        .with_heads()
        .entity_maps()
        .create_temporal(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(&created.body().entity_map, len(eq(4)));
    let map_id = created.body().id.clone();
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let first_page = json
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&building_type)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    expect_that!(first_page.items(), len(eq(2)));
    let later_page = json
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&building_type)
        .limit(1)
        .offset(2)
        .send()
        .await?;
    expect_that!(later_page.items(), len(eq(1)));
    let victim = later_page.items().first().map(|entity| entity.id.clone()).or_fail()?;
    json.temporal().delete(&victim).send().await?;
    let remaining = json
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request)
        .entity_type(&building_type)
        .limit(100)
        .offset(0)
        .send()
        .await?;
    expect_that!(remaining.items(), len(eq(3)));
    expect_that!(remaining.items(), not(contains(has_id(&victim))));
    let retrieved = json.entity_maps().retrieve(&map_id).send().await?;
    expect_that!(&retrieved.entity_map, len(eq(3)));
    expect_that!(retrieved.entity_map.contains_key(&victim), eq(false));
    broker.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
