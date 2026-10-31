//! A linked context declaring a scoped context is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity under a Link naming a context that defines gear with a scoped @context answers 400
/// `BadRequestData` and creates nothing, and under a Link naming a context defining gear plainly answers 201
/// Clause: 5.5.7, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9032_a_linked_context_declaring_a_scoped_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/scoped.jsonld")?))?;
    let device_link_scoped_id = namespace.urn("Device", "link-scoped")?;
    let device_type = namespace.entity_type("Device")?;
    let linked_2 = broker.with_context(Context::Single(contexts.reference("contexts/plain.jsonld")?))?;
    let device_link_plain_id = namespace.urn("Device", "link-plain")?;
    contexts
        .serve(
            "contexts/scoped.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear": {{
          "@id": "https://example.org/{namespace}/gear",
          "@context": {{
            "observedAt": "https://example.org/{namespace}/rewritten"
          }}
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear": "https://example.org/{namespace}/gear"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device_link_scoped = Entity::builder()
        .id(device_link_scoped_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    let answer = linked.with_responses().entities().create(&device_link_scoped).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_link_scoped_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let device_link_plain = Entity::builder()
        .id(device_link_plain_id)
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    linked_2.entities().create(&device_link_plain).send().await?.done()?;
    Ok(())
}
