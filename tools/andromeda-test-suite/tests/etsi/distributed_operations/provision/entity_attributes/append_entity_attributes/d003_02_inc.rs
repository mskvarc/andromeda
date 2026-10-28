//! ETSI `D003_02_inc`: an append with `noOverwrite` to an entity an inclusive registration matches is forwarded to
//! the context source with the flag and keeps the broker's attributes it would overwrite (clauses 4.3.3, 4.3.6.2 and
//! 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, unregister},
        provision::entity_attributes::attribute_sources::{
            append_without_overwrite,
            expect_no_overwrite_forwarded,
            parked_by_john_doe_and_speed,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    suite_keywords::{create_entity_with_context_string, delete_entities, ld_json_body_client, retrieve_entity_json},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Check that an entity attribute is appended and the inclusive registration forwards the request to the Context
/// Source with the noOverwrite flag
/// Clause: 4.3.3, 4.3.6.2, 5.6.3
/// Corrected: `fix(D003_inc)`: make the append and retrieves share the suite context (6.3.5)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: both retrievals are held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d003_02_inc_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Inclusive), &[])?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let old_body = retrieve_entity_json(&reader, &id).await?;
    stub(&source, vehicle_rule(&namespace, Method::POST, "", "/attrs"), StatusCode::MULTI_STATUS, None).await?;
    let taken = source.received().await?.len();
    append_without_overwrite(&ld_json_body_client(&broker, &vocabulary)?, &id, &parked_by_john_doe_and_speed(&namespace)?).await?;
    expect_no_overwrite_forwarded(&wait_for_request(&source, taken).await?);
    let body = retrieve_entity_json(&reader, &id).await?;
    expect_that!(body.pointer("/speed"), some(anything()));
    expect_that!(body.pointer("/isParked"), eq(old_body.pointer("/isParked")));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
