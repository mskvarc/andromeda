//! ETSI `D006_01_red`: a deletion of the `speed` of an entity two redirect registrations match is forwarded to both
//! context sources (clauses 4.3.3, 4.3.6.3 and 5.6.5).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            DeletedInstances,
            delete_attribute_instance,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    suite_keywords::delete_entities,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has a redirect registration on a Context Broker with redirectionOps, one is able to
/// delete entities attributes on a Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.5
/// Corrected: `fix(D006_red)`: no local entity under a redirect registration (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d006_01_red_delete_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &vehicle_attribute_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect), &[])?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| vehicle_rule(&namespace, Method::DELETE, below, "/attrs/speed");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    delete_attribute_instance(&broker.with_context(vocabulary.context())?, &id, "speed", DeletedInstances::Default).await?;
    wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, gt(0));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
