//! A create with the null scope is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Create Entity with scope "urn:ngsi-ld:null" answers 400 `BadRequestData` and creates nothing
/// Clause: 4.18, 5.5.4, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9027_a_create_with_the_null_scope_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_create_null_id = namespace.urn("Building", "create-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_create_null = Entity::builder()
        .id(building_create_null_id.clone())
        .entity_type(building_type)
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    // The model refuses it (invalid scope), so `scope` is written as text.
    let scope: RawJson = r#""urn:ngsi-ld:null""#.parse()?;
    let document = UncheckedDocument::from_entity(&building_create_null, Lossless::Normalized).set_member("scope", &scope);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building_create_null).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_create_null_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
