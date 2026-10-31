//! A body less forward carries no content type.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_head,
    header_matcher::absent,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
    wire::header_values,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::LINK};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, ContentType, Link},
        operation::RegisterContextSource,
    },
    model::{Context, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities forwarded as a GET to a registration whose `contentType` is application/ld+json carries no
/// Content-Type, the forwarded request having no body, and carries one context Link
/// Clause: 6.3.19, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_fed_qry_6607_a_body_less_forward_carries_no_content_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    let registration_id = namespace.registration_id(1)?;
    let veh_type = namespace.entity_type("Veh")?;
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
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["spd"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
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
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .attrs(vec!["spd".parse()?])
        .send()
        .await?;
    let forwarded_request = SourceRequest::new(Method::GET, "/ngsi-ld/v1/entities").query("type", format!("Veh_{namespace}"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<ContentType>(), absent());
    expect_that!(header_values(forwarded_head::head(forwarded_one).headers(), &LINK), len(eq(1)));
    expect_that!(
        forwarded_head::head(forwarded_one).typed::<Link>().valid(),
        some(eq(&Link::try_json_ld_context(IriRefBuf::new(contexts.url("jc/client.jsonld")?.to_string())?)?))
    );
    Ok(())
}
