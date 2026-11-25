//! A forwarded q names a term the forwarding context lacks as a quoted absolute IRI, and travels as written under the
//! client's own context.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    quoted_name_query::query_by_text,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, Link},
        operation::RegisterContextSource,
    },
    model::{Context, NgsiLdMediaType, RawJson},
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with q=brand=="x" forwarded to a registration whose `jsonldContext` holds no term for brand
/// reaches the Context Source with the name as the quoted absolute IRI, "<brand IRI>"=="x", and Query Entities with
/// q="ex:brand"=="x" forwarded to a registration with no `jsonldContext`, under the @context a Link header names,
/// reaches it as the client wrote it
/// Clause: 4.3.6.6, 4.9, 5.5.7
/// Recorded: clause 4.3.6.6 compacts the forwarded query parameters as JSON-LD section 4.1.5 compacts IRIs, so a name
/// with no term travels as its absolute IRI, which Andromeda writes as a `quotedStr`, since no `AttrName` holds a colon
#[gtest]
#[tokio::test]
async fn n_fed_qry_6611_a_forwarded_q_quotes_a_name_the_forwarding_context_has_no_term_for() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let vocabulary = format!("https://test.andromeda/{namespace}/vocab");
    let veh_type = namespace.entity_type("Veh")?;
    let bus_type = namespace.entity_type("Bus")?;
    contexts
        .serve(
            "jc/client.jsonld",
            &format!(
                r#"{{"@context": {{"brand": "{vocabulary}/brand", "ex": "{vocabulary}/", "Veh_{namespace}": "{vocabulary}/Vehicle", "Bus_{namespace}": "{vocabulary}/Bus"}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/configured.jsonld",
            &format!(r#"{{"@context": {{"Car_{namespace}": "{vocabulary}/Vehicle"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    source.answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&[])).await?;
    source.answer(Method::GET, "/b/ngsi-ld/v1/entities", source_answer::entities(&[])).await?;
    let configured_id = namespace.registration_id(1)?;
    let configured = CSourceRegistration::builder(source.endpoint("a")?)
        .id(configured_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let endpoint = contexts.url("jc/configured.jsonld")?;
    let context_source_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{endpoint}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(configured.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = linked.unchecked(&RegisterContextSource { registration: &configured })?.document(&document);
    expect_that!(&linked.send_unchecked(request).await, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _configured_cleanup = Cleanup::of(&linked).registration(&configured_id);
    let plain_id = namespace.registration_id(2)?;
    let plain = CSourceRegistration::builder(source.endpoint("b")?)
        .id(plain_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(bus_type.clone()).with_id_pattern(namespace.id_pattern("Bus")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    linked.registrations().register(&plain).send().await?;
    let _plain_cleanup = Cleanup::of(&linked).registration(&plain_id);
    linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&veh_type)
        .q(q::attr("brand").eq("x"))
        .send()
        .await?;
    let quoted_iri = format!(r#""{vocabulary}/brand"=="x""#);
    let forwarded = source
        .received_matching(&SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("q", quoted_iri.as_str()))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    // The q model writes a name only as an unquoted AttrName, so the query is sent as written.
    let link = Link::try_json_ld_context(IriRefBuf::new(contexts.url("jc/client.jsonld")?.to_string())?)?;
    let answer = query_by_text(&broker, &bus_type, r#""ex:brand"=="x""#, &[], Some(&link)).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    let travelled = source
        .received_matching(&SourceRequest::new(Method::GET, "/b/ngsi-ld/v1/entities").query("q", r#""ex:brand"=="x""#))
        .await?;
    expect_that!(travelled, len(eq(1)));
    Ok(())
}
