//! A registration naming purge entity is forwarded the purge.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities of a type matched by a redirect registration advertising `purgeEntity` itself is forwarded
/// to its Context Source once
/// Clause: 4.20, Table 4.20-1, 5.6.21.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9015_a_registration_naming_purge_entity_is_forwarded_the_purge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_purge_named_id = namespace.registration_id("purge-named")?;
    let purge_named_probe_type = namespace.entity_type("PurgeNamedProbe")?;
    source
        .answer(
            Method::DELETE,
            "/purge-named/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration_purge_named = CSourceRegistration::builder(source.endpoint("purge-named")?)
        .id(registration_purge_named_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(purge_named_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::PurgeEntity])
        .build()?;
    broker.registrations().register(&registration_purge_named).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_purge_named_cleanup = Cleanup::of(&broker).registration(&registration_purge_named_id);
    broker.entities().purge().entity_type(&purge_named_probe_type).send().await?.done()?;
    let forwarded = source.received_on(Method::DELETE, "/purge-named/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_purge_named_id).send().await?;
    Ok(())
}
