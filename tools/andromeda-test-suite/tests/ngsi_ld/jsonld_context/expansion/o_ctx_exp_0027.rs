//! A relative context in a redirected document resolves against the final url.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record::cached_under,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::LOCATION};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A create naming a remote @context that redirects to a document naming a relative @context resolves it
/// against the URL the redirect led to, fetches that once, and records the download under the URL the request named
/// Clause: 5.5.6, 5.13.1, JSON-LD 1.1 API s4.1.2
/// Recorded: NGSI-LD references the JSON-LD 1.1 syntax, not the processing API whose document loader this behaviour
/// comes from; Andromeda loads a remote @context as that document loader does
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0027_a_relative_context_in_a_redirected_document_resolves_against_the_final_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "jc/v2/terms.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let wrapper = contexts
        .serve("jc/v2/wrapper.jsonld", r#"{"@context": "terms.jsonld"}"#, Some("application/ld+json"))
        .await?;
    let moved = contexts
        .serve_answer(
            "jc/moved.jsonld",
            "",
            ServedAnswer::ok()
                .with_status(StatusCode::FOUND)
                .with_header(LOCATION, HeaderValue::from_str(wrapper.as_str())?),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference("jc/moved.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_id = namespace.urn("Vehicle", 20)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("speed", 1)
        .build()?;
    body_context.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/remote/speed"), any_attribute().value(1))
    );
    expect_that!(contexts.downloads("jc/v2/terms.jsonld").await?, eq(1));
    cached_under(&broker, &moved).await?;
    Ok(())
}
