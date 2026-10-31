//! A registration without mode and operations reads back expanded with their defaults.

use andromeda_test_suite::{broker, cleanup::Cleanup, core_context::core_entry, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, EntityTypeName, InlineContext, OneOrMany},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode, SupportedOperation},
};
use url::Url;

/// Pins: Retrieve Context Source Registration without a Link of a registration created with no mode or operations
/// answers 200 with every user term as its IRI, mode inclusive and operations `federationOps`
/// Clause: 5.10.1, 5.2.9, 4.20, 5.5.7
/// Recorded: Table 5.2.9-1 gives inclusive and `federationOps` as the values of an undefined mode and operations
/// but does not say an undefined member is rendered; Andromeda renders both
#[gtest]
#[tokio::test]
async fn o_csr_ret_0003_a_registration_without_mode_and_operations_reads_back_expanded_with_their_defaults() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_a456_id = namespace.urn("Vehicle", "A456")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_type = namespace.entity_type("OffStreetParking")?;
    let vehicle_type_2 = format!("https://test.andromeda/{namespace}/Vehicle").parse::<EntityTypeName>()?;
    let off_street_parking_type_2 = format!("https://test.andromeda/{namespace}/OffStreetParking").parse::<EntityTypeName>()?;
    let context = Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "brandName": "https://test.andromeda/{namespace}/brandName", "speed": "https://test.andromeda/{namespace}/speed", "isParked": "https://test.andromeda/{namespace}/isParked", "OffStreetParking_{namespace}": "https://test.andromeda/{namespace}/OffStreetParking", "availableSpotNumber": "https://test.andromeda/{namespace}/availableSpotNumber", "totalSpotNumber": "https://test.andromeda/{namespace}/totalSpotNumber", "isNextToBuilding": "https://test.andromeda/{namespace}/isNextToBuilding", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "locatedAt": "https://test.andromeda/{namespace}/locatedAt"}}"#).parse::<InlineContext>()?), core_entry()?]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type).with_id(vehicle_a456_id.clone())])
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
    let registration_2 = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(registration_2.id(), eq(&registration_id));
    expect_that!(registration_2.information(), len(eq(2)));
    expect_that!(registration_2.information().first().map(|info| info.entities().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id),
        some(eq(&vehicle_a456_id))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(vehicle_type_2)))
    );
    expect_that!(registration_2.information().first().map(|info| info.property_names().len()), some(eq(2)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/brandName").as_str()))
    );
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.property_names().get(1))
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/speed").as_str()))
    );
    expect_that!(registration_2.information().first().map(|info| info.relationship_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.relationship_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/isParked").as_str()))
    );
    expect_that!(registration_2.information().get(1).map(|info| info.entities().len()), some(eq(2)));
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.entities().first())
            .and_then(EntityInfo::id_pattern)
            .map(ToString::to_string)
            .as_deref(),
        some(eq(".*downtown$"))
    );
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(off_street_parking_type_2.clone())))
    );
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.entities().get(1))
            .and_then(EntityInfo::id_pattern)
            .map(ToString::to_string)
            .as_deref(),
        some(eq(".*47$"))
    );
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.entities().get(1))
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(off_street_parking_type_2.clone())))
    );
    expect_that!(registration_2.information().get(1).map(|info| info.property_names().len()), some(eq(2)));
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.property_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/availableSpotNumber").as_str()))
    );
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.property_names().get(1))
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/totalSpotNumber").as_str()))
    );
    expect_that!(registration_2.information().get(1).map(|info| info.relationship_names().len()), some(eq(1)));
    expect_that!(
        registration_2
            .information()
            .get(1)
            .and_then(|info| info.relationship_names().first())
            .map(ToString::to_string)
            .as_deref(),
        some(eq(format!("https://test.andromeda/{namespace}/isNextToBuilding").as_str()))
    );
    expect_that!(registration_2.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    expect_that!(registration_2.mode(), some(eq(RegistrationMode::Inclusive)));
    expect_that!(registration_2.operations().map(<[_]>::len), some(eq(1)));
    expect_that!(
        registration_2.operations().and_then(|operations| operations.first()),
        some(eq(&"federationOps".parse::<SupportedOperation>()?))
    );
    Ok(())
}
