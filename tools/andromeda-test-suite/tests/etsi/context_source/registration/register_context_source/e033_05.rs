//! A registration created as `application/json` with no JSON-LD `@context` expands under the default context, whatever
//! context its retrieval names (clauses 5.9.2 and 6.3.5).

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
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use serde_json::{Value, json};

/// The registration of the cases: every `Building`, created in the tenant `reader` speaks to with no `@context` at
/// all, requiring 201 and an empty body, and answering its identifier and the cleanup that deletes it.
async fn created(reader: &BrokerClient, namespace: &Namespace) -> Result<(RegistrationId, Cleanup)> {
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    let registration = CSourceRegistration::builder(SuiteConfig::get()?.unreachable_origin().as_str())
        // The registration keeps the identifier; the case reads it again for the retrieval and the teardown.
        .id(id.clone())
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .build()?;
    let cleanup = Cleanup::of(reader).registration(&id);
    let created = reader.with_responses().registrations().register(&registration).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(created?.body.len(), eq(0));
    Ok((id, cleanup))
}

/// Retrieves the entity type of the registration's first information element with `reader`.
async fn retrieved_type(reader: &BrokerClient, id: &RegistrationId) -> Result<Value> {
    let received = reader.with_accept(Accept::any()).with_responses().registrations().retrieve(id).send().await;
    json_body(&received?)?.pointer("/information/0/entities/0/type").cloned().or_fail()
}

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context and retrieve the information with ngsild context
/// Clause: 6.3.5
/// Tenant: private (the case registers a Building with no @context, which expands under the core default vocabulary,
/// where any other file that names Building without a @context shares it; such a registration would capture and
/// forward that file's queries (clause 5.12), and clause 4.14 keeps each tenant's registrations its own)
#[gtest]
#[tokio::test]
async fn e033_05_01_default_context_with_json_content_type_retrieved_with_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&tenant, &namespace, &["Building"]).await?;
    let (id, _registration) = created(&tenant, &namespace).await?;

    let reader = tenant.with_context(vocabulary.context())?;
    expect_that!(retrieved_type(&reader, &id).await?, eq(&json!("ngsi-ld:default-context/Building")));

    tenant.registrations().delete(&id).send().await?;
    Ok(())
}

/// Pins: Check that the default @context is used if the Content-Type header is "application/json" and the Link header
/// does not contain a JSON-LD @context and retrieve the information without ngsild context
/// Clause: 6.3.5
/// Tenant: private (the case registers a Building with no @context, which expands under the core default vocabulary,
/// where any other file that names Building without a @context shares it; such a registration would capture and
/// forward that file's queries (clause 5.12), and clause 4.14 keeps each tenant's registrations its own)
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e033_05_02_default_context_with_json_content_type_retrieved_without_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let (id, _registration) = created(&tenant, &namespace).await?;

    expect_that!(retrieved_type(&tenant, &id).await?, eq(&json!("Building")));

    tenant.registrations().delete(&id).send().await?;
    Ok(())
}
