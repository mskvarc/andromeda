//! A client instance id on append is not the history identity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::AppendAttributes},
    model::{AttributeName, Entity, EntityFragment, ErrorType, InstanceId, Lossless, NgsiLdMediaType, RawJson},
    query::temporal_query::TemporalProperty,
};

/// Pins: Append Attributes adding speed with an `instanceId` to an existing Entity answers 204; the Temporal
/// Evolution holds one instance of speed whose `instanceId` is a broker-minted one, and Delete Attribute instance
/// addressed to the client's `instanceId` answers 404 `ResourceNotFound`
/// Clause: 5.2.5, 4.5.7, 5.6.3.4, 5.6.15.4
#[gtest]
#[tokio::test]
async fn n_att_app_9012_a_client_instance_id_on_append_is_not_the_history_identity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_appended_id = namespace.urn("Vehicle", "appended")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let client_instance = format!("urn:ngsi-ld:Instance:{namespace}-client").parse::<InstanceId>()?;
    let vehicle_appended = Entity::builder()
        .id(vehicle_appended_id.clone())
        .entity_type(vehicle_type)
        .property("name", "Bus")
        .build()?;
    broker.entities().create(&vehicle_appended).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses an `instanceId` on a written attribute, so `speed` is written as text.
    let speed: RawJson = format!(r#"{{"type": "Property", "value": 40, "instanceId": "urn:ngsi-ld:Instance:{namespace}-client"}}"#).parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("speed", &speed);
    let request = broker
        .unchecked(&AppendAttributes::builder().id(&vehicle_appended_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_appended_id)
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
                len(eq(1)),
                nth(0, any_attribute().value(40).with_member(Member::InstanceId)),
                nth(0, not(any_attribute().instance_id(&client_instance)))
            )
        )
    );
    let answer_2 = broker
        .with_responses()
        .temporal()
        .delete_instance(&vehicle_appended_id, &"speed".parse::<AttributeName>()?, &client_instance)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
