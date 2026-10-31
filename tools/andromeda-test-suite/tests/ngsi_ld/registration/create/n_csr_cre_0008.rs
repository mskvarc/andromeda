//! A registration without an id is retrievable under its location.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, registration_vocabulary::registration_vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Location},
    },
    model::{Context, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration without an id answers 201 with a Location, and Retrieve Context Source
/// Registration under that Location answers 200 with an id
/// Clause: 5.9.2, 5.10.1
#[gtest]
#[tokio::test]
async fn n_csr_cre_0008_a_registration_without_an_id_is_retrievable_under_its_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(&broker, &registration_vocabulary(&namespace)?).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_id = namespace.urn("Vehicle", "A456")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_type = namespace.entity_type("OffStreetParking")?;
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
                .property_names(["brandName", "speed"])
                .relationship_names(["isParked"])
                .build()?,
            RegistrationInfo::builder()
                .entities([
                    EntityInfo::new(parking_type.clone()).with_id_pattern(".*downtown$".parse::<IdPattern>()?),
                    EntityInfo::new(parking_type.clone()).with_id_pattern(".*47$".parse::<IdPattern>()?),
                ])
                .property_names(["availableSpotNumber", "totalSpotNumber"])
                .relationship_names(["isNextToBuilding"])
                .build()?,
        ])
        .build()?;
    let created = body_context.with_responses().registrations().register(&registration).send().await?;
    expect_that!(
        created
            .head
            .typed::<Location>()
            .valid()
            .map(|location| location.as_iri_ref().as_str().to_owned()),
        some(contains_regex("/csourceRegistrations/.+$"))
    );
    let registration_id = created.outcome.id().or_fail()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.information(), len(eq(2)));
    Ok(())
}
