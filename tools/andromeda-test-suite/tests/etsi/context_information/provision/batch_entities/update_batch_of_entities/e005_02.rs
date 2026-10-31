//! A batch of fragments with `options=noOverwrite` keeps the existing attributes of the entities, adds new ones, and
//! reports the update of an existing attribute in 207 (clause 5.6.9).

use crate::{
    batch_operations::{expect_batch_done, partial_batch_outcome},
    building_documents::{located_building_json, simple_building_json},
    building_payloads::{BUILDING_REPLACE_TERMS, BUILDING_SIMPLE_TERMS, building_simple_attributes},
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client, query_entities_json},
    geometry_payloads::point,
    linked_place_payloads::building_located_in_paris,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Complete, Entity, EntityFragment, NgsiLdMediaType},
};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` named Eiffel Tower and sub-categorised as tourism, with a `location`
/// `GeoProperty` at 13.3986, 52.5547.
fn located_geo_fragment(namespace: &Namespace, n: u32) -> Result<EntityFragment<Complete>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .geo_property("location", point(13.3986, 52.5547)?)
        .build()?
        .into_fragment())
}

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_02_01_entity_with_existing_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_simple_attributes(&namespace, 1)?.into_fragment(),
        building_simple_attributes(&namespace, 2)?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .overwrite(Overwrite::Keep)
        .send()
        .await;
    partial_batch_outcome(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            simple_building_json(&namespace, 1, context),
            simple_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_02_02_entity_with_new_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_located_in_paris(&namespace, "1")?.into_fragment(),
        building_located_in_paris(&namespace, "2")?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .overwrite(Overwrite::Keep)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            located_building_json(&namespace, 1, context),
            located_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_02_03_entity_with_new_and_existing_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [located_geo_fragment(&namespace, 1)?, located_geo_fragment(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .overwrite(Overwrite::Keep)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    let geo_located = |n: u32| {
        let mut building = simple_building_json(&namespace, n, context);
        building["location"] = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}});
        building
    };
    expect_that!(&body, matches_document(json!([geo_located(1), geo_located(2)])));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
