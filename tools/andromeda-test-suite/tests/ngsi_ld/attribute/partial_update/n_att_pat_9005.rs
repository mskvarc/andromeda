//! An empty object in a fragment is refused and keeps the target.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Partial Attribute Update writing object `[]` to a Relationship answers 400 `BadRequestData`, and the
/// Relationship keeps its target
/// Clause: 5.6.4.4, 4.5.3.2
#[gtest]
#[tokio::test]
async fn n_att_pat_9005_an_empty_object_in_a_fragment_is_refused_and_keeps_the_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let person_id = namespace.urn("Person", 1)?;
    let empty_probe = Entity::builder()
        .id(empty_probe_id.clone())
        .entity_type(empty_probe_type)
        .relationship("owner", person_id.clone())
        .build()?;
    broker.entities().create(&empty_probe).send().await?.done()?;
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{empty_probe_id}/attrs/owner"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n    \"object\": []\n}"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("owner", any_attribute().object(person_id.clone())));
    Ok(())
}
