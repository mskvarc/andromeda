//! A deletion of every `speed` instance of an entity two redirect registrations match is forwarded to both context
//! sources (clauses 4.3.3, 4.3.6.3 and 5.6.5).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            DeletedInstances,
            delete_attribute_instance,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    entity_operations::delete_entities,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has a redirect registration on a Context Broker with redirectionOps, one is able to
/// delete entities attributes on a Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.5
/// Expected: no local entity exists under a redirect registration (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d006_02_red_delete_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "speed"]).await?;
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
    let rule = |below: &str| vehicle_rule(&namespace, Method::DELETE, below, "/attrs/speed");
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    delete_attribute_instance(&broker.with_context(vocabulary.context())?, &id, "speed", DeletedInstances::All).await?;
    wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule("/broker1")).await?, gt(0));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
