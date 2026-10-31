//! An operation outside the named operations is bad request data.

use andromeda_test_suite::{broker, core_context::core_entry, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Context, ContextEntry, ErrorType, InlineContext, RawJson},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with operations holding `partialAttrUpdate`, which is no operation or
/// group named in clause 4.20, answers 400 `BadRequestData` and creates nothing
/// Clause: 5.9.2, 5.2.9, 4.20
#[gtest]
#[tokio::test]
async fn n_csr_cre_0009_an_operation_outside_the_named_operations_is_bad_request_data() -> Result<()> {
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
                .entities([EntityInfo::new(vehicle_type).with_id(vehicle_a456_id)])
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
    // The model refuses it (`partialAttrUpdate` is not a valid value of `operations` at operations), so `operations` is written as text.
    let operations: RawJson = r#"["partialAttrUpdate"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("operations", &operations);
    let request = body_context
        .unchecked(&RegisterContextSource { registration: &registration })?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
