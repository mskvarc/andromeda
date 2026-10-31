//! A wrapper url is minted on the request origin.

use andromeda_test_suite::{broker, minted_wrapper::minted_wrapper, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: A json Retrieve Entity under a user @context without the core @context links a wrapper whose absolute URL is
/// built on the scheme and authority the request was addressed to
/// Clause: 6.3.6, 5.13.1
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
/// Recorded: without a configured apiRoot Andromeda mints the URL on the request's own origin (clause 6.2 leaves the
/// apiRoot to the deployment)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0001_a_wrapper_url_is_minted_on_the_request_origin() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    linked.entities().create(&device).send().await?.done()?;
    let headed = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    expect_that!(wrapper.url.as_str(), starts_with(format!("{}jsonldContexts/", broker.endpoint().base_url())));
    Ok(())
}
