//! Geojson with prefer body json links the wrapper and carries no body context.

use andromeda_test_suite::{broker, minted_wrapper::minted_wrapper, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, GeoJsonBody, Prefer},
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities as application/geo+json with Prefer body=json under a user @context without the core links
/// one minted wrapper and carries no @context on the collection or on a feature
/// Clause: 6.3.6, 6.3.15, 5.13.1
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0012_geojson_with_prefer_body_json_links_the_wrapper_and_carries_no_body_context() -> Result<()> {
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
    let device = Entity::builder()
        .id(device_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 3)
        .build()?;
    linked.entities().create(&device).send().await?.done()?;
    let headed = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .with_prefer(Prefer::builder().geo_json_body(GeoJsonBody::Json).build()?)
        .entities()
        .query_geo_json()
        .entity_type(&device_type)
        .ids(vec![device_id.clone()])
        .send()
        .await?;
    minted_wrapper(&headed.head)?;
    let page = &headed.outcome;
    expect_that!(page.context(), none());
    let feature = page.items().first().or_fail()?;
    expect_that!(feature.entity.id, eq(&device_id));
    expect_that!(feature.context.as_ref(), none());
    Ok(())
}
