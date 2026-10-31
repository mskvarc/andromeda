//! Type and attribute terms expand under the request context.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, EntityTypeName, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by a type term and an attrs term answers 200 with the registration
/// holding their IRIs under a Link naming the context that defines the terms, and without it when no Link is given
/// Clause: 5.10.2.4, 5.5.7
#[gtest]
#[tokio::test]
async fn n_csr_qry_6601_type_and_attribute_terms_expand_under_the_request_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/terms.jsonld")?))?;
    contexts
        .serve(
            "jc/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Veh_{namespace}": "https://test.andromeda/{namespace}/Vehicle",
        "spd": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?)
                    .with_id_pattern(namespace.id_pattern("Vehicle")?),
            ])
            .property_names([format!("https://test.andromeda/{namespace}/speed").as_str()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Veh")?)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Veh")?)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), not(contains(has_id(&registration_id))));
    Ok(())
}
