//! An append with `noOverwrite` to an entity whose `speed` an exclusive registration holds is forwarded to the context
//! source with the flag (clauses 4.3.3, 4.3.6.3 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, unregister},
        provision::entity_attributes::attribute_sources::{
            append_without_overwrite,
            expect_no_overwrite_forwarded,
            parked_by_john_doe_and_speed,
            vehicle_attribute_registration,
            vehicle_rule,
        },
        redirected_request::wait_for_redirected,
    },
    entity_operations::{delete_entities, ld_json_body_client},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::RegistrationMode;

/// Pins: Check that an entity attribute is appended and the exclusive registration forwards the request to the Context
/// Source with the noOverwrite flag
/// Clause: 4.3.3, 4.3.6.3, 5.6.3
/// Expected: the append is sent with the test vocabulary, so speed matches the registration (clauses 6.3.5, 5.12)
/// Expected: the append and the retrievals share the test vocabulary (clause 6.3.5)
#[gtest]
#[tokio::test]
async fn d003_02_exc_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
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
    stub(
        &source,
        vehicle_rule(&namespace, Method::POST, "/broker1", "/attrs"),
        StatusCode::MULTI_STATUS,
        None,
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    append_without_overwrite(&ld_json_body_client(&broker, &vocabulary)?, &id, &parked_by_john_doe_and_speed(&namespace)?).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_no_overwrite_forwarded(&forwarded);
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
