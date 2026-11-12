//! A context source alias is an HTTP token.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextSourceAlias, ErrorType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with a `contextSourceAlias` holding a character outside the `tchar` of
/// IETF RFC 7230 (`example.com:8080`, `broker/a`, `broker@a`, `"broker"`) answers 400 `BadRequestData` and stores no
/// registration, and one made only of `tchar` answers 201 and reads back as written
/// Clause: 5.9.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_6019_a_context_source_alias_is_an_http_token() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let information = format!(r#"[{{"entities":[{{"idPattern":"urn:ngsi-ld:Vehicle:{namespace}-.*","type":"Vehicle_{namespace}"}}]}}]"#);
    // Each alias breaks the grammar the client's model enforces, so the registrations are written as text.
    let aliases = [r#""example.com:8080""#, r#""broker/a""#, r#""broker@a""#, r#""\"broker\"""#];
    let mut refused_ids = Vec::new();
    let mut bodies = Vec::new();
    for (suffix, alias) in (1..).zip(aliases) {
        let id = namespace.registration_id(suffix)?;
        bodies.push(format!(
            r#"{{"id":"{id}","type":"ContextSourceRegistration","information":{information},"endpoint":"http://source.test/{suffix}","contextSourceAlias":{alias}}}"#
        ));
        refused_ids.push(id);
    }
    expect_bad_request_data(&broker, &Method::POST, "csourceRegistrations", &bodies).await?;
    for id in &refused_ids {
        let absent = broker.with_responses().with_accept(Accept::any()).registrations().retrieve(id).send().await;
        expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    let registration_id = namespace.registration_id(5)?;
    let alias = format!("{namespace}_b~c!#$%&'*+^`|").parse::<ContextSourceAlias>()?;
    let registration = CSourceRegistration::builder(source.endpoint("5")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .context_source_alias(alias.clone())
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.context_source_alias(), some(eq(&alias)));
    Ok(())
}
