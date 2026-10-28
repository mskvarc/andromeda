//! ETSI `D001_01_red`: two entities, each matched by its own redirect registration, are created on the context source
//! of their registration (clauses 4.3.3, 4.3.6.3 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that if one requests the Context Broker to create an entity that matches two redirect registrations,
/// this is created only on the Context Sources
/// Clause: 4.3.3, 4.3.6.3, 5.6.1
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d001_01_red_create_entity_on_both_context_sources() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (n, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        let registration = vehicle_id_registration(&namespace, n, n, &endpoint, Some(RegistrationMode::Redirect), REDIRECTION)?;
        registrations.push(register(&broker, &vocabulary, &registration).await?);
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities")));
    stub(&source, rule("/broker1"), StatusCode::CREATED, None).await?;
    stub(&source, rule("/broker2"), StatusCode::CREATED, None).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, gt(0));
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    expect_that!(stub_count(&source, &rule("/broker2")).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?]).await;
    Ok(())
}
