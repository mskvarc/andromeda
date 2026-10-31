//! A json ld body reads back compacted under its context.

use andromeda_test_suite::{broker, cleanup::Cleanup, core_context::core_entry, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, OneOrMany},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with an application/ld+json body carrying its @context answers 201, and
/// the registration then reads back under that @context with its entity type compacted
/// Clause: 5.9.2, 5.10.1, 6.3.5
#[gtest]
#[tokio::test]
async fn n_csr_cre_0014_a_json_ld_body_reads_back_compacted_under_its_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_a456_id = namespace.urn("Vehicle", "A456")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_type = namespace.entity_type("OffStreetParking")?;
    let context = Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "brandName": "https://test.andromeda/{namespace}/brandName", "speed": "https://test.andromeda/{namespace}/speed", "isParked": "https://test.andromeda/{namespace}/isParked", "OffStreetParking_{namespace}": "https://test.andromeda/{namespace}/OffStreetParking", "availableSpotNumber": "https://test.andromeda/{namespace}/availableSpotNumber", "totalSpotNumber": "https://test.andromeda/{namespace}/totalSpotNumber", "isNextToBuilding": "https://test.andromeda/{namespace}/isNextToBuilding", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "locatedAt": "https://test.andromeda/{namespace}/locatedAt"}}"#).parse::<InlineContext>()?), core_entry()?]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_a456_id)])
                .property_names(["brandName", "speed"])
                .relationship_names(["isParked"])
                .build()?,
            RegistrationInfo::builder()
                .entities([
                    EntityInfo::new(off_street_parking_type.clone()).with_id_pattern(".*downtown$".parse::<IdPattern>()?),
                    EntityInfo::new(off_street_parking_type.clone()).with_id_pattern(".*47$".parse::<IdPattern>()?),
                ])
                .property_names(["availableSpotNumber", "totalSpotNumber"])
                .relationship_names(["isNextToBuilding"])
                .build()?,
        ])
        .build()?;
    body_context.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&body_context).registration(&registration_id);
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let registration_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
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
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    Ok(())
}
