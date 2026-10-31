//! The type listing merges the source types once and a local listing forwards nothing.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    source_discovery::entity_type_list,
    source_discovery_details::entity_type_details,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Relationship,
        Uri,
        discovery::{EntityTypeDetails, EntityTypeList},
    },
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Entity Types with local true lists only the local type and contacts no source; without
/// local it forwards once and lists the local type and the source type Drone once each; with details true it carries
/// the `EntityType` the source details
/// Clause: 5.7.5.4, 5.7.6.4, 6.3.18, 5.2.24, 5.2.25
/// Tenant: private (an unfiltered /types or /attributes listing is matched by every registration of the tenant, clause
/// 5.12, and pages over every file's entries)
/// Recorded: 5.7.11 lets an implementation answer discovery from local data and the registry alone, or forward to every
/// source supporting the operation and merge the answers; Andromeda forwards and merges
#[gtest]
#[tokio::test]
async fn o_fed_dsc_0001_the_type_listing_merges_the_source_types_once_and_a_local_listing_forwards_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let broker = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
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
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let drone_type = namespace.entity_type("Drone")?;
    let source_list = EntityTypeList::new(
        format!("urn:ngsi-ld:EntityTypeList:{namespace}").parse::<Uri>()?,
        vec![drone_type.clone(), vehicle_type.clone()],
    );
    source.answer(Method::GET, "/a/ngsi-ld/v1/types", entity_type_list(&source_list)).await?;
    let drone_details = EntityTypeDetails::new(
        format!("https://uri.etsi.org/ngsi-ld/default-context/Drone_{namespace}").parse::<Uri>()?,
        drone_type.clone(),
        vec![namespace.attribute("altitude")?],
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/types").query("details", "true").priority(1),
            entity_type_details(&[drone_details]),
        )
        .await?;
    let json = broker.with_accept(Accept::any());
    let local_list = json.entity_types().list().locality(Locality::Local).send().await?;
    expect_that!(local_list.type_list, len(eq(1)));
    expect_that!(local_list.type_list.first(), some(eq(&vehicle_type)));
    let entity_requests = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    expect_that!(source.received_matching(&entity_requests).await?, len(eq(0)));
    let merged_list = json.entity_types().list().send().await?;
    expect_that!(merged_list.type_list, len(eq(2)));
    expect_that!(merged_list.type_list, contains(eq(&vehicle_type)));
    expect_that!(merged_list.type_list, contains(eq(&drone_type)));
    let type_requests = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/types");
    expect_that!(source.received_matching(&type_requests).await?, len(eq(1)));
    let details = json.entity_types().list_details().send().await?;
    let drone_iri: Uri = format!("https://uri.etsi.org/ngsi-ld/default-context/Drone_{namespace}").parse()?;
    let drone_detail = details.iter().find(|detail| detail.id == drone_iri).or_fail()?;
    expect_that!(drone_detail.type_name, eq(&drone_type));
    Ok(())
}
