//! An append of attributes to an entity whose `speed` an exclusive registration holds sends the `speed` to the context
//! source and keeps it off the broker (clauses 4.3.3, 4.3.6.3 and 5.6.3).

use crate::{
    distributed_operations::{
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, stub, stub_count, unregister},
        provision::entity_attributes::attribute_sources::{parked_by_john_doe_and_speed, vehicle_attribute_registration, vehicle_rule},
    },
    entity_operations::{append_attributes, delete_entities, ld_json_body_client},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    query::Locality,
    registration::RegistrationMode,
};

/// Pins: Check that an entity attribute is appended and the exclusive registration forwards the request to the Context
/// Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.3
/// Expected: the append is sent with the test vocabulary, so speed matches the registration (clauses 6.3.5, 5.12)
/// Expected: speed is absent (clause 4.3.6.3)
/// Expected: the append and the retrievals share the test vocabulary (clause 6.3.5)
#[gtest]
#[tokio::test]
async fn d003_01_exc_append_entity_attribute() -> Result<()> {
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
    let rule = || vehicle_rule(&namespace, Method::POST, "/broker1", "/attrs");
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    append_attributes(
        &ld_json_body_client(&broker, &vocabulary)?,
        &id,
        &parked_by_john_doe_and_speed(&namespace)?,
        Overwrite::Replace,
    )
    .await?;
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let retrieved = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&retrieved, answered(StatusCode::OK))?;
    let body = json_body(&retrieved?)?;
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.pointer("/speed"), none());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
