//! An append with `noOverwrite` to an entity an inclusive registration matches is forwarded to the context source with
//! the flag and keeps the broker's attributes it would overwrite (clauses 4.3.3, 4.3.6.2 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, unregister},
        provision::entity_attributes::attribute_sources::{
            append_without_overwrite,
            expect_no_overwrite_forwarded,
            parked_by_john_doe_and_speed,
            vehicle_attribute_registration,
            vehicle_rule,
            wait_for_request,
        },
    },
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client, retrieve_entity_json},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Check that an entity attribute is appended and the inclusive registration forwards the request to the Context
/// Source with the noOverwrite flag
/// Clause: 4.3.3, 4.3.6.2, 5.6.3
/// Expected: the append and the retrievals share the test vocabulary (clause 6.3.5)
#[gtest]
#[tokio::test]
async fn d003_02_inc_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_attribute_registration(&namespace, 1, &endpoint, Some(RegistrationMode::Inclusive), &[])?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let old_body = retrieve_entity_json(&reader, &id).await?;
    stub(&source, vehicle_rule(&namespace, Method::POST, "", "/attrs"), StatusCode::MULTI_STATUS, None).await?;
    let taken = source.received().await?.len();
    append_without_overwrite(&ld_json_body_client(&broker, &vocabulary)?, &id, &parked_by_john_doe_and_speed(&namespace)?).await?;
    expect_no_overwrite_forwarded(&wait_for_request(&source, taken).await?);
    let body = retrieve_entity_json(&reader, &id).await?;
    expect_that!(body.pointer("/speed"), some(anything()));
    expect_that!(body.pointer("/isParked"), eq(old_body.pointer("/isParked")));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
