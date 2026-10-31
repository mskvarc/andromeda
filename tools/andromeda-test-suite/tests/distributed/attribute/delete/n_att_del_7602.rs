//! A change is read back and missing targets are refused.

use andromeda_test_suite::{broker, entity_matcher::lacks_attribute, namespace::Namespace, outcome::Done, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteAttribute},
    model::{AttributeName, Entity, ErrorType},
};

/// Pins: Delete Attribute on broker A of an Entity it holds answers 204 and the change reads back; of colour, which
/// the Entity does not hold, it answers 404 `ResourceNotFound`; of an id that is not a URI it answers 400
/// `BadRequestData`
/// Clause: 5.6.5.4, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_7602_a_change_is_read_back_and_missing_targets_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let answer = broker
        .with_responses()
        .entities()
        .delete_attribute(&vehicle_id, &"colour".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute("speed"));
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(&DeleteAttribute::builder().id(&placeholder_id).name(&"speed".parse::<AttributeName>()?).build())?
        .uri(api_uri(&broker, "entities/not-a-uri/attrs/speed")?);
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
