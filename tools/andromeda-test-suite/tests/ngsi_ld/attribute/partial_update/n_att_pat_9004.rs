//! A partial attribute update empties the value.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Value},
};

/// Pins: Partial Attribute Update writing value [] to a Property holding two strings answers 204, and the Property
/// then holds the empty array
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9004_a_partial_attribute_update_empties_the_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe = Entity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type)
        .property("tags", Value::Array(vec!["a".into(), "b".into()]))
        .build()?;
    broker.entities().create(&empty_probe).send().await?.done()?;
    // A patch of a bare value is rendered as the value alone by the model, so it is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &format!("entities/{empty_probe_id}/attrs/tags"), r#"{"value": []}"#)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("tags", property().value(Value::Array(Vec::new()))));
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    Ok(())
}
