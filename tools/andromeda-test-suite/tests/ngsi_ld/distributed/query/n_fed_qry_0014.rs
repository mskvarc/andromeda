//! A via header listing another alias does not stop forwarding.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Via, ViaEntry},
        operation::RegisterContextSource,
    },
    model::{Entity, RawJson, Relationship},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with Via naming an alias other than the registration `contextSourceAlias` is forwarded once
/// and answers the Entity the source returns
/// Clause: 6.3.18, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_qry_0014_a_via_header_listing_another_alias_does_not_stop_forwarding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .build()?;
    // The model refuses it (a registration contextSourceAlias), so `contextSourceAlias` is written as text.
    let context_source_alias: RawJson = format!(r#""cs-{namespace}-a""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceAlias", &context_source_alias);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::entities(&source_entities),
        )
        .await?;
    let page = broker
        .with_header(Via::try_from_entries([format!("1.1 cs-{namespace}-b").parse::<ViaEntry>()?])?)?
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
