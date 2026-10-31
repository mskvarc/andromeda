//! A client instance id on replace is not the history identity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, ErrorType, InstanceId, NgsiLdMediaType},
    query::temporal_query::TemporalProperty,
};

/// Pins: Replace Entity with a replacement whose speed carries an `instanceId` answers 204; the Temporal Evolution
/// holds two instances of speed, neither carrying the client's `instanceId`, and Delete Attribute instance
/// addressed to the client's `instanceId` answers 404 `ResourceNotFound`
/// Clause: 5.2.5, 4.5.7, 5.6.18.4, 5.6.15.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9009_a_client_instance_id_on_replace_is_not_the_history_identity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_replaced_id = namespace.urn("Vehicle", "replaced")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let client_instance = format!("urn:ngsi-ld:Instance:{namespace}-client").parse::<InstanceId>()?;
    let vehicle_replaced = Entity::builder()
        .id(vehicle_replaced_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle_replaced).send().await?.done()?;
    // The model allows no `instanceId` on an input attribute, and the replacement leaves out the `id` it requires,
    // so the request is sent as written.
    let request = json_request_to(
        &broker,
        Method::PUT,
        &format!("entities/{vehicle_replaced_id}"),
        &format!(
            "{{\n    \"type\": \"Vehicle_{namespace}\",\n    \"speed\": {{ \"type\": \"Property\", \"value\": 55, \"instanceId\": \"urn:ngsi-ld:Instance:{namespace}-client\" }}\n}}"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_replaced_id)
        .attrs(vec!["speed".parse()?])
        .time_property(TemporalProperty::ModifiedAt)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                each(any_attribute().with_member(Member::InstanceId)),
                not(contains(any_attribute().instance_id(&client_instance)))
            )
        )
    );
    let deletion = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_replaced_id, &"speed".parse::<AttributeName>()?, &client_instance)
        .send()
        .await;
    expect_that!(&deletion, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
