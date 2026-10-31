//! A deletion is read back and missing targets are refused.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::DeleteEntity},
    model::{Entity, ErrorType},
};

/// Pins: Delete Entity on broker A of an Entity it holds answers 204 and the Entity is gone; of an unknown Entity
/// it answers 404 `ResourceNotFound`; of an id that is not a URI it answers 400 `BadRequestData`
/// Clause: 5.6.6.4, 6.5.3.2
#[gtest]
#[tokio::test]
async fn n_ent_del_7601_a_deletion_is_read_back_and_missing_targets_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let placeholder_id = namespace.urn("Placeholder", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    let answer = broker.with_responses().with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker.with_responses().entities().delete(&vehicle_2_id).send().await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(&DeleteEntity::builder().id(&placeholder_id).build())?
        .uri(api_uri(&broker, "entities/not-a-uri")?);
    let answer_3 = broker.send_unchecked(request).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
