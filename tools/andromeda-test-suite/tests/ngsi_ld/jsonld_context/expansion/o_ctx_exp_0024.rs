//! A context served as any json shaped type or without a content type is read.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_record::cached_under,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A create naming a remote @context served as text/plain, text/plain with a charset, with no Content-Type,
/// as application/json, as application/ld+json or as a +json type expands through it, and each download is recorded
/// as Cached
/// Clause: 5.5.6, 5.13.1
/// Recorded: NGSI-LD sets no media type for a fetched @context; Andromeda reads every JSON-shaped type, text/plain
/// and a response without a Content-Type as JSON
/// Recorded: clause 5.13.1 lets a broker keep a downloaded @context as a Cached entry ("may"); Andromeda records
/// every remote @context a request names
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0024_a_context_served_as_any_json_shaped_type_or_without_a_content_type_is_read() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let document = format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#);
    let served = [
        (10, "jc/text-plain.jsonld", Some("text/plain")),
        (11, "jc/text-plain-charset.jsonld", Some("text/plain; charset=utf-8")),
        (12, "jc/bare.jsonld", None),
        (13, "jc/json.jsonld", Some("application/json")),
        (14, "jc/ld-json.jsonld", Some("application/ld+json")),
        (15, "jc/suffix.jsonld", Some("application/vnd.example+json")),
    ];
    for (suffix, at, media_type) in served {
        contexts.serve(at, &document, media_type).await?;
        let body_context = broker
            .with_context(Context::Single(contexts.reference(at)?))?
            .with_context_delivery(ContextDelivery::LdJsonBody);
        let vehicle_id = namespace.urn("Vehicle", suffix)?;
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
        cached_under(&broker, &contexts.url(at)?).await?;
    }
    Ok(())
}
