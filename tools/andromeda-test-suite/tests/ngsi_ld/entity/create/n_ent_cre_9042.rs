//! A client instance id on create is not the history identity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    problem::{answered, problem},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{AttributeName, Entity, ErrorType, InstanceId, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Create Entity with a Property carrying `instanceId` answers 201; the Temporal Evolution holds one
/// instance of name whose `instanceId` is a broker-minted one, and Delete Attribute instance addressed to the
/// client's `instanceId` answers 404 `ResourceNotFound`
/// Clause: 5.2.5, 4.5.7, 5.6.15.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9042_a_client_instance_id_on_create_is_not_the_history_identity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_created_id = namespace.urn("Vehicle", "created")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let client_instance_id = format!("urn:ngsi-ld:Instance:{namespace}-client").parse::<InstanceId>()?;
    let vehicle = Entity::builder().id(vehicle_created_id.clone()).entity_type(vehicle_type).build()?;
    let body = format!(
        r#"{{"id": "urn:ngsi-ld:Vehicle:{namespace}-created", "type": "Vehicle_{namespace}", "name": {{ "type": "Property", "value": "Bus", "instanceId": "urn:ngsi-ld:Instance:{namespace}-client" }}}}"#
    );
    // The model refuses `instanceId` on input, so the body is sent as written.
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.body(Bytes::from(body));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_created_id)
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("name", all!(len(eq(1)), contains(any_attribute().value("Bus")))));
    let minted = instance_id_at(evolution, &"name".parse()?, 0)?;
    expect_that!(minted, not(eq(&client_instance_id)));
    let answer_2 = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_created_id, &"name".parse::<AttributeName>()?, &client_instance_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
