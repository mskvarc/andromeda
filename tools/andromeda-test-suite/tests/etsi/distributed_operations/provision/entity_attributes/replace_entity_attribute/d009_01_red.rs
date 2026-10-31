//! A replacement of the `speed` of an entity two redirect registrations match is forwarded to each context source once,
//! and the broker answers a `speed` of 99 (clauses 4.3.3, 4.3.6.3, 5.6.19 and 6.3.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister, vehicle_retrieval_rule},
        provision::entity_attributes::attribute_sources::{replace_speed, speed_99_reply, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::retrieve_entity_json,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};
use serde_json::json;
use wiremock::{Mock, ResponseTemplate};

/// Pins: Check that one can replace an existing entity attribute, the changes will be forwarded to the Context Sources
/// thanks to the redirect registrations
/// Clause: 4.3.3, 4.3.6.3, 5.6.19, 6.3.18
/// Expected: the registration id is distinct, the body is an attribute replacement, and both sources answer (clause
/// 5.6.19)
#[gtest]
#[tokio::test]
async fn d009_01_red_replace_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed", "source"]).await?;
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
    let id = namespace.urn("Vehicle", 1)?;
    for below in ["/broker1", "/broker2"] {
        // The source answers the created id as bare text, which no JSON reply writes.
        let created = ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entities")));
        let reply = ResponseTemplate::new(StatusCode::CREATED).set_body_string(id.to_string());
        source.mount(Mock::given(created).respond_with(reply)).await?;
    }
    let rule = |below: &str| vehicle_rule(&namespace, Method::PUT, below, "/attrs/speed");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let linked = broker.with_context(vocabulary.context())?;
    replace_speed(&linked, &id).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    for below in ["/broker1", "/broker2"] {
        stub(
            &source,
            vehicle_retrieval_rule(&namespace, below, 1),
            StatusCode::OK,
            Some(&speed_99_reply(&namespace)),
        )
        .await?;
    }
    let body = retrieve_entity_json(&linked.with_accept(Accept::any()), &id).await?;
    expect_that!(body.pointer("/speed/value"), some(eq(&json!(99))));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
