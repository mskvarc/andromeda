//! A partial update of the `speed` of an entity two redirect registrations match is forwarded to both context sources
//! with the `speed` member (clauses 4.3.3, 4.3.6.3 and 5.6.4).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            expect_speed_forwarded,
            speed_patch_document,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    entity_operations::{delete_entities, ld_json_body_client, partial_update_document},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{model::AttributeName, registration::RegistrationMode};

/// Pins: Check that if one request the Context Broker to partially update an attribute whose id matches a redirect
/// registration, the entity is updated on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.4
#[gtest]
#[tokio::test]
async fn d005_01_red_partial_partial_attribute_update() -> Result<()> {
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
    let rule = |below: &str| vehicle_rule(&namespace, Method::PATCH, below, "/attrs/speed");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    partial_update_document(&ld_json_body_client(&broker, &vocabulary)?, &id, &speed, &speed_patch_document()?).await?;
    let forwarded = wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, gt(0));
    expect_speed_forwarded(&forwarded)?;
    expect_that!(stub_count(&source, &rule("/broker2")).await?, gt(0));
    expect_speed_forwarded(&forwarded)?;
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
