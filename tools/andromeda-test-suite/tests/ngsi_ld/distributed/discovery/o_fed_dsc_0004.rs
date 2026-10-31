//! The attribute listing merges the source attributes once and a local listing forwards nothing.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    outcome::Done,
    source_discovery::attribute_list,
    source_discovery_details::attribute_details,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        Relationship,
        Uri,
        discovery::{AttributeDetails, AttributeList},
    },
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Available Attributes with local true lists only the local `brandName` and `isParked` and contacts no
/// source; without local it forwards once and lists `brandName`, `isParked` and the source speed once each; with
/// details true it carries the Attribute the source details
/// Clause: 5.7.8.4, 5.7.9.4, 6.3.18, 5.2.27, 5.2.28
/// Tenant: private (an unfiltered /types or /attributes listing is matched by every registration of the tenant, clause
/// 5.12, and pages over every file's entries)
/// Recorded: 5.7.11 lets an implementation answer discovery from local data and the registry alone, or forward to every
/// source supporting the operation and merge the answers; Andromeda forwards and merges
#[gtest]
#[tokio::test]
async fn o_fed_dsc_0004_the_attribute_listing_merges_the_source_attributes_once_and_a_local_listing_forwards_nothing() -> Result<()> {
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
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let speed: AttributeName = "speed".parse()?;
    let brand_name: AttributeName = "brandName".parse()?;
    let is_parked: AttributeName = "isParked".parse()?;
    let source_list = AttributeList::new(
        format!("urn:ngsi-ld:AttributeList:{namespace}").parse::<Uri>()?,
        vec![speed.clone(), brand_name.clone()],
    );
    source.answer(Method::GET, "/a/ngsi-ld/v1/attributes", attribute_list(&source_list)).await?;
    let mut altitude_details = AttributeDetails::new(
        format!("https://uri.etsi.org/ngsi-ld/default-context/altitude_{namespace}").parse::<Uri>()?,
        namespace.attribute("altitude")?,
    );
    altitude_details.type_names = Some(vec![namespace.entity_type("Drone")?]);
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/attributes").query("details", "true").priority(1),
            attribute_details(&[altitude_details]),
        )
        .await?;
    let json = broker.with_accept(Accept::any());
    let local_list = json.attributes().list().locality(Locality::Local).send().await?;
    expect_that!(local_list.attribute_list, len(eq(2)));
    expect_that!(local_list.attribute_list, contains(eq(&brand_name)));
    expect_that!(local_list.attribute_list, contains(eq(&is_parked)));
    let entity_requests = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    expect_that!(source.received_matching(&entity_requests).await?, len(eq(0)));
    let merged_list = json.attributes().list().send().await?;
    expect_that!(merged_list.attribute_list, len(eq(3)));
    expect_that!(merged_list.attribute_list, contains(eq(&speed)));
    expect_that!(merged_list.attribute_list, contains(eq(&brand_name)));
    expect_that!(merged_list.attribute_list, contains(eq(&is_parked)));
    let attribute_requests = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/attributes");
    expect_that!(source.received_discoveries_matching(&attribute_requests).await?, len(eq(1)));
    let details = json.attributes().list_details().send().await?;
    let altitude_iri: Uri = format!("https://uri.etsi.org/ngsi-ld/default-context/altitude_{namespace}").parse()?;
    let altitude = details.iter().find(|detail| detail.id == altitude_iri).or_fail()?;
    expect_that!(altitude.attribute_name, eq(&namespace.attribute("altitude")?));
    Ok(())
}
