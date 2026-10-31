//! A deletion of every `speed` instance an exclusive registration holds is forwarded to the context source (clauses
//! 4.3.3, 4.3.6.3 and 5.6.5).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{
            DeletedInstances,
            delete_attribute_instance,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Verify that, when one has an exclusive registration on a Context Broker with redirectionOps, one is able to
/// delete entities attributes on a Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.5
#[gtest]
#[tokio::test]
async fn d006_02_exc_delete_entity_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Exclusive), &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || vehicle_rule(&namespace, Method::DELETE, "/broker1", "/attrs/speed");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    delete_attribute_instance(&broker.with_context(vocabulary.context())?, &id, "speed", DeletedInstances::All).await?;
    wait_for_request(&source, 0).await?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
