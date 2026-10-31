//! A registration names its relationship short.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, core_context::core_entry, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, NgsiLdMediaType, OneOrMany},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration under the @context that declared its terms, one of them with @type
/// @id, answers its entity type, `propertyNames` and `relationshipNames` as those terms
/// Clause: 5.10.1, Table 5.2.9-1, C.3
#[gtest]
#[tokio::test]
async fn n_csr_ret_9001_a_registration_names_its_relationship_short() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![contexts.reference("ctx/declaring.jsonld")?, core_entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let linked = broker.with_context(Context::Single(contexts.reference("ctx/declaring.jsonld")?))?;
    contexts
        .serve(
            "ctx/declaring.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Vehicle_{namespace}": "http://example.org/{namespace}/Vehicle",
        "speed_{namespace}": "http://example.org/{namespace}/speed",
        "isParked_{namespace}": {{
          "@type": "@id",
          "@id": "http://example.org/{namespace}/isParked"
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .relationship_names([namespace.attribute("isParked")?.to_string()])
            .build()?])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    let registration_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.information().first().map(|info| info.relationship_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.relationship_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("isParked_{namespace}").as_str()))
    );
    expect_that!(registration_2.information().first().map(|info| info.property_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("speed_{namespace}").as_str()))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
