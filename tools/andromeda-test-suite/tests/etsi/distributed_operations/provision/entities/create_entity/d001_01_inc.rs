//! ETSI `D001_01_inc`: an entity created on the broker that an inclusive registration matches is created on the
//! context source and on the broker (clauses 4.3.3, 4.3.6.2 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        redirected_request::{forwarded_body, wait_for_request},
    },
    suite_keywords::{create_entity_with_context_string, delete_entities},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, query::Locality};
use serde_json::{Value, json};

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration,
/// this is created on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.1
/// Corrected: `fix(D001_01_inc)`: register redirection ops so the create is forwarded (table 4.20-2)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Get Request Body is parsed as a Python `'''...'''` literal; the port refuses a backslash, a `'''` and a
/// final `'` and then requires the body to be JSON.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d001_01_inc_create_entity_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_id_registration(&namespace, 1, 1, &endpoint, None, REDIRECTION)?).await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entities")),
        StatusCode::CREATED,
        None,
    )
    .await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let forwarded = forwarded_body(&wait_for_request(&source, 0).await?)?;
    let id = namespace.urn_text("Vehicle", 1);
    expect_that!(forwarded.get("id"), some(eq(&json!(id))));
    let listed = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&listed, answered(StatusCode::OK))?;
    let body = json_body(&listed?)?;
    let entities = body.as_array().or_fail()?;
    expect_that!(entities, len(eq(1)));
    expect_that!(
        entities
            .iter()
            .find(|entity| entity.get("id").and_then(Value::as_str) == Some(id.as_str()))
            .and_then(Value::as_object),
        some(anything())
    );
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
