//! A fragment carrying the target id and type replaces the instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    instance_time::instance_time_at,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    system_time::SystemMember,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ModifyAttributeInstance, SystemAttributes},
    },
    model::{Attribute, AttributeName, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Modify Attribute instance with a Fragment carrying the target id, its type and speed holding one instance
/// answers 204, and the instance holds 129 with its `instanceId` and `createdAt` and a `modifiedAt`
/// Clause: 5.4, 5.6.14.3, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_tmp_mod_9001_a_fragment_carrying_the_target_id_and_type_replaces_the_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_fragment_replaces_id = namespace.urn("Vehicle", "fragment-replaces")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_fragment_replaces_evolution = TemporalEntity::builder()
        .id(vehicle_fragment_replaces_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("fuel", Property::builder(40).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&vehicle_fragment_replaces_evolution)
        .send()
        .await?
        .created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .temporal()
        .retrieve(&vehicle_fragment_replaces_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let instance_id = instance_id_at(retrieved.body(), &"speed".parse()?, 0)?;
    let created_at = instance_time_at(retrieved.body(), &"speed".parse()?, 0, SystemMember::CreatedAt)?;
    // The operation writes one instance; the fragment names the target entity beside the instance, so the typed
    // request is sent with the temporal entity rendered as its body.
    let fragment = TemporalEntity::builder()
        .id(vehicle_fragment_replaces_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(129).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    let placeholder = Attribute::from(Property::builder(129).build()?);
    let request = broker
        .unchecked(
            &ModifyAttributeInstance::builder()
                .id(&vehicle_fragment_replaces_id)
                .name(&"speed".parse::<AttributeName>()?)
                .instance_id(&instance_id)
                .instance(&placeholder)
                .build(),
        )?
        .body(Bytes::from(fragment.render(Lossless::Normalized).to_string()));
    let answer = broker.send_unchecked(request).await?;
    expect_that!(answer.status(), eq(StatusCode::NO_CONTENT));
    let retrieved_2 = json
        .temporal()
        .retrieve(&vehicle_fragment_replaces_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let evolution = retrieved_2.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            nth(0, any_attribute().value(129).instance_id(&instance_id).with_member(Member::ModifiedAt))
        )
    );
    expect_that!(instance_time_at(evolution, &"speed".parse()?, 0, SystemMember::CreatedAt)?, eq(created_at));
    Ok(())
}
