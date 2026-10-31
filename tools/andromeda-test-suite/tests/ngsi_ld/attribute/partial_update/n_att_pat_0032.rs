//! A partial update keeps the attribute created at and moves its modified at.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update of a Property value keeps the createdAt the Attribute was created with and moves
/// its modifiedAt to the instant of the update
/// Clause: 4.8, 5.6.4.4, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_att_pat_0032_a_partial_update_keeps_the_attribute_created_at_and_moves_its_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", 20)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_attribute(entity, &["temperature"], SystemMember::CreatedAt)?;
    let modified = system_time::of_attribute(entity, &["temperature"], SystemMember::ModifiedAt)?;
    // A patch of a bare value is rendered as the value alone by the model, so it is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &format!("entities/{building_id}/attrs/temperature"), r#"{"value": 21}"#)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("temperature", property().value(21)));
    expect_that!(system_time::of_attribute(entity_2, &["temperature"], SystemMember::CreatedAt)?, eq(created));
    expect_that!(
        system_time::of_attribute(entity_2, &["temperature"], SystemMember::ModifiedAt)?,
        not(eq(modified))
    );
    Ok(())
}
