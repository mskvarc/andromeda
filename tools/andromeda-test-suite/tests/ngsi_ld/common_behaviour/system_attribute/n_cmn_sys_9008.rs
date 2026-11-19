//! A sub attribute added to an existing attribute enters at the instant of the write.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker::{self, BrokerClient},
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{MergeEntity, PartialAttributeUpdate, SystemAttributes},
    },
    model::{AttributeMembers, AttributeName, Change, Entity, EntityFragment, EntityId, Lossless, NgsiLdMediaType, Output, Property, RawJson, Value},
};

/// Pins: a sub-attribute that Partial Attribute Update or Merge Entity adds to an existing Property, carrying a
/// client `createdAt` and `modifiedAt`, answers 204 and reads back under sysAttrs with both times equal and later
/// than the Property's `createdAt`; the Property keeps its `createdAt` and its `modifiedAt` becomes the added
/// sub-attribute's `createdAt`, the sub-attribute the write does not name keeps both of its times, and without
/// sysAttrs neither sub-attribute shows a system time
/// Clause: 4.8, 4.5.2.2, 5.5.8, 5.6.4.4, 5.6.17.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9008_a_sub_attribute_added_to_an_existing_attribute_enters_at_the_write_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let temperature: AttributeName = "temperature".parse()?;
    let patched_id = namespace.urn("Building", "patched")?;
    let merged_id = namespace.urn("Building", "merged")?;
    for id in [&patched_id, &merged_id] {
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("temperature", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let patched_before = with_system_times(&broker, &patched_id).await?;
    let fragment = Change::from(AttributeMembers::builder().property("precision", Value::Number("0.1".parse()?)).build()?);
    // The model refuses output-only members on input, so the fragment is written as text.
    let precision: RawJson =
        r#"{"precision": {"type": "Property", "value": 0.1, "createdAt": "1999-01-01T00:00:00Z", "modifiedAt": "1999-01-01T00:00:00Z"}}"#.parse()?;
    let request = broker
        .unchecked(
            &PartialAttributeUpdate::builder()
                .id(&patched_id)
                .name(&temperature)
                .attribute(&fragment)
                .build(),
        )?
        .document(&UncheckedDocument::try_from(precision)?);
    expect_that!(&broker.send_unchecked(request).await, answered(StatusCode::NO_CONTENT));
    let patched_after = with_system_times(&broker, &patched_id).await?;
    entered_at_the_write(&patched_before, &patched_after)?;
    let merged_before = with_system_times(&broker, &merged_id).await?;
    let merge = EntityFragment::builder().build()?;
    // The model refuses output-only members on input, so `temperature` is written as text.
    let merged_temperature: RawJson = r#"{"type": "Property", "value": 20, "precision": {"type": "Property", "value": 0.1, "createdAt": "1999-01-01T00:00:00Z", "modifiedAt": "1999-01-01T00:00:00Z"}}"#.parse()?;
    let document = UncheckedDocument::try_from(merge.render(Lossless::Normalized))?.set_member("temperature", &merged_temperature);
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&merged_id).fragment(&merge).build())?
        .document(&document);
    expect_that!(&broker.send_unchecked(request_2).await, answered(StatusCode::NO_CONTENT));
    let merged_after = with_system_times(&broker, &merged_id).await?;
    entered_at_the_write(&merged_before, &merged_after)?;
    for id in [&patched_id, &merged_id] {
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(id).send().await?;
        expect_that!(
            retrieved.body(),
            has_attribute(
                "temperature",
                any_attribute()
                    .sub_attribute("accuracy", any_attribute().without(Member::CreatedAt).without(Member::ModifiedAt))
                    .sub_attribute("precision", any_attribute().without(Member::CreatedAt).without(Member::ModifiedAt))
            )
        );
    }
    Ok(())
}

/// The Entity `id` as Retrieve Entity answers it with `options=sysAttrs`.
async fn with_system_times(broker: &BrokerClient, id: &EntityId) -> Result<Entity<Output>> {
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    Ok(retrieved.into_body())
}

/// Checks that the write between `before` and `after` added `precision` to `temperature` at the instant of the
/// write, moved the Property's `modifiedAt` to that instant, and left `accuracy` as it was.
fn entered_at_the_write(before: &Entity<Output>, after: &Entity<Output>) -> Result<()> {
    let created = *system_time::of_attribute(before, &["temperature"], SystemMember::CreatedAt)?;
    let added = *system_time::of_attribute(after, &["temperature", "precision"], SystemMember::CreatedAt)?;
    // The client's instants lie before the Property was created, so a sub-attribute that kept either fails the
    // checks below.
    verify_that!(added, gt(created))?;
    verify_that!(
        *system_time::of_attribute(after, &["temperature", "precision"], SystemMember::ModifiedAt)?,
        eq(added)
    )?;
    verify_that!(*system_time::of_attribute(after, &["temperature"], SystemMember::CreatedAt)?, eq(created))?;
    verify_that!(*system_time::of_attribute(after, &["temperature"], SystemMember::ModifiedAt)?, eq(added))?;
    for member in [SystemMember::CreatedAt, SystemMember::ModifiedAt] {
        verify_that!(
            *system_time::of_attribute(after, &["temperature", "accuracy"], member)?,
            eq(*system_time::of_attribute(before, &["temperature", "accuracy"], member)?)
        )?;
    }
    Ok(())
}
