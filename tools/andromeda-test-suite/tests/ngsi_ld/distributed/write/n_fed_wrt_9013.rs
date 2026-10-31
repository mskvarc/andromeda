//! Redirection ops advertises purge entity.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities of a type matched by a redirect registration advertising only `redirectionOps` is forwarded
/// to its Context Source once
/// Clause: Table 4.20-2, 5.6.21.4, 5.9.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9013_redirection_ops_advertises_purge_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_redirection_ops_id = namespace.registration_id("redirection-ops")?;
    let purge_group_probe_type = namespace.entity_type("PurgeGroupProbe")?;
    source
        .answer(
            Method::DELETE,
            "/redirection-ops/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration_redirection_ops = CSourceRegistration::builder(source.endpoint("redirection-ops")?)
        .id(registration_redirection_ops_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(purge_group_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([OperationGroup::RedirectionOps])
        .build()?;
    broker.registrations().register(&registration_redirection_ops).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_redirection_ops_cleanup = Cleanup::of(&broker).registration(&registration_redirection_ops_id);
    broker.entities().purge().entity_type(&purge_group_probe_type).send().await?.done()?;
    let forwarded = source.received_on(Method::DELETE, "/redirection-ops/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_redirection_ops_id).send().await?;
    Ok(())
}
