//! A posted query under a scoped body context is bad request data.

use andromeda_test_suite::{broker, core_context::core_entry, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, operation::QueryMethod},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Query Entities by POST whose body @context defines gear with a scoped @context answers 400
/// `BadRequestData`, and with a plain one 200
/// Clause: 5.5.7, 5.7.2, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9041_a_posted_query_under_a_scoped_body_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let body_context_2 = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"gear": "https://example.org/{namespace}/gear"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = body_context
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Device")?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = body_context_2
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Device")?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
