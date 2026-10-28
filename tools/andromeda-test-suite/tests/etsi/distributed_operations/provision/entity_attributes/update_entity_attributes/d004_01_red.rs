//! ETSI `D004_01_red`: an update of an entity two redirect registrations hold is forwarded to both context sources
//! (clauses 4.3.3, 4.3.6.3 and 5.6.2).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{vehicle_attribute_registration, vehicle_rule},
    },
    suite_keywords::update_attributes,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::ContextDelivery, model::EntityFragment, registration::RegistrationMode};

/// Pins: Check that if one request the Context Broker to update an entity that matches an inclusive registration, this
/// is updated on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.2
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the teardown deletes of the registrations are held to the 204 a deleted registration answers, not only to
/// a status below 400.
#[gtest]
#[tokio::test]
async fn d004_01_red_query_the_context_broker_with_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandname"]).await?;
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
    let rule = |below: &str| vehicle_rule(&namespace, Method::PATCH, below, "/attrs");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let fragment = EntityFragment::builder().property("brandname", "BMW").build()?;
    update_attributes(&writer, &namespace.urn("Vehicle", 1)?, &fragment).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, gt(0));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
