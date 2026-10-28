//! ETSI `D004_01_inc`: an update of an entity only an inclusive registration holds is forwarded to the context source
//! with the written `brandname` (clauses 4.3.3, 4.3.6.2 and 5.6.2).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{vehicle_attribute_registration, vehicle_rule},
        redirected_request::{forwarded_body, wait_for_redirected},
    },
    suite_keywords::update_attributes,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::ContextDelivery, model::EntityFragment};
use serde_json::json;

/// Pins: Check that if one request the Context Broker to update an entity that matches an inclusive registration, this
/// is updated on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.2
/// Corrected: `fix(D004_01_inc)`: update of a remote-only entity is 204 not 207 (5.6.2.4)
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Wait for redirected request replies 200 to a request the stub has already answered, so the server keeps
/// that reply for the next request no stub matches; the port queues it on the source as one reply of 200 and no body.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: Get Request Body is parsed as a Python `'''...'''` literal; the port refuses a backslash, a `'''` and a
/// final `'` and then requires the body to be JSON.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d004_01_inc_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandname"]).await?;
    let source = ContextSource::refusing()?;
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_attribute_registration(&namespace, 1, &endpoint, None, &[])?).await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::PATCH, "", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let fragment = EntityFragment::builder().property("brandname", "BMW").build()?;
    update_attributes(&writer, &namespace.urn("Vehicle", 1)?, &fragment).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    expect_that!(forwarded_body(&forwarded)?.pointer("/brandname/value"), some(eq(&json!("BMW"))));
    unregister(&broker, &namespace, 1).await
}
