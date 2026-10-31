//! System times a client writes on a sub attribute are replaced by the write instant.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    problem::answered,
    system_time,
    system_time::SystemMember,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, SystemAttributes},
    },
    model::{DateTime, Entity, Lossless, NgsiLdMediaType, Property, RawJson, Value},
};

/// Pins: `createdAt` and `modifiedAt` written on a sub-attribute are not kept: with sysAttrs the sub-attribute
/// shows the instant of the write that created it, the same as its Attribute, and without sysAttrs it shows none
/// Clause: 4.8, 5.2.5, 5.6.1.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_0023_system_times_a_client_writes_on_a_sub_attribute_are_replaced_by_the_write_instant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", Property::builder(21))
        .build()?;
    // The model refuses output-only members on input, so `temperature` is written as text.
    let temperature: RawJson = r#"{"type": "Property", "value": 21, "accuracy": {"type": "Property", "value": 0.5, "createdAt": "1999-01-01T00:00:00Z", "modifiedAt": "1999-01-01T00:00:00Z"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("temperature", &temperature);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let created = system_time::of_attribute(retrieved.body(), &["temperature"], SystemMember::CreatedAt)?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved_2.body();
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::ModifiedAt)?,
        eq(created)
    );
    expect_that!(
        system_time::of_attribute(entity, &["temperature", "accuracy"], SystemMember::CreatedAt)?,
        not(eq(&"1999-01-01T00:00:00Z".parse::<DateTime>()?))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    expect_that!(
        retrieved_3.body(),
        has_attribute(
            "temperature",
            any_attribute().sub_attribute(
                "accuracy",
                any_attribute()
                    .value(Value::Number("0.5".parse()?))
                    .without(Member::CreatedAt)
                    .without(Member::ModifiedAt)
            )
        )
    );
    Ok(())
}
