//! ETSI `057_01`: a batch of fragments is merged into two existing entities, and the broker retrieves the merged
//! entities (clause 5.6.17).

use crate::{
    suite_keywords::{
        create_entity_with_context_string,
        delete_entities,
        expect_batch_done,
        ld_json_body_client,
        located_building_json,
        query_entities_json,
        simple_building_json,
    },
    suite_payloads::{BUILDING_REPLACE_TERMS, BUILDING_SIMPLE_TERMS, building_located_in_paris, building_simple_attributes, pisa_tower},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Complete, EntityFragment, NgsiLdMediaType, Property};
use serde_json::{Value, json};

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` named Pisa Tower, as the broker retrieves it after a merge naming it.
fn pisa_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    json!({
        "id": namespace.urn_text("Building", n),
        "type": "Building",
        "name": {"type": "Property", "value": "Pisa Tower"},
        "subCategory": {"type": "Property", "value": "tourism"},
        "airQualityLevel": {
            "type": "Property",
            "value": 6,
            "unitCode": "C62",
            "observedAt": "2020-10-10T16:40:00.000Z"
        },
        "almostFull": {"type": "Property", "value": true},
        "@context": context,
    })
}

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` of [`simple_building_json`] whose `airQualityLevel` a partial merge
/// replaces with the value 1 at 2024-01-01T00:00:00Z, keeping the `unitCode` C62 of the entity.
fn partially_merged_building_json(namespace: &Namespace, n: u32, context: &str) -> Value {
    let mut building = simple_building_json(namespace, n, context);
    building["airQualityLevel"] = json!({
        "type": "Property",
        "value": 1,
        "unitCode": "C62",
        "observedAt": "2024-01-01T00:00:00.000Z"
    });
    building
}

/// Pins: Check that you can merge a batch of entities
/// Clause: 5.6.17
/// Changed: the setup entities are created with 201 required, where the Hurl setup accepts any status.
/// Changed: the `@context` of each retrieved entity is the vocabulary the request names, which the Hurl case also
/// accepts as any versioned core `@context`.
#[gtest]
#[tokio::test]
async fn e057_01_01_merge_an_empty_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building").build()?,
        EntityFragment::for_entity(namespace.urn("Building", 2)?, "Building").build()?,
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_merge(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            simple_building_json(&namespace, 1, context),
            simple_building_json(&namespace, 2, context)
        ]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that you can merge a batch of entities
/// Clause: 5.6.17
/// Changed: the setup entities are created with 201 required, where the Hurl setup accepts any status.
/// Changed: the `@context` of each retrieved entity is the vocabulary the request names, which the Hurl case also
/// accepts as any versioned core `@context`.
#[gtest]
#[tokio::test]
async fn e057_01_02_merge_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [pisa_tower(&namespace, 1)?.into_fragment(), pisa_tower(&namespace, 2)?.into_fragment()];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_merge(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([pisa_building_json(&namespace, 1, context), pisa_building_json(&namespace, 2, context)])).hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that you can merge a batch of entities
/// Clause: 5.6.17
/// Changed: the setup entities are created with 201 required, where the Hurl setup accepts any status.
/// Changed: the `@context` of each retrieved entity is the vocabulary the request names, which the Hurl case also
/// accepts as any versioned core `@context`.
#[gtest]
#[tokio::test]
async fn e057_01_03_merge_simple_relationship() -> Result<()> {
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
        .batch_merge(&fragments)
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
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that you can merge a batch of entities
/// Clause: 5.6.17
/// Changed: the setup entities are created with 201 required, where the Hurl setup accepts any status.
/// Changed: the `@context` of each retrieved entity is the vocabulary the request names, which the Hurl case also
/// accepts as any versioned core `@context`.
#[gtest]
#[tokio::test]
async fn e057_01_04_merge_property_with_partial_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let partial = |n: u32| -> Result<EntityFragment<Complete>> {
        Ok(EntityFragment::for_entity(namespace.urn("Building", n)?, "Building")
            .property("airQualityLevel", Property::builder(1).observed_at("2024-01-01T00:00:00.000Z".parse()?))
            .build()?)
    };
    let fragments = [partial(1)?, partial(2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_merge(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            partially_merged_building_json(&namespace, 1, context),
            partially_merged_building_json(&namespace, 2, context),
        ]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
