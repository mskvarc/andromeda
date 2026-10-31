//! A retrieve is forwarded only to the source whose folded id pattern covers it.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
    teardown::AnyAnswer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityId, ErrorType, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity of HIVE:9 is forwarded to the source registered with an `idPattern` folding hive, and
/// Retrieve Entity of Hivex:9 is not forwarded to it
/// Clause: 4.9, 5.7.1.4, 5.12
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_fed_ret_0001_a_retrieve_is_forwarded_only_to_the_source_whose_folded_id_pattern_covers_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let hive_type = namespace.entity_type("Hive")?;
    let ngsi_ld_hive_id = format!("urn:ngsi-ld:Hive:{namespace}:HIVE:9").parse::<EntityId>()?;
    let ngsi_ld_hive_id_2 = format!("urn:ngsi-ld:Hive:{namespace}:Hivex:9").parse::<EntityId>()?;
    let source_problem = source_answer::problem_without_detail(ErrorType::ResourceNotFound, "not here", None);
    source
        .answer_when(
            SourceRequest::any_matching("/.*").priority(1),
            source_answer::problem(StatusCode::NOT_FOUND, &source_problem),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(hive_type).with_id_pattern(format!("^urn:ngsi-ld:Hive:{namespace}:(?i:hive):").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_hive_id)
        .send()
        .await
        .any_answer();
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_hive_id_2)
        .send()
        .await
        .any_answer();
    let forwarded = source.received_on(Method::GET, &format!("/ngsi-ld/v1/entities/{ngsi_ld_hive_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request = SourceRequest::matching(Method::GET, "/.*");
    let forwarded_2 = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
