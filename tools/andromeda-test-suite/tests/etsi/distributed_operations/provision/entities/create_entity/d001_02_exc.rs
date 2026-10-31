//! An entity whose `speed` an exclusive registration holds for the retrieval operations alone is refused as existing,
//! and nothing reaches the context source (clauses 4.3.3, 4.3.6.3 and 5.6.1).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
    },
    entity_operations::{delete_entities, send_create_entity},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, Property},
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Check that the entity cannot be created in the Context Source. The creation shall raise a conflict error.
/// Clause: 4.3.3, 4.3.6.3, 5.6.1
/// Expected: the exclusive registration declares attributes and retrieve-only operations (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d001_02_exc_create_entity_without_redirection_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let coverage = Coverage {
        operations: OperationGroup::RetrieveOps,
        properties: &["speed"],
    };
    let endpoint = source_endpoint(&source, &namespace, "/broker1")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_id_registration(&namespace, 1, 1, &endpoint, Some(RegistrationMode::Exclusive), coverage)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entities"));
    stub(&source, rule(), StatusCode::CONFLICT, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(56).property("source", "Speedometer"))
        .build()?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    expect_that!(&send_create_entity(&writer, &speed).await, answered(StatusCode::CONFLICT));
    expect_that!(stub_count(&source, &rule()).await?, eq(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
