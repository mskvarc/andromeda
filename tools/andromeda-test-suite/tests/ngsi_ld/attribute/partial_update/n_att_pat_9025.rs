//! A client instance id on a partial update is not the history identity.

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

/// Pins: Partial Attribute update of speed with an instanceId answers 204; the Temporal Evolution holds two
/// instances of speed, neither carrying the client's instanceId, and Delete Attribute instance addressed to the
/// client's instanceId answers 404 `ResourceNotFound`
/// Clause: 5.2.5, 4.5.7, 5.6.4.4, 5.6.15.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9025_a_client_instance_id_on_a_partial_update_is_not_the_history_identity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_patched_id = namespace.urn("Vehicle", "patched")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let client_instance = format!("urn:ngsi-ld:Instance:{namespace}-client").parse::<InstanceId>()?;
    let vehicle_patched = Entity::builder()
        .id(vehicle_patched_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle_patched).send().await?.done()?;
    // The model refuses an `instanceId` on a written attribute, so the patch is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_patched_id}/attrs/speed"),
        &format!(r#"{{"value": 55, "instanceId": "urn:ngsi-ld:Instance:{namespace}-client"}}"#),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_patched_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["speed".parse()?])
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
                each(not(any_attribute().instance_id(&client_instance)))
            )
        )
    );
    let answer_2 = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_patched_id, &"speed".parse::<AttributeName>()?, &client_instance)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
