//! Replacing an entity whose `speed` an exclusive registration holds sends the replacement to the context source once
//! and leaves no `speed` on the broker (clauses 4.3.3, 4.3.6.3 and 5.6.18).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{Coverage, vehicle_id_registration},
        vehicle_payloads::vehicle_speed_parked2,
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    query::Locality,
    registration::{OperationGroup, RegistrationMode},
};

/// Pins: Check that if one requests the Context Broker to replace an entity that matches an exclusive registration,
/// the entity is replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.18
/// Expected: the replaced speed is absent locally (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d007_01_exc_replace_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(
        &broker,
        &namespace,
        &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "isParked2"],
    )
    .await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let coverage = Coverage {
        operations: OperationGroup::RedirectionOps,
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
    let rule = || ForkRule::resource(Method::PUT, &source_path(&namespace, &format!("/broker1/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let linked = broker.with_context(vocabulary.context())?;
    let replaced = linked.with_responses().entities().replace(&vehicle_speed_parked2(&namespace, 1)?).send().await;
    expect_that!(&replaced, answered(StatusCode::NO_CONTENT));
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    let retrieved = linked
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&id)
        .locality(Locality::Local)
        .send()
        .await?;
    let body = json_body(&retrieved)?;
    expect_that!(body.as_object(), some(anything()));
    expect_that!(body.get("speed"), none());
    unregister(&broker, &namespace, 1).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
