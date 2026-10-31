//! An html page naming a json ld alternate is read through it.

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
use http::{HeaderValue, header::LINK};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A create naming a remote @context served as an HTML page with a JSON-LD alternate Link expands through the
/// alternate, fetches the page and the alternate once each, and records the download under the URL the request
/// named
/// Clause: 5.5.6, 5.13.1, JSON-LD 1.1 s6.2
/// Recorded: NGSI-LD references the JSON-LD 1.1 syntax, not the processing API whose document loader this behaviour
/// comes from; Andromeda loads a remote @context as that document loader does
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0019_an_html_page_naming_a_json_ld_alternate_is_read_through_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "jc/alternate.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve_answer(
            "jc/page",
            "<html></html>",
            ServedAnswer::ok().with_media_type("text/html").with_header(
                LINK,
                HeaderValue::from_static(r#"<alternate.jsonld>; rel="alternate"; type="application/ld+json""#),
            ),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference("jc/page")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_id = namespace.urn("Vehicle", 1)?;
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
    cached_under(&broker, &contexts.url("jc/page")?).await?;
    expect_that!(contexts.downloads("jc/page").await?, eq(1));
    expect_that!(contexts.downloads("jc/alternate.jsonld").await?, eq(1));
    Ok(())
}
