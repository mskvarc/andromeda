//! An upsert carrying long references creates every entity.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, relationship},
    body_json::json_count,
    broker,
    entity_matcher::has_attribute,
    exact_length,
    namespace::Namespace,
    outcome::UpsertCreated,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ListRelationship, NgsiLdMediaType, Relationship},
};

/// Pins: Batch Entity Upsert of a new Entity whose Relationship object is a URN of over 6,000 octets and a new
/// Entity whose `ListRelationship` member is one of over 4,096 answers 201, and both read back unchanged
/// Clause: 5.6.8, 4.6.2
#[gtest]
#[tokio::test]
async fn n_bat_ups_9006_an_upsert_carrying_long_references_creates_every_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let long_reference_batch_1_id = namespace.urn("LongReference", "batch-1")?;
    let long_reference_type = namespace.entity_type("LongReference")?;
    let long_reference_batch_2_id = namespace.urn("LongReference", "batch-2")?;
    let stop_id = exact_length::urn(&namespace, "Stop", 5982)?;
    let stop_time_update_id = exact_length::urn(&namespace, "StopTimeUpdate", 4068)?;
    let entities = [
        Entity::builder()
            .id(long_reference_batch_1_id.clone())
            .entity_type(long_reference_type.clone())
            .relationship("link", Relationship::builder(stop_id.clone()))
            .build()?,
        Entity::builder()
            .id(long_reference_batch_2_id.clone())
            .entity_type(long_reference_type.clone())
            .list_relationship("servedBy", ListRelationship::builder(vec![stop_time_update_id.clone()]))
            .build()?,
    ];
    let created = broker.entities().batch_upsert(&entities).send().await?.created_entities()?;
    expect_that!(created, len(eq(2)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&long_reference_batch_1_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("link", relationship().object(stop_id.clone())));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&long_reference_batch_2_id)
        .send()
        .await?;
    let entity_2 = received.outcome.body();
    expect_that!(
        entity_2,
        has_attribute("servedBy", any_attribute().list_length(1).list_object(0, stop_time_update_id.clone()))
    );
    // The model reads an `objectList` item written as a bare URI or as an object alike, so the item spelling is read as
    // received.
    expect_that!(&received.body, json_count("$.servedBy.objectList[0]", 1));
    Ok(())
}
