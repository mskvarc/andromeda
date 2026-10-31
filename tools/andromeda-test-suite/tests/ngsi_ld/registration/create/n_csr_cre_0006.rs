//! A registration with a location reads back with it.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    core_context::core_entry,
    geometry_matcher::geometry,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, OneOrMany, RawJson, Value},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};
use url::Url;

/// Pins: Create Context Source Registration with a Point location answers 201, and the registration then reads back
/// with that location
/// Clause: 5.9.2, 5.10.1, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_0006_a_registration_with_a_location_reads_back_with_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let context = Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "brandName": "https://test.andromeda/{namespace}/brandName", "speed": "https://test.andromeda/{namespace}/speed", "isParked": "https://test.andromeda/{namespace}/isParked", "OffStreetParking_{namespace}": "https://test.andromeda/{namespace}/OffStreetParking", "availableSpotNumber": "https://test.andromeda/{namespace}/availableSpotNumber", "totalSpotNumber": "https://test.andromeda/{namespace}/totalSpotNumber", "isNextToBuilding": "https://test.andromeda/{namespace}/isNextToBuilding", "Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "locatedAt": "https://test.andromeda/{namespace}/locatedAt"}}"#).parse::<InlineContext>()?), core_entry()?]);
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration = CSourceRegistration::builder("http://my.csource.org:1026/")
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Point", "coordinates": [-8.521, 41.2]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
    let request = body_context
        .unchecked(&RegisterContextSource { registration: &registration })?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
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
        registration_2.location(),
        some(
            geometry()
                .of_type("Point")
                .length([], 2)
                .ordinate([0], Value::Number("-8.521".parse()?))
                .ordinate([1], Value::Number("41.2".parse()?))
        )
    );
    expect_that!(registration_2.id(), eq(&registration_id));
    expect_that!(
        registration_2
            .information()
            .first()
            .and_then(|info| info.entities().first())
            .map(EntityInfo::entity_type),
        some(eq(&OneOrMany::one(building_type.clone())))
    );
    expect_that!(registration_2.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    Ok(())
}
