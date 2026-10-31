//! A second hop two alternates or a non json ld alternate are not available.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::problem,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::LINK};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, Entity, ErrorType},
};

/// Pins: A create naming an HTML page whose alternate is itself an HTML page, one naming two JSON-LD alternates, or
/// one naming an alternate of another media type answers 504 `LdContextNotAvailable`, and the second hop is never
/// fetched
/// Clause: 5.5.6, 5.5.2, JSON-LD 1.1 s6.2
/// Recorded: NGSI-LD references the JSON-LD 1.1 syntax, not the processing API whose document loader this behaviour
/// comes from; Andromeda loads a remote @context as that document loader does
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0021_a_second_hop_two_alternates_or_a_non_json_ld_alternate_are_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let alternate = |target: &'static str, media_type: &'static str| -> Result<ServedAnswer> {
        Ok(ServedAnswer::ok()
            .with_media_type("text/html")
            .with_header(LINK, HeaderValue::from_str(&format!(r#"<{target}>; rel="alternate"; type="{media_type}""#))?))
    };
    contexts
        .serve(
            "jc/third.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve_answer("jc/second", "<html></html>", alternate("third.jsonld", "application/ld+json")?)
        .await?;
    contexts
        .serve_answer("jc/first", "<html></html>", alternate("second", "application/ld+json")?)
        .await?;
    contexts
        .serve_answer(
            "jc/two",
            "<html></html>",
            alternate("a.jsonld", "application/ld+json")?
                .with_header(LINK, HeaderValue::from_static(r#"<b.jsonld>; rel="alternate"; type="application/ld+json""#)),
        )
        .await?;
    contexts
        .serve_answer("jc/other-type", "<html></html>", alternate("third.jsonld", "text/turtle")?)
        .await?;
    for (suffix, page) in [(3, "jc/first"), (4, "jc/two"), (5, "jc/other-type")] {
        let body_context = broker
            .with_context(Context::Single(contexts.reference(page)?))?
            .with_context_delivery(ContextDelivery::LdJsonBody);
        let vehicle = Entity::builder()
            .id(namespace.urn("Vehicle", suffix)?)
            .entity_type(namespace.entity_type("Vehicle")?)
            .property("speed", 1)
            .build()?;
        let answer = body_context.with_responses().entities().create(&vehicle).send().await;
        expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    }
    expect_that!(contexts.downloads("jc/third.jsonld").await?, eq(0));
    Ok(())
}
