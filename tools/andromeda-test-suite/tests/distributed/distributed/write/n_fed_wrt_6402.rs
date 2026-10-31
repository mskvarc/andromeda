//! A context source info jsonld context moves the context to a link header.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    context_source::ContextSource,
    forwarded_body,
    forwarded_head,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::headers::{ContentType, Link},
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity under inclusive registrations whose `contextSourceInfo` names a `jsonldContext`, with and
/// without a `contentType`, is forwarded with Content-Type application/json, a Link header naming that context and
/// no body @context
/// Clause: 4.3.6.6, 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6402_a_context_source_info_jsonld_context_moves_the_context_to_a_link_header() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let core_context = SuiteConfig::get()?.core_context().as_str().to_owned();
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .context_source_info(("contentType", "application/json"))
        .context_source_info(("jsonldContext", core_context.as_str()))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .context_source_info(("jsonldContext", core_context.as_str()))
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(Method::POST, "/1/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    source
        .answer(Method::POST, "/2/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 31)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let link = Link::try_json_ld_context(IriRefBuf::new(core_context)?)?;
    for path in ["/1/ngsi-ld/v1/entities", "/2/ngsi-ld/v1/entities"] {
        let forwarded = source.received_on(Method::POST, path).await?;
        expect_that!(forwarded, len(eq(1)));
        let forwarded_one = forwarded.first().or_fail()?;
        expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&link)));
        expect_that!(
            forwarded_head::head(forwarded_one).typed::<ContentType>().valid(),
            some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
        );
        let forwarded_entity = forwarded_body::entity(forwarded_one)?;
        expect_that!(forwarded_entity.context.as_ref(), none());
    }
    Ok(())
}
