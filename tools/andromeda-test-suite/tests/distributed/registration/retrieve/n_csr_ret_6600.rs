//! Type and attribute terms read back as iris without a link and as terms under it.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, EntityTypeName, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration of a registration created under a context defining its type and
/// attribute terms answers 200 with their IRIs without a Link and with the terms under a Link naming that context
/// Clause: 5.10.1.4, 5.5.7
#[gtest]
#[tokio::test]
async fn n_csr_ret_6600_type_and_attribute_terms_read_back_as_iris_without_a_link_and_as_terms_under_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/terms.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_type = format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?;
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
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .build()?;
    linked.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type)))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/speed").as_str()))
    );
    let registration_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        registration_3
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(veh_type.clone())))
    );
    expect_that!(
        registration_3
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("spd"))
    );
    Ok(())
}
