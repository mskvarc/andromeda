//! An ld json batch delete id array is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: an application/ld+json Batch Entity Delete whose body is an array of ids, which can carry no @context, is
/// refused as `BadRequestData` and deletes nothing
/// Clause: 6.3.5, 5.6.10, 6.17.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0009_an_ld_json_batch_delete_id_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building).send().await?.done()?;
    // The body goes as application/ld+json, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!("[\n    \"urn:ngsi-ld:Building:{namespace}-1\"\n]")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
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
