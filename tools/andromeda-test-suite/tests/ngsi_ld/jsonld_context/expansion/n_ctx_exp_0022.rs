//! A context served with max age zero is fetched again.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::{RemoteContexts, ServedAnswer},
};
use googletest::prelude::*;
use http::header::CACHE_CONTROL;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity},
};

/// Pins: Two creates naming a remote @context served with Cache-Control max-age=0 fetch it twice
/// Clause: 6.3.16, 5.13.1, RFC 9111 s5.2.2.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0022_a_context_served_with_max_age_zero_is_fetched_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve_answer(
            "jc/max-age-zero.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://test.andromeda/{namespace}/remote/speed"}}}}"#),
            ServedAnswer::ok()
                .with_media_type("application/ld+json")
                .with_header(CACHE_CONTROL, "max-age=0".parse()?),
        )
        .await?;
    let body_context = broker
        .with_context(Context::Single(contexts.reference("jc/max-age-zero.jsonld")?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_id = namespace.urn("Vehicle", 6)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    body_context.entities().create(&vehicle).send().await?.done()?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/remote/speed"), any_attribute().value(1))
    );
    let vehicle_2_id = namespace.urn("Vehicle", 7)?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    body_context.entities().create(&vehicle_2).send().await?.done()?;
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_2_id).send().await?;
    expect_that!(
        retrieved_2.body(),
        has_attribute(&format!("https://test.andromeda/{namespace}/remote/speed"), any_attribute().value(1))
    );
    expect_that!(contexts.downloads("jc/max-age-zero.jsonld").await?, ge(2));
    Ok(())
}
