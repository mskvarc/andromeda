//! A forwarded q names a term the forwarding context abbreviates to a compact IRI unquoted, and quotes a compact IRI
//! holding a dot.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    remote_context::RemoteContexts,
    source_answer,
    source_request::SourceRequest,
    written_parameters::send_written,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        QueryParameter,
        UncheckedDocument,
        operation::{QueryEntities, RegisterContextSource},
    },
    model::{Context, Lossless, RawJson},
    query::builders::SelectorTypeBuilder,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with q=brand=="x";serial=="y" under an @context mapping brand to <vocabulary>/brand and serial
/// to <vocabulary>/serial.no, forwarded to a registration whose `jsonldContext` holds only the prefix ex for
/// <vocabulary>/, reaches the Context Source with q=ex:brand=="x";"ex:serial.no"=="y"
/// Clause: 4.3.6.6, 4.9, 5.5.7
/// Recorded: clause 4.3.6.6 compacts the forwarded query parameters as JSON-LD section 4.1.5 compacts IRIs, so each
/// name travels as the compact IRI the prefix gives; Andromeda writes a compact IRI unquoted where it reads back so
/// and as a `quotedStr` when it holds a dot
#[gtest]
#[tokio::test]
async fn n_fed_qry_6612_a_forwarded_q_writes_a_compact_iri_unquoted_unless_it_holds_a_dot() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let contexts = RemoteContexts::start().await?;
    let vocabulary = format!("https://test.andromeda/{namespace}/vocab");
    let veh_type = namespace.entity_type("Veh")?;
    contexts
        .serve(
            "jc/client.jsonld",
            &format!(r#"{{"@context": {{"brand": "{vocabulary}/brand", "serial": "{vocabulary}/serial.no", "Veh_{namespace}": "{vocabulary}/Vehicle"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/prefixed.jsonld",
            &format!(r#"{{"@context": {{"ex": "{vocabulary}/"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/client.jsonld")?))?;
    source.answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&[])).await?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(veh_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let endpoint = contexts.url("jc/prefixed.jsonld")?;
    let context_source_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{endpoint}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = linked.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    expect_that!(&linked.send_unchecked(request).await, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _cleanup = Cleanup::of(&linked).registration(&registration_id);
    let vehicles = SelectorTypeBuilder::from(&veh_type).build()?;
    let query = QueryEntities::builder().entity_type(&vehicles).format(Lossless::Normalized).build();
    send_written(&linked, &query, &[(QueryParameter::Q, r#"brand=="x";serial=="y""#)]).await?;
    let forwarded = source
        .received_matching(&SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("q", r#"ex:brand=="x";"ex:serial.no"=="y""#))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
