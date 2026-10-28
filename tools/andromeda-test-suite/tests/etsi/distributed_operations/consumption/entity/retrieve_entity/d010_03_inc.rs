//! ETSI `D010_03_inc`: a retrieval by id is forwarded to an inclusive context source registered for `retrieveEntity`,
//! and to none registered only for `queryEntity` or `queryBatch` (clauses 4.3.3, 4.3.6.2 and 5.7.1).

use crate::distributed_operations::{
    fork_rule::{ForkRule, QueriedEntity},
    forwarding_source::{
        register,
        registration_id,
        serve,
        source_endpoint,
        source_path,
        stub,
        stub_query_of,
        unregister,
        vehicle_reply,
        vehicle_retrieval_rule,
    },
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem},
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo},
};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 4] = ["Vehicle", "brandName", "isParked", "providedBy"];

/// The setup of the cases in the private tenant `broker`: registers the vocabulary and every Vehicle at the source for
/// `operation` alone, and starts the source; answers the vocabulary and the source.
async fn source_for(broker: &BrokerClient, namespace: &Namespace, operation: DistributedOperation) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    let source = ContextSource::refusing()?;
    let registration = CSourceRegistration::builder(source_endpoint(&source, namespace, "")?.as_str())
        .id(registration_id(namespace, 1)?)
        .information([RegistrationInfo::builder().entities(["Vehicle"]).build()?])
        .operations([operation])
        .build()?;
    let registrations = register(broker, &vocabulary, &registration).await?;
    Ok((vocabulary, serve(source).await?, registrations))
}

/// What a case's retrieval answers.
#[derive(Debug, Clone, Copy)]
enum Outcome {
    /// 200, the entity forwarded to the source.
    Found,
    /// 404 `ResourceNotFound`, nothing forwarded.
    NotFound,
}

/// The case's retrieval of `urn:ngsi-ld:Vehicle:<ns>-1` in the vocabulary's `Link` context, expected to answer
/// `outcome`.
async fn expect_retrieval(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, outcome: Outcome) -> Result<()> {
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .send()
        .await;
    match outcome {
        Outcome::Found => expect_that!(&retrieved, answered(StatusCode::OK)),
        Outcome::NotFound => expect_that!(&retrieved, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound)),
    }
    Ok(())
}

/// The expectations of the unforwarded cases: the source received no request in the 5 s after the retrieval.
async fn expect_no_request(source: &ContextSource) -> Result<()> {
    sleep(Duration::from_secs(5)).await;
    expect_that!(source.received().await?.len(), eq(0));
    Ok(())
}

/// Pins: Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration
/// only allows the retrieveEntity operation, the request is forwarded as expected
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Corrected: `fix(D010_03_inc)`: assert the unforwarded retrieve without counting the queryBatch stub (5.7.1.4)
/// Corrected: `fix(D010_03_inc)`: a retrieve is not forwarded to a query-only registration (5.7.1.4)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the suite names this case `D010_03_inc_01`, with the case number after the mode; the file name moves it
/// before the mode (d010-03-01-inc), as in the other files of the area
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d010_03_01_inc_retrieve_entity_by_id_using_the_retrieve_entity_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let (vocabulary, source, _registrations) = source_for(&broker, &namespace, DistributedOperation::RetrieveEntity).await?;
    stub(
        &source,
        vehicle_retrieval_rule(&namespace, "", 1),
        StatusCode::OK,
        Some(&vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")),
    )
    .await?;
    expect_retrieval(&broker, &vocabulary, &namespace, Outcome::Found).await?;
    unregister(&broker, &namespace, 1).await
}

/// Pins: Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration
/// only allows the queryEntity operation, the request is not forwarded and the entity is not found
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Corrected: `fix(D010_03_inc)`: assert the unforwarded retrieve without counting the queryBatch stub (5.7.1.4)
/// Corrected: `fix(D010_03_inc)`: a retrieve is not forwarded to a query-only registration (5.7.1.4)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the suite names this case `D010_03_inc_02`, with the case number after the mode; the file name moves it
/// before the mode (d010-03-02-inc), as in the other files of the area
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Absence: Wait For No Request 5 s
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d010_03_02_inc_retrieve_entity_by_id_using_the_query_entity_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let (vocabulary, source, _registrations) = source_for(&broker, &namespace, DistributedOperation::QueryEntity).await?;
    let reply = Value::Array(vec![vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")]);
    stub(
        &source,
        ForkRule::query(
            Method::GET,
            &source_path(&namespace, "/ngsi-ld/v1/entities"),
            &["type", "vehicle"],
            Some(&["brandname", "id", "isparked", "type"]),
        ),
        StatusCode::OK,
        Some(&reply),
    )
    .await?;
    expect_retrieval(&broker, &vocabulary, &namespace, Outcome::NotFound).await?;
    expect_no_request(&source).await?;
    unregister(&broker, &namespace, 1).await
}

/// Pins: Check that if one retrieves entity living on a Context Source from a Context Broker, and the registration
/// only allows the queryBatch operation, the request is not forwarded and the entity is not found
/// Clause: 4.3.3, 4.3.6.2, 5.7.1
/// Corrected: `fix(D010_03_inc)`: assert the unforwarded retrieve without counting the queryBatch stub (5.7.1.4)
/// Corrected: `fix(D010_03_inc)`: a retrieve is not forwarded to a query-only registration (5.7.1.4)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the suite names this case `D010_03_inc_03`, with the case number after the mode; the file name moves it
/// before the mode (d010-03-03-inc), as in the other files of the area
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
/// Absence: Wait For No Request 5 s
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d010_03_03_inc_retrieve_entity_by_id_using_the_query_batch_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let (vocabulary, source, _registrations) = source_for(&broker, &namespace, DistributedOperation::QueryBatch).await?;
    let reply = Value::Array(vec![vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")]);
    stub_query_of(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/query")),
        QueriedEntity {
            entity_type: "Vehicle".to_owned(),
            id: namespace.urn_text("Vehicle", 1),
        },
        StatusCode::OK,
        Some(&reply),
    )
    .await?;
    expect_retrieval(&broker, &vocabulary, &namespace, Outcome::NotFound).await?;
    expect_no_request(&source).await?;
    unregister(&broker, &namespace, 1).await
}
