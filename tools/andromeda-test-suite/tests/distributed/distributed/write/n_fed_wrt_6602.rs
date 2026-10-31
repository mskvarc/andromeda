//! An ld json source receives the inbound context inline and no context link.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    core_context::core_entry,
    entity_matcher::has_attribute,
    forwarded_body,
    forwarded_head,
    header_matcher::absent,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        ContextDelivery,
        UncheckedDocument,
        headers::{ContentType, Link},
        operation::RegisterContextSource,
    },
    model::{Context, Entity, NgsiLdMediaType, OneOrMany, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity as application/ld+json forwarded to a registration whose `contentType` is
/// application/ld+json carries no context Link, Content-Type application/ld+json and the inbound @context inline
/// with the names in the client's terms
/// Clause: 4.3.6.6, 6.3.5
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6602_an_ld_json_source_receives_the_inbound_context_inline_and_no_context_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
    let vehicle_write3_id = namespace.urn("Vehicle", "write3")?;
    let body_context = broker
        .with_context(Context::Multiple(vec![contexts.reference("jc/client.jsonld")?, core_entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
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
    source
        .answer(Method::POST, "/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id(vehicle_write3_id.clone())])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "contentType", "value": "application/ld+json"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = linked.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = linked.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&linked).registration(&registration_id);
    let vehicle_write3 = Entity::builder()
        .id(vehicle_write3_id.clone())
        .entity_type(veh_type.clone())
        .property("spd", 40)
        .build()?;
    body_context.entities().create(&vehicle_write3).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::entity(forwarded_one)?;
    expect_that!(
        forwarded_entity.context.as_ref().map(Context::entries).and_then(|entries| entries.first()),
        some(eq(&contexts.reference("jc/client.jsonld")?))
    );
    expect_that!(&forwarded_entity.entity_type, eq(&OneOrMany::one(veh_type.clone())));
    expect_that!(&forwarded_entity, has_attribute("spd", any_attribute().value(40)));
    let forwarded_one_2 = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::head(forwarded_one_2).typed::<Link>(), absent());
    expect_that!(
        forwarded_head::head(forwarded_one_2).typed::<ContentType>().valid(),
        some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
    );
    Ok(())
}
