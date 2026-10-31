//! Output only members are ignored.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered, received_status::ExpectedStatus};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateEntityMap},
    model::{DateTime, Entity, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Update `EntityMap` whose fragment also carries entityMap and linkedMaps answers 204 and leaves both
/// output-only members as the broker generated them
/// Clause: 5.14.2.4, 5.2.39
#[gtest]
#[tokio::test]
async fn n_map_upd_0002_output_only_members_are_ignored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_fake_id = namespace.urn("Vehicle", "fake")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let plus_3h: DateTime = (Utc::now() + TimeDelta::hours(3)).format("%Y-%m-%dT%H:%M:%SZ").to_string().parse()?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    // The typed update writes the expiry alone, and the test sends the output-only members beside it.
    let entity_map: RawJson = format!(r#"{{"{vehicle_fake_id}": ["@none"]}}"#).parse()?;
    let linked_maps: RawJson = r#"{"fake": "fake"}"#.parse()?;
    let document = UncheckedDocument::empty()
        .set_text("expiresAt", &plus_3h.to_string())
        .set_member("entityMap", &entity_map)
        .set_member("linkedMaps", &linked_maps);
    let request = broker
        .unchecked(&UpdateEntityMap {
            id: map_id,
            expires_at: &plus_3h,
        })?
        .document_as_is(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let map = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(map.entity_map.get(&vehicle_id), some(anything()));
    expect_that!(map.entity_map.get(&vehicle_fake_id), none());
    expect_that!(&map.linked_maps, is_empty());
    Ok(())
}
