//! An exclusive registration reads back with its mode and operations.

use andromeda_test_suite::{broker, cleanup::Cleanup, core_context::core_entry, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, OneOrMany},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode, SupportedOperation},
};
use url::Url;

/// Pins: Create Context Source Registration with mode exclusive on one entity id and operations `queryEntity`,
/// `queryTemporal` and `updateOps` answers 201, and the registration then reads back with that mode and those
/// operations in order
/// Clause: 5.9.2, 5.10.1, 5.2.9, 4.3.6.2, 4.20
#[gtest]
#[tokio::test]
async fn n_csr_cre_0005_an_exclusive_registration_reads_back_with_its_mode_and_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "brandName": "https://test.andromeda/{namespace}/brandName", "speed": "https://test.andromeda/{namespace}/speed", "isParked": "https://test.andromeda/{namespace}/isParked", "OffStreetParking_{namespace}": "https://test.andromeda/{namespace}/OffStreetParking", "availableSpotNumber": "https://test.andromeda/{namespace}/availableSpotNumber", "totalSpotNumber": "https://test.andromeda/{namespace}/totalSpotNumber", "isNextToBuilding": "https://test.andromeda/{namespace}/isNextToBuilding", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "locatedAt": "https://test.andromeda/{namespace}/locatedAt"}}"#).parse::<InlineContext>()?), core_entry()?]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["brandName", "speed"])
            .relationship_names(["isParked"])
            .build()?])
        .mode(RegistrationMode::Exclusive)
        .operations([
            SupportedOperation::from(DistributedOperation::QueryEntity),
            SupportedOperation::from(DistributedOperation::QueryTemporal),
            SupportedOperation::from(OperationGroup::UpdateOps),
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
    expect_that!(registration_2.id(), eq(&registration_id));
    expect_that!(registration_2.information(), len(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        some(eq(&vehicle_id))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type.clone())))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("brandName"))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().get(1))
            .map(ToString::to_string)
            .as_deref(),
        some(eq("speed"))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.relationship_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq("isParked"))
    );
    expect_that!(registration_2.mode(), some(eq(RegistrationMode::Exclusive)));
    expect_that!(registration_2.operations().map(<[_]>::len), some(eq(3)));
    expect_that!(
        registration_2.operations().and_then(|operations| operations.first()),
        some(eq(&"queryEntity".parse::<SupportedOperation>()?))
    );
    expect_that!(
        registration_2.operations().and_then(|operations| operations.get(1)),
        some(eq(&"queryTemporal".parse::<SupportedOperation>()?))
    );
    expect_that!(
        registration_2.operations().and_then(|operations| operations.get(2)),
        some(eq(&"updateOps".parse::<SupportedOperation>()?))
    );
    expect_that!(registration_2.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    Ok(())
}
