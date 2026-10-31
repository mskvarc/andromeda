//! A scoped context one remote reference away is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity under a Link naming a context whose @context names a remote context defining gear with a
/// scoped @context answers 400 `BadRequestData`
/// Clause: 5.5.7, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9034_a_scoped_context_one_remote_reference_away_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let served = contexts.url("")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/suite.jsonld")?))?;
    let device_nested_id = namespace.urn("Device", "nested")?;
    let device_type = namespace.entity_type("Device")?;
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
            "contexts/suite.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}contexts/scoped.jsonld",
        {{
          "axle": "https://example.org/{namespace}/axle"
        }}
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device_nested = Entity::builder()
        .id(device_nested_id.clone())
        .entity_type(device_type)
        .property("axle", 2)
        .build()?;
    let answer = linked.with_responses().entities().create(&device_nested).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_nested_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
