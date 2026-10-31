//! A scoped context is refused wherever it stands in the set.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity under a Link naming a wrapper of a plain and a scoped context answers 400 `BadRequestData`
/// whichever comes first, and so does a body @context holding the core context, a plain term and the scoped context
/// last
/// Clause: 5.5.7, 6.3.5
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9033_a_scoped_context_is_refused_wherever_it_stands_in_the_set() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let served = contexts.url("")?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/plain-first.jsonld")?))?;
    let device_plain_first_id = namespace.urn("Device", "plain-first")?;
    let device_type = namespace.entity_type("Device")?;
    let linked_2 = broker.with_context(Context::Single(contexts.reference("contexts/scoped-first.jsonld")?))?;
    let device_scoped_first_id = namespace.urn("Device", "scoped-first")?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"axle": "https://example.org/{namespace}/axle"}}"#).parse::<InlineContext>()?),
            contexts.reference("contexts/scoped.jsonld")?,
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_inline_last_id = namespace.urn("Device", "inline-last")?;
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
            "contexts/axle.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "axle": "https://example.org/{namespace}/axle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain-first.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}contexts/axle.jsonld",
        "{served}contexts/scoped.jsonld"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/scoped-first.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}contexts/scoped.jsonld",
        "{served}contexts/axle.jsonld"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device_plain_first = Entity::builder()
        .id(device_plain_first_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    let answer = linked.with_responses().entities().create(&device_plain_first).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let device_scoped_first = Entity::builder()
        .id(device_scoped_first_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    let answer_2 = linked_2.with_responses().entities().create(&device_scoped_first).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let device_inline_last = Entity::builder()
        .id(device_inline_last_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 3)
        .build()?;
    let answer_3 = body_context.with_responses().entities().create(&device_inline_last).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_plain_first_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_scoped_first_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_inline_last_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    Ok(())
}
