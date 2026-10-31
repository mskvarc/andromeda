//! An attribute iri sharing only its last segment is not covered.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_request::SourceRequest};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes of `http://a.example/v#speed` on a local Entity whose type an inclusive `updateEntity`
/// registration covers for the property `http://b.example/ns/speed` answers 204 and forwards nothing
/// Clause: 5.12, 5.6.2.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9001_an_attribute_iri_sharing_only_its_last_segment_is_not_covered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let attribute_iri_coverage_type = namespace.entity_type("AttributeIriCoverage")?;
    let attribute_iri_coverage_id = namespace.urn("AttributeIriCoverage", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(attribute_iri_coverage_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names(["http://b.example/ns/speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::UpdateEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let attribute_iri_coverage = Entity::builder()
        .id(attribute_iri_coverage_id.clone())
        .entity_type(attribute_iri_coverage_type.clone())
        .property("http://a.example/v#speed", 10)
        .build()?;
    broker.entities().create(&attribute_iri_coverage).send().await?.done()?;
    let fragment = EntityFragment::builder().property("http://a.example/v#speed", 20).build()?;
    broker
        .entities()
        .update_attributes(&attribute_iri_coverage_id, &fragment)
        .send()
        .await?
        .done()?;
    let forwarded_request = SourceRequest::any_matching("/a/.*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
