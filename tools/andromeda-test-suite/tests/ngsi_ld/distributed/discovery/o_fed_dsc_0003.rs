//! Type information adds the source attribute details and entity count.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    source_discovery::entity_type_information,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        AttributeName,
        Entity,
        Relationship,
        Uri,
        discovery::{EntityTypeAttribute, EntityTypeInfo},
    },
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Entity Type Information of the file type, held locally by one Entity with `brandName` and
/// by the source with `entityCount` 5 and attribute `airQualityLevel`, carries both attribute details and `entityCount`
/// 6
/// Clause: 5.7.7.4, 5.2.26, 5.2.28
/// Tenant: private (a type or attribute information request is forwarded to every source of the tenant supporting the
/// operation, clause 5.7.11, so every other file's registration would see it)
/// Recorded: 5.7.11 lets an implementation answer discovery from local data and the registry alone, or forward to every
/// source supporting the operation and merge the answers; Andromeda forwards and merges
#[gtest]
#[tokio::test]
async fn o_fed_dsc_0003_type_information_adds_the_source_attribute_details_and_entity_count() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let mut air_quality_level = EntityTypeAttribute::new(
        format!("https://uri.etsi.org/ngsi-ld/default-context/airQualityLevel_{namespace}").parse::<Uri>()?,
        namespace.attribute("airQualityLevel")?,
    );
    air_quality_level.attribute_types = Some(vec![AttributeKind::Property]);
    let source_information = EntityTypeInfo::new(
        format!("https://uri.etsi.org/ngsi-ld/default-context/Vehicle_{namespace}").parse::<Uri>()?,
        vehicle_type.clone(),
        5,
        vec![air_quality_level],
    );
    source
        .answer_when(
            SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/types/.*Vehicle_{namespace}")),
            entity_type_information(&source_information),
        )
        .await?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&vehicle_type).send().await?;
    let air_quality_name = namespace.attribute("airQualityLevel")?;
    let brand_name: AttributeName = "brandName".parse()?;
    expect_that!(
        information.attribute_details.iter().any(|detail| detail.attribute_name == air_quality_name),
        eq(true)
    );
    expect_that!(information.attribute_details.iter().any(|detail| detail.attribute_name == brand_name), eq(true));
    expect_that!(information.entity_count, eq(6));
    let forwarded = SourceRequest::matching(Method::GET, &format!("/a/ngsi-ld/v1/types/.*Vehicle_{namespace}"));
    expect_that!(source.received_matching(&forwarded).await?, len(eq(1)));
    Ok(())
}
