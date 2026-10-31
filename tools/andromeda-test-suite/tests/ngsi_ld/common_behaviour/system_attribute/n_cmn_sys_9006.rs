//! A sys attrs read written back adopts no system time.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
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
        operation::{CreateEntity, ReplaceEntity, SystemAttributes},
    },
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Replace Entity with the `sysAttrs` reading of the entity, its entity, Attribute and sub-Attribute system
/// times set to 1900, answers 204, and the entity and its Property keep their `createdAt`, and the reading carries
/// the claimed instant nowhere
/// Clause: 5.2.5, 4.8, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9006_a_sys_attrs_read_written_back_adopts_no_system_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone()).build()?;
    // The model refuses it (member `createdAt` is not allowed here at name.accuracy), so `name` is written as text.
    let name: RawJson = r#"{"type": "Property", "value": "first", "accuracy": {"type": "Property", "value": 1, "createdAt": "1900-01-01T00:00:00Z", "modifiedAt": "1900-01-01T00:00:00Z"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("name", &name);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_created = system_time::of_entity(entity, SystemMember::CreatedAt)?;
    let name_created = system_time::of_attribute(entity, &["name"], SystemMember::CreatedAt)?;
    let replacement = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (member `createdAt` is not allowed here at name), so the system times and `name` are written as text.
    let name_2: RawJson = r#"{"type": "Property", "value": "first", "createdAt": "1900-01-01T00:00:00Z", "modifiedAt": "1900-01-01T00:00:00Z", "accuracy": {"type": "Property", "value": 1, "createdAt": "1900-01-01T00:00:00Z"}}"#.parse()?;
    let document_2 = UncheckedDocument::from_entity(&replacement, Lossless::Normalized)
        .set_text("createdAt", "1900-01-01T00:00:00Z")
        .set_text("modifiedAt", "1900-01-01T00:00:00Z")
        .set_member("name", &name_2);
    let request_2 = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("1900-01-01T00:00:00Z")));
    let entity_2 = received.outcome.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::CreatedAt)?, eq(entity_created));
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::CreatedAt)?, eq(name_created));
    expect_that!(system_time::of_attribute(entity_2, &["name"], SystemMember::ModifiedAt).ok(), some(anything()));
    expect_that!(
        entity_2,
        has_attribute("name", any_attribute().sub_attribute("accuracy", any_attribute().value(1)))
    );
    Ok(())
}
