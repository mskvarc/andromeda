//! A minted instance id is a version 7 uuid urn.

use andromeda_test_suite::{
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: A Temporal Evolution created without `instanceId` reads back its instance with an `instanceId` that is a
/// URN ending in a version-7 UUID of the RFC 9562 variant
/// Clause: 4.5.7, 5.6.11, 5.7.3; IETF RFC 9562 sections 4.1 and 5.7
/// Recorded: clause 4.5.7 leaves the form of a broker-generated `instanceId` to the broker; Andromeda mints
/// version-7 UUIDs, which disclose their approximate mint time
#[gtest]
#[tokio::test]
async fn a_tmp_ret_9035_a_minted_instance_id_is_a_version_7_uuid_urn() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "uuid-privacy-disclosure")?;
    let vehicle = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(1))));
    expect_that!(
        instance_id_at(evolution, &"speed".parse()?, 0)?.to_string(),
        matches_regex("^urn:ngsi-ld:.*:[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    );
    Ok(())
}
