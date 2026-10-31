//! A chain at the depth limit is admitted and named alone.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: A create under a Link naming the first of eight documents, each naming the next and the last ending with
/// the core, answers 201; an ld+json retrieval under it names it alone and compacts the type, and the entity holds
/// the type IRI
/// Clause: 5.5.6, 6.3.5, Annex C.8
/// Tenant: private (the case asserts what the tenant holds afterwards, and clause 4.14 keeps it the tenant's own)
#[gtest]
#[tokio::test]
async fn a_ctx_exp_0040_a_chain_at_the_depth_limit_is_admitted_and_named_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let core = config.core_context();
    let served = contexts.url("")?;
    let tenant_linked = tenant.with_context(Context::Single(contexts.reference("jc/at-limit/1.jsonld")?))?;
    let device_id = namespace.urn("Device", 1)?;
    let device_type = namespace.entity_type("Device")?;
    let device_type_2 = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/at-limit/8.jsonld",
            &format!(
                r#"{{
      "@context": [
        {{
          "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
          "speed": "https://test.andromeda/{namespace}/speed"
        }},
        "{core}"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/7.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/8.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/6.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/7.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/5.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/6.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/4.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/5.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/3.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/4.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/2.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/3.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/at-limit/1.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/at-limit/2.jsonld"
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
    tenant_linked.entities().create(&device).send().await?.done()?;
    let retrieved = tenant_linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.context.as_ref(), some(eq(&Context::Single(contexts.reference("jc/at-limit/1.jsonld")?))));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(device_type.clone())));
    let retrieved_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(device_type_2)));
    Ok(())
}
