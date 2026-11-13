//! System times a client writes are ignored whatever they hold.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::answered,
    system_time::{self, SystemMember},
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

/// Pins: Create Entity whose entity carries a `createdAt` and a `deletedAt` that are no `DateTime`, and whose attribute
/// and sub-attribute carry a `createdAt` that is no `DateTime` and a `modifiedAt` with a `+01:00` offset, answers 201,
/// and with sysAttrs the entity shows a `createdAt` and no `deletedAt`, and the attribute and the sub-attribute show the
/// instant of the write that created them
/// Clause: 5.2.2, 5.2.5, 4.8, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9066_system_times_a_client_writes_are_ignored_whatever_they_hold() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "stamped")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(namespace.entity_type("Building")?)
        .build()?;
    // The model refuses output-only members on input, so they are written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": 21, "createdAt": "yesterday", "modifiedAt": "1999-01-01T00:00:00+01:00", "accuracy": {"type": "Property", "value": 1, "createdAt": "yesterday", "modifiedAt": "1999-01-01T00:00:00+01:00"}}"#.parse()?;
    let yesterday: RawJson = r#""yesterday""#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized)
        .set_member("temperature", &temperature)
        .set_member("createdAt", &yesterday)
        .set_member("deletedAt", &yesterday);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?;
    expect_that!(system_time::of_attribute(entity, &["temperature"], SystemMember::ModifiedAt)?, eq(created));
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        eq(created)
    );
    expect_that!(system_time::of_entity(entity, SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(system_time::of_entity(entity, SystemMember::DeletedAt), err(anything()));
    Ok(())
}
