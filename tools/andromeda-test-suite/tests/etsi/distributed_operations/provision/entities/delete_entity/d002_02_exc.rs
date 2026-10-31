//! Deleting an entity whose `speed` an exclusive registration holds for the retrieval operations alone is not forwarded
//! to the context source, answered 207 (clauses 4.3.3, 4.3.6.3 and 5.6.6).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, teardown::AnyAnswer, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{OperationGroup, RegistrationMode};

/// Pins: Check that the deletion is not forwarded to the Context Source due to a lack of redirection operations in
/// the registration.
/// Clause: 4.3.3, 4.3.6.3, 5.6.6
/// Expected: the exclusive registration declares attributes and retrieve-only operations (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d002_02_exc_delete_entity_without_redirection_operations() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed"]).await?;
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
    let id = namespace.urn("Vehicle", 1)?;
    let rule = || ForkRule::resource(Method::DELETE, &source_path(&namespace, &format!("/broker1/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule(), StatusCode::MULTI_STATUS, None).await?;
    expect_that!(&broker.with_responses().entities().delete(&id).send().await, answered(StatusCode::MULTI_STATUS));
    broker.temporal().delete(&id).send().await.any_answer();
    expect_that!(stub_count(&source, &rule()).await?, eq(0));
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
