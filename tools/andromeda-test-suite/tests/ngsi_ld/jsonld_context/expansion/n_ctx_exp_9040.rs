//! A query under a scoped linked context is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities under a Link naming a context that defines gear with a scoped @context answers 400
/// `BadRequestData`, and under a plain one 200
/// Clause: 5.5.7, 5.7.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9040_a_query_under_a_scoped_linked_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("contexts/scoped.jsonld")?))?;
    let linked_2 = broker.with_context(Context::Single(contexts.reference("contexts/plain.jsonld")?))?;
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
    let answer = linked
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Device")?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = linked_2
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&namespace.entity_type("Device")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
