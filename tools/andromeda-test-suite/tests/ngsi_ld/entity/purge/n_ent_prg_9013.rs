//! A purge under a map is held to its query alone.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Purge Entities by type and two ids under an `EntityMap` that Create `EntityMap` for Query Entities created by
/// that type, those ids and lang fr answers 204 and deletes both Entities
/// Clause: 5.5.14, 5.6.21.4, Table 6.4.3.3-2
/// Recorded: clause 5.5.14 does not name Purge Entities among the requests under an `EntityMap`, so a purge is held
/// to the query of the request that created the map and to none of its other inputs, which Purge Entities does not
/// take
#[gtest]
#[tokio::test]
async fn n_ent_prg_9013_a_purge_under_a_map_is_held_to_its_query_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("floors", 3)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("floors", 4)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .lang("fr".parse::<LanguageFilter>()?)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_header = created.entity_map().valid().or_fail()?;
    let received = broker
        .with_heads()
        .entities()
        .purge()
        .entity_type(&building_type)
        .ids(vec![building_id.clone(), building_2_id.clone()])
        .entity_map(map_header)
        .send()
        .await?;
    received.outcome.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), is_empty());
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
