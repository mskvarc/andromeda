//! An append with the null scope is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Scope, ScopeChange, Value},
};

/// Pins: Append Entity Attributes with scope "urn:ngsi-ld:null" answers 400 `BadRequestData` and the scope /a/b/c
/// stands
/// Clause: 4.18, 5.5.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_att_app_9007_an_append_with_the_null_scope_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_append_null_id = namespace.urn("Building", "append-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_append_null = Entity::builder()
        .id(building_append_null_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_append_null).send().await?.done()?;
    let fragment = EntityFragment::builder().scope(ScopeChange::Delete).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .append_attributes(&building_append_null_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_append_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    Ok(())
}
