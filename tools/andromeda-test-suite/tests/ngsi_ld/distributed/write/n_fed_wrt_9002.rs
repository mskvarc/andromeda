//! The identical attribute iri is covered.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Entity, EntityFragment},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Update Attributes of `http://a.example/v#speed` on a local Entity whose type an inclusive `updateEntity`
/// registration covers for that same property answers 204 and forwards the update to the source as a PATCH of the
/// attributes
/// Clause: 5.12, 5.6.2.4
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9002_the_identical_attribute_iri_is_covered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let attribute_iri_coverage_id = namespace.urn("AttributeIriCoverage", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let attribute_iri_coverage_type = namespace.entity_type("AttributeIriCoverage")?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{attribute_iri_coverage_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(attribute_iri_coverage_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .property_names(["http://a.example/v#speed"])
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
    let forwarded = source
        .received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{attribute_iri_coverage_id}/attrs"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
