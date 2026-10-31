//! Version 1 8 amends an inline linked entity.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RetrieveEntity},
    },
    model::{Entity, Lossless, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with join=inline and `joinLevel=1` nests a Linked Entity carrying `expiresAt`, and with
/// Prefer ngsi-ld=1.8 answers 203 with the Linked Entity still nested and without `expiresAt`
/// Clause: 4.3.6.8, 4.5.23
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9011_version_1_8_amends_an_inline_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "inline-join")?;
    let vehicle_id = namespace.urn("Vehicle", "inline-join")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .relationship("refBuilding", building_id.clone())
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let joined = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Normalized)
        .join(Some(InlineJoin::Inline))
        .join_level(1.try_into()?)
        .build();
    // The answers carry the linked entity nested and adapted to an older version, so their wire form is held as received.
    let current = send_versioned(&json, &joined, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    expect_that!(&current.body, json_equals("$.refBuilding.entity.expiresAt", "2099-01-01T00:00:00Z"));
    let adapted = send_versioned(&json, &joined, Some("1.8")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.8")));
    expect_that!(&adapted.body, json_equals("$.refBuilding.entity.id", building_id.to_string()));
    expect_that!(&adapted.body, json_absent("$.refBuilding.entity.expiresAt"));
    Ok(())
}
