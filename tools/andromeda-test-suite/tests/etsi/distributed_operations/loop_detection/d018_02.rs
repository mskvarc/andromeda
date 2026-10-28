//! ETSI `D018_02`: a broker forwarding a create, an update, a delete, a replacement or a retrieval to an inclusive
//! context source sends the source a `Via` header (clauses 4.3.3, 5.6.6 and 6.3.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, registration_id, serve, source_endpoint, source_path, stub, unregister, vehicle_retrieval_rule},
        loop_detection::via_header::forwarded_via,
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{DatasetId, Entity, EntityFragment, EntityTypeName, Input, Property, Relationship},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// The terms of the `@context` every case registers.
const BASE_TERMS: [&str; 4] = ["Vehicle", "brandName", "isParked", "providedBy"];

/// The setup of the cases: registers `terms`, creates the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` as a Mercedes parked
/// at Downtown1, registers every Vehicle of the namespace inclusively for the redirection operations at the source's
/// `/broker1`, starts the source and stubs it to answer `status` to the requests `rule` matches; answers the
/// vocabulary and the source.
async fn via_source(
    broker: &BrokerClient,
    namespace: &Namespace,
    terms: &[&str],
    rule: ForkRule,
    status: StatusCode,
) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, terms).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(broker, &vocabulary, &vehicle_parked(namespace, 1)?).await?;
    let vehicles = EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id_pattern(namespace.id_pattern("Vehicle")?);
    let registration = CSourceRegistration::builder(source_endpoint(&source, namespace, "/broker1")?.as_str())
        .id(registration_id(namespace, 1)?)
        .information([RegistrationInfo::builder().entities([vehicles]).build()?])
        .operations([OperationGroup::RedirectionOps])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    let registrations = register(broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    stub(&source, rule, status, None).await?;
    Ok((vocabulary, source, registrations))
}

/// The stub rule of `method` on the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` at the source's `/broker1`, below `below`.
fn vehicle_rule(namespace: &Namespace, method: Method, below: &str) -> ForkRule {
    ForkRule::resource(
        method,
        &source_path(namespace, &format!("/broker1/ngsi-ld/v1/entities/{}{below}", namespace.urn_text("Vehicle", 1))),
    )
}

/// The teardown of the cases: deletes the Vehicles `-1` and `-2`, then the registration (204).
async fn torn_down(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(broker, &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?]).await;
    unregister(broker, namespace, 1).await
}

/// Pins: Verify that the request contains the POST Via header when forwarding operations to the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_02_01_check_post_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = [BASE_TERMS.as_slice(), &["speed", "source", "isParked2"]].concat();
    let rule = ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"));
    let (vocabulary, source, _registrations) = via_source(&broker, &namespace, &terms, rule, StatusCode::CREATED).await?;
    let speed = Property::builder(56)
        .property("source", "Speedometer")
        .dataset_id(namespace.urn_text("Property", "speedometerA4567-speed2").parse::<DatasetId>()?);
    let parked = Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
        .observed_at("2019-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?);
    let second: Entity<Input> = Entity::builder()
        .id(namespace.urn("Vehicle", 2)?)
        .entity_type("Vehicle")
        .property("speed", speed)
        .relationship("isParked2", parked)
        .build()?;
    create_entity_with_context_string(&broker, &vocabulary, &second).await?;
    expect_that!(forwarded_via(&source).await?, some(anything()));
    torn_down(&broker, &namespace).await
}

/// Pins: Verify that the request contains the PATCH Via header when forwarding operations to the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_02_02_check_update_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = [BASE_TERMS.as_slice(), &["brandname"]].concat();
    let rule = vehicle_rule(&namespace, Method::PATCH, "/attrs");
    let (vocabulary, source, _registrations) = via_source(&broker, &namespace, &terms, rule, StatusCode::NO_CONTENT).await?;
    let fragment = EntityFragment::builder().property("brandname", "BMW").build()?;
    let updated = broker
        .with_context(vocabulary.context())?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .with_responses()
        .entities()
        .update_attributes(&namespace.urn("Vehicle", 1)?, &fragment)
        .send()
        .await;
    expect_that!(&updated, answered(StatusCode::NO_CONTENT));
    expect_that!(forwarded_via(&source).await?, some(anything()));
    torn_down(&broker, &namespace).await
}

/// Pins: Verify that the request contains the DELETE Via header when forwarding operations to the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_02_03_check_delete_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let rule = vehicle_rule(&namespace, Method::DELETE, "");
    let (_vocabulary, source, _registrations) = via_source(&broker, &namespace, &BASE_TERMS, rule, StatusCode::NO_CONTENT).await?;
    let vehicle = namespace.urn("Vehicle", 1)?;
    let deleted = broker.with_responses().entities().delete(&vehicle).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    broker.temporal().delete(&vehicle).send().await.any_answer();
    expect_that!(forwarded_via(&source).await?, some(anything()));
    torn_down(&broker, &namespace).await
}

/// Pins: Verify that the request contains the PUT Via header when forwarding operations to the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_02_04_check_put_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = [BASE_TERMS.as_slice(), &["isParked2"]].concat();
    let rule = vehicle_rule(&namespace, Method::PUT, "");
    let (vocabulary, source, _registrations) = via_source(&broker, &namespace, &terms, rule, StatusCode::NO_CONTENT).await?;
    let replaced = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .replace(&vehicle_parked2(&namespace, 1)?)
        .send()
        .await;
    expect_that!(&replaced, answered(StatusCode::NO_CONTENT));
    expect_that!(forwarded_via(&source).await?, some(anything()));
    torn_down(&broker, &namespace).await
}

/// Pins: Verify that the request contains the GET Via header when forwarding operations to the Context Source
/// Clause: 4.3.3, 5.6.6, 6.3.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request reads the last request the source received once it holds one.
/// Changed: Get Request Headers reads the field without regard to case (RFC 9110 clause 5.1).
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d018_02_05_check_get_via_header_forwarding_to_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let rule = vehicle_retrieval_rule(&namespace, "/broker1", 1);
    let (_vocabulary, source, _registrations) = via_source(&broker, &namespace, &BASE_TERMS, rule, StatusCode::OK).await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&namespace.urn("Vehicle", 1)?)
        .send()
        .await;
    expect_that!(&retrieved, answered(StatusCode::OK));
    expect_that!(forwarded_via(&source).await?, some(anything()));
    torn_down(&broker, &namespace).await
}
