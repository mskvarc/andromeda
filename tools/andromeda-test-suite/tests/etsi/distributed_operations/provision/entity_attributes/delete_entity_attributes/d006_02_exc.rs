//! ETSI `D006_02_exc`: a deletion of every `speed` instance an exclusive registration holds is forwarded to the
//! context source (clauses 4.3.3, 4.3.6.3 and 5.6.5).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            DeletedInstances,
            delete_attribute_instance,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    suite_keywords::delete_entities,
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has an exclusive registration on a Context Broker with redirectionOps, one is able to
/// delete entities attributes on a Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.5
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d006_02_exc_delete_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Exclusive), &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::DELETE, "/broker1", "/attrs/speed");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    delete_attribute_instance(&broker.with_context(vocabulary.context())?, &id, "speed", DeletedInstances::All).await?;
    wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
