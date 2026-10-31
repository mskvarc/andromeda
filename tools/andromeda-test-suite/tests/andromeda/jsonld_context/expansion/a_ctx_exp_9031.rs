//! A scoped context refusal names the rule.

use andromeda_test_suite::{broker, core_context::core_entry, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext},
};

/// Pins: The `BadRequestData` refusing a body @context that defines a term with a scoped @context names the scoped
/// @context in its detail
/// Clause: 5.5.7
/// Recorded: 5.5.7 fixes the error type and leaves its detail to the broker; Andromeda names the scoped @context
#[gtest]
#[tokio::test]
async fn a_ctx_exp_9031_a_scoped_context_refusal_names_the_rule() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_worded_id = namespace.urn("Device", "worded")?;
    let device_type = namespace.entity_type("Device")?;
    let device_worded = Entity::builder().id(device_worded_id).entity_type(device_type).property("gear", 3).build()?;
    let answer = body_context.with_responses().entities().create(&device_worded).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("scoped @context")
    );
    Ok(())
}
