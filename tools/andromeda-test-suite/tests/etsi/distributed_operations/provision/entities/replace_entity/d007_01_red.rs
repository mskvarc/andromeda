//! ETSI `D007_01_red`: replacing an entity two redirect registrations match sends the replacement to both context
//! sources once (clauses 4.3.3, 4.3.6.3 and 5.6.18).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
    provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    vehicle_payloads::vehicle_parked2,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that if one requests the Context Broker to replace an entity that matches redirect registrations, the
/// entity is replaced on the Context Sources
/// Clause: 4.3.3, 4.3.6.3, 5.6.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d007_01_red_replace_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        let registration = vehicle_id_registration(&namespace, suffix, 1, &endpoint, Some(RegistrationMode::Redirect), REDIRECTION)?;
        registrations.push(register(&broker, &vocabulary, &registration).await?);
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let rule = |below: &str| ForkRule::resource(Method::PUT, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let replaced = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .replace(&vehicle_parked2(&namespace, 1)?)
        .send()
        .await;
    expect_that!(&replaced, answered(StatusCode::NO_CONTENT));
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
