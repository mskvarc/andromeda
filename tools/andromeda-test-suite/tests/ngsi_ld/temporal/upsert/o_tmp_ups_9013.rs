//! A series then an upsert of the latest reading keeps one core instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, has_instances},
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::{Done, Upserted},
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, UpsertMode},
    },
    model::{Entity, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::q,
};

/// Pins: Create Temporal Evolution of an Entity with v 10, 200 and 50 observed on successive days, then Batch
/// Entity Upsert with options=update restating 50, leaves Retrieve Entity answering v as one Property of value 50,
/// `keyValues` 50, and q=v==50 selecting the Entity and q=v==200 nothing
/// Clause: 4.5.5, 5.6.8.4, 5.6.11.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_tmp_ups_9013_a_series_then_an_upsert_of_the_latest_reading_keeps_one_core_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let crowd_flow_id = namespace.urn("CrowdFlow", 1)?;
    let crowd_flow_type = namespace.entity_type("CrowdFlow")?;
    let crowd_flow_evolution = TemporalEntity::builder()
        .id(crowd_flow_id.clone())
        .entity_type(crowd_flow_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&crowd_flow_evolution).send().await?.created()?;
    let entities = [Entity::builder()
        .id(crowd_flow_id.clone())
        .entity_type(crowd_flow_type.clone())
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&crowd_flow_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("v", 1));
    expect_that!(entity, has_attribute("v", any_attribute().value(50)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&crowd_flow_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("v", 50));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&crowd_flow_type)
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&crowd_flow_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&crowd_flow_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
