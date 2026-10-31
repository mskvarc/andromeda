//! A discovered registration carries the default mode and operations.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    core_context::core_entry,
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode, SupportedOperation},
};

/// Pins: Query Context Source Registrations by type answers 200 with a registration created with no mode or
/// operations holding mode inclusive and operations `federationOps`
/// Clause: 5.10.2, 5.2.9, 4.20
/// Recorded: Table 5.2.9-1 gives inclusive and `federationOps` as the values of an undefined mode and operations
/// but does not say an undefined member is rendered; Andromeda renders both
#[gtest]
#[tokio::test]
async fn o_csr_qry_0006_a_discovered_registration_carries_the_default_mode_and_operations() -> Result<()> {
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
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let listed = page.items().first().or_fail()?;
    expect_that!(listed.mode(), some(eq(RegistrationMode::Inclusive)));
    expect_that!(listed.operations().map(<[_]>::len), some(eq(1)));
    expect_that!(
        listed.operations().and_then(|operations| operations.first()),
        some(eq(&"federationOps".parse::<SupportedOperation>()?))
    );
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    Ok(())
}
