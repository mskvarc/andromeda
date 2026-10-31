//! Attribute information unites the source types and adds its count.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    source_discovery::attribute_information,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, Uri, discovery::AttributeInfo},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Attribute Information of an attribute held locally as a Property of one Entity of the file
/// type, and by the source as a Relationship of 4 Drones, answers 200 with `attributeTypes` Property and Relationship,
/// `typeNames` of both types and `attributeCount` 5
/// Clause: 5.7.10.4, 5.2.28
/// Tenant: private (a type or attribute information request is forwarded to every source of the tenant supporting the
/// operation, clause 5.7.11, so every other file's registration would see it)
/// Recorded: 5.7.11 lets an implementation answer discovery from local data and the registry alone, or forward to every
/// source supporting the operation and merge the answers; Andromeda forwards and merges
#[gtest]
#[tokio::test]
async fn o_fed_dsc_0006_attribute_information_unites_the_source_types_and_adds_its_count() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let drone_type = namespace.entity_type("Drone")?;
    let brand_name = namespace.attribute("brandName")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property(brand_name.clone(), "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .property_names([brand_name.clone()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let mut source_information = AttributeInfo::new(
        format!("https://uri.etsi.org/ngsi-ld/default-context/brandName_{namespace}").parse::<Uri>()?,
        brand_name.clone(),
    );
    source_information.attribute_types = Some(vec![AttributeKind::Relationship]);
    source_information.type_names = Some(vec![drone_type.clone()]);
    source_information.attribute_count = Some(4);
    source
        .answer_when(
            SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/attributes/.*brandName_{namespace}")),
            attribute_information(&source_information),
        )
        .await?;
    let information = broker.with_accept(Accept::any()).attributes().retrieve(&brand_name).send().await?;
    expect_that!(information.attribute_types.as_deref().map(<[AttributeKind]>::len), some(eq(2)));
    expect_that!(information.attribute_types.as_deref(), some(contains(eq(&AttributeKind::Property))));
    expect_that!(information.attribute_types.as_deref(), some(contains(eq(&AttributeKind::Relationship))));
    expect_that!(information.type_names.as_deref(), some(contains(eq(&vehicle_type))));
    expect_that!(information.type_names.as_deref(), some(contains(eq(&drone_type))));
    expect_that!(information.attribute_count, some(eq(5)));
    let forwarded = SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/attributes/.*brandName_{namespace}"));
    expect_that!(source.received_discoveries_matching(&forwarded).await?, len(eq(1)));
    Ok(())
}
