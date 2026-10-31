//! No other operation group advertises purge entity.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::ErrorType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities of a type matched by a redirect registration advertising only `federationOps`, or only
/// `associationOps`, answers 409 Conflict and forwards nothing to its Context Source
/// Clause: Table 4.20-2, 5.6.21.4, 5.9.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9014_no_other_operation_group_advertises_purge_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_federation_ops_id = namespace.registration_id("federation-ops")?;
    let purge_federation_probe_type = namespace.entity_type("PurgeFederationProbe")?;
    let registration_association_ops_id = namespace.registration_id("association-ops")?;
    let purge_association_probe_type = namespace.entity_type("PurgeAssociationProbe")?;
    source
        .answer(
            Method::DELETE,
            "/federation-ops/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration_federation_ops = CSourceRegistration::builder(source.endpoint("federation-ops")?)
        .id(registration_federation_ops_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(purge_federation_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration_federation_ops).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_federation_ops_cleanup = Cleanup::of(&broker).registration(&registration_federation_ops_id);
    source
        .answer(
            Method::DELETE,
            "/association-ops/ngsi-ld/v1/entities",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration_association_ops = CSourceRegistration::builder(source.endpoint("association-ops")?)
        .id(registration_association_ops_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(purge_association_probe_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([OperationGroup::AssociationOps])
        .build()?;
    broker.registrations().register(&registration_association_ops).send().await?;
    let _registration_association_ops_cleanup = Cleanup::of(&broker).registration(&registration_association_ops_id);
    let answer = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&purge_federation_probe_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let answer_2 = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&purge_association_probe_type)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::CONFLICT, ErrorType::Conflict));
    let forwarded = source.received_on(Method::DELETE, "/federation-ops/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(0)));
    let forwarded_2 = source.received_on(Method::DELETE, "/association-ops/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(0)));
    broker.registrations().delete(&registration_federation_ops_id).send().await?;
    broker.registrations().delete(&registration_association_ops_id).send().await?;
    Ok(())
}
