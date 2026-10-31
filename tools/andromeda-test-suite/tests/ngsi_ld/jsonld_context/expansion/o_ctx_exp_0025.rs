//! A context served as a non json type without an alternate is not available.

use andromeda_test_suite::{broker, context_record::unrecorded, namespace::Namespace, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, ErrorType},
};

/// Pins: A create naming a remote @context served as application/octet-stream or text/html without an alternate
/// answers 504 `LdContextNotAvailable`, and nothing is stored under its URL
/// Clause: 5.5.6, 5.5.2, 5.13.4.4
/// Recorded: NGSI-LD sets no media type for a fetched @context; Andromeda reads no other media type than the
/// JSON-shaped ones and text/plain as a @context
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0025_a_context_served_as_a_non_json_type_without_an_alternate_is_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let document = format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#);
    for (suffix, at, media_type) in [(16, "jc/binary.jsonld", "application/octet-stream"), (17, "jc/page.html", "text/html")] {
        contexts.serve(at, &document, Some(media_type)).await?;
        let body_context = broker
            .with_context(Context::Single(contexts.reference(at)?))?
            .with_context_delivery(ContextDelivery::LdJsonBody);
        let vehicle = Entity::builder()
            .id(namespace.urn("Vehicle", suffix)?)
            .entity_type(namespace.entity_type("Vehicle")?)
            .property("speed", 1)
            .build()?;
        let answer = body_context.with_responses().entities().create(&vehicle).send().await;
        expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
        unrecorded(&broker, &contexts.url(at)?).await?;
    }
    Ok(())
}
