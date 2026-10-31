//! A via header naming the broker itself stops all forwarding.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_head::head,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Via},
        operation::RegisterContextSource,
    },
    model::{Entity, RawJson, Relationship},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: A Query Entities forwarded once records the Via value the broker sends; the same query sent with that Via
/// answers 200 empty and the source is not contacted again
/// Clause: 6.3.18, 4.3.6.4
/// Recorded: 6.3.18 makes the Via listing exclude the registrations of the sources it names; Andromeda reads its own
/// entry as a loop and matches no registration at all
#[gtest]
#[tokio::test]
async fn o_fed_qry_0020_a_via_header_naming_the_broker_itself_stops_all_forwarding() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .build()?;
    // The model refuses it (a registration contextSourceAlias), so `contextSourceAlias` is written as text.
    let context_source_alias: RawJson = format!(r#""cs-{namespace}-a""#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceAlias", &context_source_alias);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    broker.send_unchecked(request).await?;
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
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    expect_that!(page.items(), nth(0, has_attribute("brandName", any_attribute().value("Mercedes"))));
    let forwarded_request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let via = head(forwarded.first().or_fail()?).typed::<Via>().valid().cloned().or_fail()?;
    let looped = broker
        .with_header(via)?
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(looped.items(), is_empty());
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    Ok(())
}
