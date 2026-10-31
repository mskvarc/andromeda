//! An absolute alternate target is followed.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
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

/// Pins: A create naming a remote @context served as an HTML page whose JSON-LD alternate Link names an absolute
/// URL expands through that alternate
/// Clause: 5.5.6, JSON-LD 1.1 s6.2
/// Recorded: NGSI-LD references the JSON-LD 1.1 syntax, not the processing API whose document loader this behaviour
/// comes from; Andromeda loads a remote @context as that document loader does
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0020_an_absolute_alternate_target_is_followed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let alternate = contexts
        .serve(
            "jc/elsewhere/context.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            Some("application/json"),
        )
        .await?;
    contexts
        .serve_answer(
            "jc/landing",
            "<html></html>",
            ServedAnswer::ok().with_media_type("text/html").with_header(
                LINK,
                HeaderValue::from_str(&format!(r#"<{alternate}>; rel="alternate"; type="application/ld+json""#))?,
            ),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference("jc/landing")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_id = namespace.urn("Vehicle", 2)?;
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
    Ok(())
}
