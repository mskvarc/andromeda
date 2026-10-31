//! A batch delete whose body is not valid json is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: an application/json Batch Entity Delete whose payload is not a valid JSON document is refused as
/// `InvalidRequest`, with a title, and deletes nothing
/// Clause: 5.5.4, 5.6.10, 6.17.3.1
#[gtest]
#[tokio::test]
async fn n_cmn_val_0002_a_batch_delete_whose_body_is_not_valid_json_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The body is not JSON, which the model cannot write, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!("[\"urn:ngsi-ld:Building:{namespace}-1\",,]")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    Ok(())
}
