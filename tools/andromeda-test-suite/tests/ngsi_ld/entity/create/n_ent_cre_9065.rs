//! Create Entity ignores a deletedAt written on an instance and on a sub-attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    claimed_deletion::{has_speed_without_deletion, speed_claiming_deletion},
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, SystemAttributes},
    },
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity holding `speed` with a `deletedAt` on its instance and on its `accuracy` sub-attribute, and
/// `heading` with a `deletedAt` that is no `DateTime`, answers 201, and the Entity reads back, with sysAttrs, holding
/// the written values and no `deletedAt` at either depth
/// Clause: 5.2.5, 5.6.1.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9065_create_entity_ignores_a_written_deleted_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "created")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .build()?;
    // The model refuses output-only members on input, so both attributes are written as text.
    let heading: RawJson = r#"{"type": "Property", "value": 90, "deletedAt": "yesterday"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized)
        .set_member("speed", &speed_claiming_deletion(60, 2)?)
        .set_member("heading", &heading);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_speed_without_deletion(60, 2));
    expect_that!(retrieved.body(), has_attribute("heading", any_attribute().value(90).without(Member::DeletedAt)));
    Ok(())
}
