//! An inline body context is forwarded against the core and the answer is read in the client terms.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    core_context::core_context_link,
    entity_matcher::has_attribute,
    forwarded_head,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Link},
        operation::QueryMethod,
    },
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: A posted Query with an inline @context object is forwarded as a GET to a `queryEntity` source with fully
/// qualified type and attrs and no context other than the core, and the source's fully qualified answer reaches the
/// client in the client's terms
/// Clause: 4.3.6.6, 6.3.5, 5.7.2.4
/// Recorded: the spec does not say which @context a forwarded request is compacted against when the registration
/// names no `jsonldContext`; Andromeda compacts against the single absolute @context the client request named, else
/// the core @context
#[gtest]
#[tokio::test]
async fn o_fed_qry_6605_an_inline_body_context_is_forwarded_against_the_core_and_the_answer_is_read_in_the_client_terms() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_remote5_id = namespace.urn("Vehicle", "remote5")?;
    let registration_id = namespace.registration_id(1)?;
    let body_context = broker
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"spd": "https://test.andromeda/{namespace}/vocab/speed", "Veh_{namespace}": "https://test.andromeda/{namespace}/vocab/Vehicle"}}"#)
                .parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let veh_type = namespace.entity_type("Veh")?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_remote5_id.clone())
        .entity_type(format!("https://test.andromeda/{namespace}/vocab/Vehicle").parse::<EntityTypeName>()?)
        .property(format!("https://test.andromeda/{namespace}/vocab/speed").as_str(), 7)
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([
                EntityInfo::new(format!("https://test.andromeda/{namespace}/vocab/Vehicle").parse::<EntityTypeName>()?)
                    .with_id_pattern(namespace.id_pattern("Vehicle")?),
            ])
            .property_names([format!("https://test.andromeda/{namespace}/vocab/speed").as_str()])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = body_context
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Veh")?)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    expect_that!(
        page.items(),
        contains(all!(
            has_id(&vehicle_remote5_id),
            has_attribute("spd", any_attribute().value(7)),
            has_type(&veh_type)
        ))
    );
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities")
        .query("type", format!("https://test.andromeda/{namespace}/vocab/Vehicle"))
        .query("attrs", format!("https://test.andromeda/{namespace}/vocab/speed"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<Link>().valid(), some(eq(&core_context_link()?)));
    Ok(())
}
