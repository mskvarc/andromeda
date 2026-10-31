//! A client instance id is not rendered in the current state.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
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

/// Pins: Create Entity with a Property carrying `instanceId` answers 201, and Retrieve Entity renders no
/// `instanceId`, with and without `sysAttrs`
/// Clause: 5.2.5, 4.5.7
/// Recorded: Table 5.2.5 gives `instanceId` "Only used in temporal representation of Properties", so the
/// current-state representation carries none
#[gtest]
#[tokio::test]
async fn n_ent_cre_4001_a_client_instance_id_is_not_rendered_in_the_current_state() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(vehicle_type).build()?;
    // The model refuses it (member `instanceId` is not allowed here at name), so `name` is written as text.
    let name: RawJson = format!(r#"{{"type": "Property", "value": "Bus", "instanceId": "urn:ngsi-ld:attribute:instance:{namespace}-1"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("name", &name);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Bus").without(Member::InstanceId)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("name", any_attribute().without(Member::InstanceId)));
    Ok(())
}
