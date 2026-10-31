//! A sub attribute keeps no client system time.

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
        operation::{CreateEntity, MergeEntity, SystemAttributes},
    },
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity and then Merge Entity with a sub-Attribute claiming `createdAt` and `modifiedAt` answer 201
/// and 204, and the `sysAttrs` reading carries the sub-Attribute and a broker `createdAt` on the Property but
/// neither claimed instant anywhere
/// Clause: 5.2.5, 4.8, 6.3.11, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_cmn_sys_9005_a_sub_attribute_keeps_no_client_system_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (member `createdAt` is not allowed here at name.accuracy), so `name` is written as text.
    let name: RawJson = r#"{"type": "Property", "value": "first", "accuracy": {"type": "Property", "value": 1, "createdAt": "1900-01-01T00:00:00Z", "modifiedAt": "1900-01-01T00:00:00Z"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("name", &name);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("1900-01-01T00:00:00Z")));
    let retrieved = &received.outcome;
    let entity = retrieved.body();
    expect_that!(system_time::of_attribute(entity, &["name"], SystemMember::CreatedAt).ok(), some(anything()));
    expect_that!(
        entity,
        has_attribute("name", any_attribute().sub_attribute("accuracy", any_attribute().value(1)))
    );
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (member `createdAt` is not allowed here at name.accuracy), so `name` is written as text.
    let name_2: RawJson = r#"{"type": "Property", "value": "second", "accuracy": {"type": "Property", "value": 1, "createdAt": "1900-01-01T00:00:00Z", "modifiedAt": "1900-01-01T00:00:00Z"}}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("name", &name_2);
    let request_2 = broker
        .unchecked(&MergeEntity::builder().id(&vehicle_id).fragment(&fragment).build())?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received_2.body), not(contains_substring("1900-01-01T00:00:00Z")));
    let retrieved_2 = &received_2.outcome;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute("name", any_attribute().value("second").sub_attribute("accuracy", any_attribute().value(1)))
    );
    Ok(())
}
