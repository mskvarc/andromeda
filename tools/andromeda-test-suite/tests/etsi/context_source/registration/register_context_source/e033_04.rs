//! A registration created with the `@context` of a `Link` header and the `application/json` content type is retrieved
//! compacted under that context or expanded without one (clauses 5.9.2 and 6.3.5).

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::SuiteConfig,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::Input,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use serde_json::{Value, json};

/// The registration of the cases: every `Building`, at the unreachable origin.
fn registration(namespace: &Namespace) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .build()?)
}

/// Creates `registration` as `application/json` with the vocabulary in a `Link` header, requiring 201 and an empty
/// body, and answers its identifier and the cleanup that deletes it.
async fn created_with_link(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<(RegistrationId, Cleanup)> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    let created = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .register(registration)
        .send()
        .await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(created?.body.len(), eq(0));
    Ok((registration.id().cloned().or_fail()?, cleanup))
}

/// Retrieves `id` with `reader`, answering the entity type of the registration's first information element.
async fn retrieved_type(reader: &BrokerClient, id: &RegistrationId) -> Result<Value> {
    let received = reader.with_responses().registrations().retrieve(id).send().await;
    json_body(&received?)?.pointer("/information/0/entities/0/type").cloned().or_fail()
}

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e033_04_01_link_header_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let (id, _registration) = created_with_link(&broker, &vocabulary, &registration(&namespace)?).await?;

    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    expect_that!(retrieved_type(&reader, &id).await?, eq(&json!("Building")));

    broker.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
/// Clause: 6.3.5
#[gtest]
#[tokio::test]
async fn e033_04_02_link_header_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let (id, _registration) = created_with_link(&broker, &vocabulary, &registration(&namespace)?).await?;

    let reader = broker.with_accept(Accept::any());
    expect_that!(retrieved_type(&reader, &id).await?, eq(&json!(namespace.expanded_term("Building")?)));

    broker.registrations().delete(&id).send().await?;
    Ok(())
}
