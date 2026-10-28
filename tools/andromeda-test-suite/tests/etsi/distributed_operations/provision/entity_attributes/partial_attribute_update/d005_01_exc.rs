//! ETSI `D005_01_exc`: a partial update of the `speed` an exclusive registration holds is forwarded to the context
//! source with the `speed` member and leaves the broker's entity as it was (clauses 4.3.3, 4.3.6.3 and 5.6.4).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            expect_speed_forwarded,
            speed_patch_document,
            vehicle_attribute_registration,
            vehicle_parked2_by_alice,
            vehicle_rule,
            wait_for_request,
        },
    },
    suite_keywords::{create_entity_with_context_string, delete_entities, ld_json_body_client, partial_update_document},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::AttributeName, query::Locality, registration::RegistrationMode};

/// Pins: Check that if one request the Context Broker to partially update an attribute whose id matches an exclusive
/// registration, this is updated on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.4
/// Corrected: `fix(D005_01_exc)`: local entity holds no exclusively-covered attribute (4.3.6.3)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: Get Request Body is parsed as a Python `'''...'''` literal; the port refuses a backslash, a `'''` and a
/// final `'` and then requires the body to be JSON.
/// Changed: both local retrievals are held to a 200, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
/// Defect: D-79: Andromeda forwards the partial update as {"type":"Property","value":{"speed":{...}}}, the client's
/// speed Attribute nested inside a value member, so the forwarded body has no top-level speed member
#[gtest]
#[tokio::test]
async fn d005_01_exc_partial_partial_attribute_update() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2_by_alice(&namespace, None)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Exclusive), &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any()).with_responses();
    let old = reader.entities().retrieve(&id).locality(Locality::Local).send().await;
    verify_that!(&old, answered(StatusCode::OK))?;
    let old_body = json_body(&old?)?;
    let rule = || vehicle_rule(&namespace, Method::PATCH, "/broker1", "/attrs/speed");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let speed: AttributeName = "speed".parse()?;
    partial_update_document(&ld_json_body_client(&broker, &vocabulary)?, &id, &speed, &speed_patch_document()?).await?;
    let forwarded = wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    expect_speed_forwarded(&forwarded)?;
    let retrieved = reader.entities().retrieve(&id).locality(Locality::Local).send().await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    expect_that!(body.pointer("/isParked2"), eq(old_body.pointer("/isParked2")));
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.pointer("/speed"), none());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
