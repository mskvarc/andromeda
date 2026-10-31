//! An entity info with an id and an id pattern reads back both.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with an `EntityInfo` carrying both id and `idPattern`, and Retrieve
/// Context Source Registration answers both members as written
/// Clause: 5.2.8
#[gtest]
#[tokio::test]
async fn n_csr_cre_0019_an_entity_info_with_an_id_and_an_id_pattern_reads_back_both() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let t_type = namespace.entity_type("T")?;
    let t_id = namespace.urn("T", 1)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(t_type.clone())
                .with_id(t_id.clone())
                .with_id_pattern(namespace.id_pattern("T")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(registration_2.information().first().map(|info| info.entities().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(t_type.clone())))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        some(eq(&t_id))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id_pattern)
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("urn:ngsi-ld:T:{namespace}-.*").as_str()))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
