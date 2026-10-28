//! ETSI `D007_01_inc`: replacing an entity an inclusive registration matches sends the replacement to the context
//! source and replaces the broker's entity as well (clauses 4.3.3, 4.3.6.2 and 5.6.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::{create_entity_with_link, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Check that if one requests the Context Broker to replace an entity that matches an inclusive registration,
/// this is replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.18
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: both retrievals are held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d007_01_inc_replace_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "isParked2"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_link(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_id_registration(&namespace, 1, 1, &endpoint, Some(RegistrationMode::Inclusive), REDIRECTION)?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let linked = broker.with_context(vocabulary.context())?;
    let reader = linked.with_accept(Accept::any()).with_responses();
    let old = json_body(&reader.entities().retrieve(&id).send().await?)?;
    stub(
        &source,
        ForkRule::resource(Method::PUT, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}"))),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    let replaced = linked.with_responses().entities().replace(&vehicle_parked2(&namespace, 1)?).send().await;
    verify_that!(&replaced, answered(StatusCode::NO_CONTENT))?;
    let body = json_body(&reader.entities().retrieve(&id).send().await?)?;
    expect_that!(body, not(eq(&old)));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
