//! ETSI `D003_02_red`: an append with `noOverwrite` to an entity two redirect registrations match is forwarded to the
//! context sources with the flag (clauses 4.3.3, 4.3.6.3 and 5.6.3).

use crate::distributed_operations::{
    forwarding_source::{register, serve, source_endpoint, stub, unregister},
    provision::entity_attributes::attribute_sources::{
        append_without_overwrite,
        expect_no_overwrite_forwarded,
        parked_by_john_doe_and_speed,
        vehicle_attribute_registration,
        vehicle_rule,
    },
    redirected_request::wait_for_redirected,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that an entity attribute is appended and the redirect registration forwards the request to the Context
/// Source with the noOverwrite flag
/// Clause: 4.3.3, 4.3.6.3, 5.6.3
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Wait For Request hands over the most recent request the server holds; the port waits until the source
/// holds a request earlier waits did not take and reads the last one, which is the one the case's action forwarded.
/// Changed: Wait for redirected request replies 200 to a request the stub has already answered, so the server keeps
/// that reply for the next request no stub matches; the port queues it on the source as one reply of 200 and no body.
/// Changed: providedBy stays out of the file vocabulary: the case reads it without a @context, so it expands under the
/// core default vocabulary as in the Robot run
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d003_02_red_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "isParked", "speed", "source"]).await?;
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
    for below in ["/broker1", "/broker2"] {
        stub(&source, vehicle_rule(&namespace, Method::POST, below, "/attrs"), StatusCode::MULTI_STATUS, None).await?;
    }
    let id = namespace.urn("Vehicle", 1)?;
    append_without_overwrite(&broker, &id, &parked_by_john_doe_and_speed(&namespace)?).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_no_overwrite_forwarded(&forwarded);
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
