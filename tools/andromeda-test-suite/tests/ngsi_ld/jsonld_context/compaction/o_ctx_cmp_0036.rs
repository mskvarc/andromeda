//! A feature collection names the context once and a feature names its own.

use andromeda_test_suite::{broker, config::SuiteConfig, core_context::core_entry, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, NgsiLdMediaType},
};

/// Pins: Query Entities as application/geo+json names the @context once on the `FeatureCollection` and on no Feature,
/// and Retrieve Entity as application/geo+json names it on the Feature: the core alone with no Link, a user @context
/// followed by the core, and a URL ending with the core alone
/// Clause: 4.5.16.2, 4.5.16.3, 5.2.3, Annex C.8
/// Tenant: private (the case reads what the tenant stored under each request @context, and clause 4.14 keeps it the
/// tenant's own)
/// Recorded: clause 5.2.3 has an application/ld+json body name the user @context and the core @context, and clause
/// 4.4 has the core treated last; Andromeda names the request @contexts in order with every core reference removed,
/// and appends the v1.9 core unless what remains leaves it in force last (Annex C.8)
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0036_a_feature_collection_names_the_context_once_and_a_feature_names_its_own() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let tenant_linked_2 = tenant.with_context(Context::Single(contexts.reference("jc/user/wrapper.jsonld")?))?;
    let device_iri = format!("https://test.andromeda/{namespace}/Device");
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
    contexts
        .serve(
            "jc/user/wrapper.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let device = Entity::builder().id(device_id.clone()).entity_type(device_type).property("speed", 3).build()?;
    tenant_linked.entities().create(&device).send().await?.done()?;
    let user_and_core = Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]);
    let wrapper_alone = Context::Single(contexts.reference("jc/user/wrapper.jsonld")?);
    let core_alone = Context::Single(core_entry()?);
    for (client, expected) in [(&tenant, &core_alone), (&tenant_linked, &user_and_core), (&tenant_linked_2, &wrapper_alone)] {
        let page = client
            .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
            .entities()
            .query_geo_json()
            .entity_type(device_iri.as_str())
            .ids(vec![device_id.clone()])
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        let feature = page.items().first().or_fail()?;
        expect_that!(feature.entity.id, eq(&device_id));
        expect_that!(feature.context.as_ref(), none());
        expect_that!(page.context(), some(eq(expected)));
        let retrieved = client
            .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
            .entities()
            .retrieve_geo_json(&device_id)
            .send()
            .await?;
        expect_that!(retrieved.body().context.as_ref(), some(eq(expected)));
    }
    Ok(())
}
