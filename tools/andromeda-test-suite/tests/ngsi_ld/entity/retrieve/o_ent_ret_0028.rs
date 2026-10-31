//! An attribute instance past its expires at is removed and the other instances stand.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instances},
    eventually::retried,
    namespace::Namespace,
    outcome::Done,
};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
};

/// Pins: a dataset instance with an `expiresAt` is read until the broker removes it after that instant; once
/// removed the Entity reads without it, while the default instance of the same Attribute and the other Attributes
/// stand
/// Clause: 4.22, 4.5.5, 5.7.1.4
/// Recorded: 4.22 lets an expired attribute instance "be automatically removed" by a periodic clean-up whose lag
/// the spec leaves open; Andromeda removes it, and the file waits up to 60 seconds for that
#[gtest]
#[tokio::test]
async fn o_ent_ret_0028_an_attribute_instance_past_its_expires_at_is_removed_and_the_other_instances_stand() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let in_10_seconds: DateTime = (Utc::now() + TimeDelta::seconds(10)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("alarm", 1)
        .property(
            "alarm",
            Property::builder(2)
                .dataset_id(namespace.dataset_id("Dataset", "siren")?)
                .expires_at(in_10_seconds),
        )
        .property("height", 30)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("alarm", 2));
    retried(async || {
        let retrieved_2 = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&building_id)
            .send()
            .await?;
        let entity_2 = retrieved_2.body();
        verify_that!(entity_2, has_attribute("alarm", any_attribute().value(1).without(Member::DatasetId)))?;
        verify_that!(entity_2, has_attribute("height", any_attribute().value(30)))?;
        Ok(())
    })
    .await?;
    Ok(())
}
