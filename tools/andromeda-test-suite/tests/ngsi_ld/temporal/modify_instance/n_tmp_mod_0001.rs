//! The instance is replaced keeping created at and advancing modified at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_where},
    instance_time::instance_time_where,
    namespace::Namespace,
    outcome::Upserted,
    system_time::SystemMember,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Attribute, AttributeName, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance answers 204 with no body and replaces that instance only: it keeps its
/// `instanceId` and `createdAt`, its `modifiedAt` moves later, and the other instance is unchanged
/// Clause: 5.6.14.4, 6.22.3.1, 4.8
#[gtest]
#[tokio::test]
async fn n_tmp_mod_0001_the_instance_is_replaced_keeping_created_at_and_advancing_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;

    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40)
                .dataset_id(namespace.dataset_id("Dataset", "fuel")?)
                .observed_at("2020-09-01T14:07:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let before = retrieved.body();
    let instance_id = instance_id_where(before, &"speed".parse()?, any_attribute().value(120))?;
    let created = instance_time_where(before, &"speed".parse()?, any_attribute().value(120), SystemMember::CreatedAt)?;
    let modified = instance_time_where(before, &"speed".parse()?, any_attribute().value(120), SystemMember::ModifiedAt)?;
    let instance = Attribute::from(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?);
    let received = broker
        .with_responses()
        .temporal()
        .modify_instance(&vehicle_id, &"speed".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await?;
    expect_that!(received.body.len(), eq(0));
    let retrieved_2 = json.temporal().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                contains(any_attribute().value(129).instance_id(&instance_id)),
                contains(any_attribute().value(80).observed_at("2020-09-01T12:05:00Z".parse()?))
            )
        )
    );
    let retrieved_3 = json
        .temporal()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let after = retrieved_3.body();
    let replaced = any_attribute().instance_id(&instance_id);
    expect_that!(after, has_evolution("speed", contains(any_attribute().instance_id(&instance_id).value(129))));
    expect_that!(
        instance_time_where(after, &"speed".parse()?, replaced.clone(), SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(instance_time_where(after, &"speed".parse()?, replaced, SystemMember::ModifiedAt)?, gt(modified));
    Ok(())
}
