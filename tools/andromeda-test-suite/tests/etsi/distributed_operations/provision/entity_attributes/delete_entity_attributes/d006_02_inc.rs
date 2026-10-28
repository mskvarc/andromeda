//! ETSI `D006_02_inc`: a deletion of every `speed` instance of an entity an inclusive registration matches is forwarded
//! to the context source and removes the `speed` from the broker (clauses 4.3.3, 4.3.6.2 and 5.6.5).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, unregister},
        provision::entity_attributes::attribute_sources::{
            DeletedInstances,
            delete_attribute_instance,
            vehicle_attribute_registration,
            vehicle_rule,
            vehicle_with_two_speeds,
            wait_for_request,
        },
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, query::Locality, registration::RegistrationMode};

/// Pins: Verify that, when one has an inclusive registration on a Context Broker with redirectionOps, one is able to
/// delete entities attributes on a Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.5
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: the local retrieval is held to a 200, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d006_02_inc_delete_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_with_two_speeds(&namespace)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Inclusive), &[])?,
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        vehicle_rule(&namespace, Method::DELETE, "", "/attrs/speed"),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    let linked = broker.with_context(vocabulary.context())?;
    delete_attribute_instance(&linked, &id, "speed", DeletedInstances::All).await?;
    wait_for_request(&source, 0).await?;
    let retrieved = linked
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.pointer("/speed"), none());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
