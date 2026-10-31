//! A body without a type is bad request data and creates nothing.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless},
    query::Locality,
};

/// Pins: Create Entity with a body holding an id and no type answers 400 `BadRequestData` and nothing is created
/// Clause: 5.6.1.4, 5.2.4, 6.4.3.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_7200_a_body_without_a_type_is_bad_request_data_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let placeholder_type = namespace.entity_type("Placeholder")?;
    let vehicle = Entity::builder().id(vehicle_id.clone()).entity_type(placeholder_type).build()?;
    // The model requires `type`, which the request leaves out, so it is written for a placeholder and removed.
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).remove_member("type");
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
