//! An empty type array is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use bytes::Bytes;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Building:{{ns}}-empty-type",
    "type": [],
    "temperature": {
        "type": "Property",
        "value": 1
    }
}"#;

/// Pins: Create Entity with type [] answers 400 `BadRequestData`, and the Entity does not exist
/// Clause: 4.5.1, 5.2.4, 5.6.1, 5.5.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9036_an_empty_type_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_empty_type_id = namespace.urn("Building", "empty-type")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_empty_type_id.clone()).entity_type(building_type).build()?;
    // The model cannot hold an empty list of types, so the body is sent as written.
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&building).build())?
        .body(Bytes::from(BODY.replace("{{ns}}", namespace.token())));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_empty_type_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
