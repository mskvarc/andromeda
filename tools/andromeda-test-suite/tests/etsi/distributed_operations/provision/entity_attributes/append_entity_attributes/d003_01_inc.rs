//! An append of attributes to an entity an inclusive registration matches is sent to the context source and changes the
//! broker's entity as well (clauses 4.3.3, 4.3.6.2 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{parked_by_john_doe_and_speed, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::{append_attributes, create_entity_with_context_string, delete_entities, ld_json_body_client, retrieve_entity_json},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::{headers::Accept, operation::Overwrite};

/// Pins: Check that, given an inclusive registration, appending entity attributes updates the Context Source
/// accordingly.
/// Clause: 4.3.3, 4.3.6.2, 5.6.3
/// Expected: the append and the retrievals share the test vocabulary (clause 6.3.5)
#[gtest]
#[tokio::test]
async fn d003_01_inc_append_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(&broker, &vocabulary, &vehicle_attribute_registration(&namespace, 1, &endpoint, None, &[])?).await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let old_body = retrieve_entity_json(&reader, &id).await?;
    let rule = || vehicle_rule(&namespace, Method::POST, "", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    append_attributes(
        &ld_json_body_client(&broker, &vocabulary)?,
        &id,
        &parked_by_john_doe_and_speed(&namespace)?,
        Overwrite::Replace,
    )
    .await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let body = retrieve_entity_json(&reader, &id).await?;
    expect_that!(body.pointer("/speed"), some(anything()));
    expect_that!(body.pointer("/isParked"), some(anything()));
    expect_that!(body.pointer("/isParked"), not(eq(old_body.pointer("/isParked"))));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
