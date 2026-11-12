//! An update to an alias outside the token characters is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::ContextSourceAlias,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with a `contextSourceAlias` holding a character outside the `tchar` of
/// IETF RFC 7230 (`example.com:8080`, `broker/a`, `broker@a`, `"broker"`) answers 400 `BadRequestData` each time, and
/// the registration reads back with the alias it was created with
/// Clause: 5.9.3.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_upd_6008_an_update_to_an_alias_outside_the_token_characters_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let alias = format!("alias-{namespace}").parse::<ContextSourceAlias>()?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .context_source_alias(alias.clone())
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Each alias breaks the grammar the client's model enforces, so the fragments are written as text.
    let fragments = [r#""example.com:8080""#, r#""broker/a""#, r#""broker@a""#, r#""\"broker\"""#].map(|alias| format!(r#"{{"contextSourceAlias":{alias}}}"#));
    expect_bad_request_data(&broker, &Method::PATCH, &format!("csourceRegistrations/{registration_id}"), &fragments).await?;
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(retrieved.context_source_alias(), some(eq(&alias)));
    Ok(())
}
