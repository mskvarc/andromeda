//! ETSI `D003_01_inc`: an append of attributes to an entity an inclusive registration matches is sent to the context
//! source and changes the broker's entity as well (clauses 4.3.3, 4.3.6.2 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{parked_by_john_doe_and_speed, vehicle_attribute_registration, vehicle_rule},
    },
    suite_keywords::{append_attributes, create_entity_with_context_string, delete_entities, ld_json_body_client, retrieve_entity_json},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::{headers::Accept, operation::Overwrite};

/// Pins: Check that, given an inclusive registration, appending entity attributes updates the Context Source
/// accordingly.
/// Clause: 4.3.3, 4.3.6.2, 5.6.3
/// Corrected: `fix(D003_inc)`: make the append and retrieves share the suite context (6.3.5)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: both retrievals are held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d003_01_inc_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_attribute_registration(&namespace, 1, &endpoint, None, &[])?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let old_body = retrieve_entity_json(&reader, &id).await?;
    let rule = || vehicle_rule(&namespace, Method::POST, "", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    append_attributes(
        &ld_json_body_client(&broker, &vocabulary)?,
        &id,
        &parked_by_john_doe_and_speed(&namespace)?,
        Overwrite::Replace,
    )
    .await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let body = retrieve_entity_json(&reader, &id).await?;
    expect_that!(body.pointer("/speed"), some(anything()));
    expect_that!(body.pointer("/isParked"), some(anything()));
    expect_that!(body.pointer("/isParked"), not(eq(old_body.pointer("/isParked"))));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
