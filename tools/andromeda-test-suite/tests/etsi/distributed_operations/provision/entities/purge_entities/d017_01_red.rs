//! ETSI `D017_01_red`: purging the Vehicles two redirect registrations match purges them on both context sources
//! (clauses 4.3.3, 4.3.6.3 and 5.6.21).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::{
            entity_sources::vehicle_type_registration,
            purge_entities::purge_request::{purge_rule, purge_vehicles},
        },
    },
    suite_keywords::create_entity_with_context_string,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has a redirect registration on a Context Broker, one is able to purge entities based
/// on type on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.21
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d017_01_red_purge_entities_on_the_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &vehicle_type_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect))?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let creation = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities")));
    stub(&source, creation("/broker1"), StatusCode::CREATED, None).await?;
    stub(&source, creation("/broker2"), StatusCode::CREATED, None).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    stub(&source, purge_rule(&namespace, "/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, purge_rule(&namespace, "/broker2"), StatusCode::NO_CONTENT, None).await?;
    purge_vehicles(&broker, &vocabulary).await?;
    expect_that!(stub_count(&source, &purge_rule(&namespace, "/broker1")).await?, gt(0));
    expect_that!(stub_count(&source, &purge_rule(&namespace, "/broker2")).await?, gt(0));
    let retrieved = broker.with_responses().entities().retrieve(&namespace.urn("Vehicle", 1)?).send().await;
    expect_that!(&retrieved, answered(StatusCode::NOT_FOUND));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
