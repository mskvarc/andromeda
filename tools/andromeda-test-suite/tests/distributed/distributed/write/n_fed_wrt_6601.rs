//! A configured jsonld context spells the forwarded body.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    forwarded_head,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
    wire::header_values,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{ContentType, Link},
        operation::RegisterContextSource,
    },
    model::{Context, Entity, NgsiLdMediaType, OneOrMany, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity forwarded to a registration with a `jsonldContext` carries one context Link naming it,
/// Content-Type application/json and a body without @context whose type and attribute are in its terms
/// Clause: 4.3.6.6, 6.3.19
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6601_a_configured_jsonld_context_spells_the_forwarded_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_write2_id = namespace.urn("Vehicle", "write2")?;
    let car_type = namespace.entity_type("Car")?;
    contexts
        .serve(
            "jc/client.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "spd": "https://test.andromeda/{namespace}/vocab/speed",
        "Veh_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/configured.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "velocity": "https://test.andromeda/{namespace}/vocab/speed",
        "Car_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    source
        .answer(Method::POST, "/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id(vehicle_write2_id.clone())])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let endpoint = contexts.url("jc/configured.jsonld")?;
    let context_source_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{endpoint}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = linked.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    let vehicle_write2 = Entity::builder()
        .id(vehicle_write2_id.clone())
        .entity_type(veh_type.clone())
        .property("spd", 30)
        .build()?;
    linked.entities().create(&vehicle_write2).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::entity(forwarded_one)?;
    expect_that!(forwarded_entity.context.as_ref(), none());
    expect_that!(&forwarded_entity.entity_type, eq(&OneOrMany::one(car_type)));
    expect_that!(&forwarded_entity, lacks_attribute("spd"));
    expect_that!(&forwarded_entity, has_attribute("velocity", any_attribute().value(30)));
    let forwarded_one_2 = forwarded.first().or_fail()?;
    expect_that!(header_values(forwarded_head::head(forwarded_one_2).headers(), &LINK), len(eq(1)));
    expect_that!(
        forwarded_head::head(forwarded_one_2).typed::<Link>().valid(),
        some(eq(&Link::try_json_ld_context(IriRefBuf::new(
            contexts.url("jc/configured.jsonld")?.to_string()
        )?)?))
    );
    expect_that!(
        forwarded_head::head(forwarded_one_2).typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::Json)))
    );
    Ok(())
}
