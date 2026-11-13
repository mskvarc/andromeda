//! A via hop whose comment holds octets beyond ASCII still names the registration alias.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::VIA};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RegisterContextSource,
    },
    model::{Entity, RawJson, Relationship},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with Via `1.1 <alias> (Zürich proxy)`, naming the `contextSourceAlias` of the only matching
/// registration in a hop whose comment holds obs-text octets, answers 200 empty with NGSILD-Warning code 199 and never
/// contacts the source
/// Clause: 6.3.18, 4.3.6.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_qry_9024_a_via_hop_whose_comment_holds_octets_beyond_ascii_still_names_the_registration_alias() -> Result<()> {
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
    // The model holds a comment of visible ASCII only, so the `Via` field is written as octets.
    let via = HeaderValue::from_bytes(format!("1.1 cs-{namespace}-a (Z\u{fc}rich proxy)").as_bytes())?;
    let page = broker
        .with_header_field(VIA, via)?
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.warnings().valid().and_then(|warnings| warnings.iter().next()).map(NgsiLdWarning::code),
        some(eq(WarningCode::MiscellaneousWarning))
    );
    expect_that!(page.items(), is_empty());
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
