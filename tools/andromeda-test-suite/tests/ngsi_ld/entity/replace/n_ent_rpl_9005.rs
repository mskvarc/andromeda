//! A replacement with the null scope is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Scope, Value},
};

/// Pins: Replace Entity with scope "urn:ngsi-ld:null" answers 400 `BadRequestData` and the scope /a/b/c stands
/// Clause: 4.18, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9005_a_replacement_with_the_null_scope_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_replace_null_id = namespace.urn("Building", "replace-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_replace_null = Entity::builder()
        .id(building_replace_null_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_replace_null).send().await?.done()?;
    // The model refuses this entity, so the replacement is sent as written.
    let request = request_to(&broker, Method::PUT, &format!("entities/{building_replace_null_id}"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Building_{namespace}\",\n    \"scope\": \"urn:ngsi-ld:null\",\n    \"temperature\": {{\n        \"type\": \"Property\",\n        \"value\": 21.5\n    }}\n}}")));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_replace_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
