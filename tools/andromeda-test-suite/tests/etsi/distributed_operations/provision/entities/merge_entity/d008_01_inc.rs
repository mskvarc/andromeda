//! Merging an entity an inclusive registration matches sends the merge to the context source and merges the broker's
//! entity as well (clauses 4.3.3, 4.3.6.2 and 5.6.17).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::entities::entity_sources::{REDIRECTION, vehicle_id_registration},
        vehicle_payloads::{vehicle_parked2, vehicle_speed_parked2_by_bob},
    },
    entity_operations::{create_entity_with_context_string, delete_entities, ld_json_body_client},
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, registration::RegistrationMode};

/// Pins: Check that if one requests the Context Broker to merge an entity that matches an inclusive registration,
/// this is merged on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.17
#[gtest]
#[tokio::test]
async fn d008_01_inc_merge_entity_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?);
    let endpoint = source_endpoint(&source, &namespace, "")?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_id_registration(&namespace, 1, 1, &endpoint, Some(RegistrationMode::Inclusive), REDIRECTION)?,
    )
    .await?;
    let source = serve(source).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let reader = broker.with_context(vocabulary.context())?.with_accept(Accept::any()).with_responses();
    let old = json_body(&reader.entities().retrieve(&id).send().await?)?;
    let old_is_parked = old.get("isParked2").or_fail()?;
    let old_brand_name = old.get("brandName").or_fail()?;
    let rule = || ForkRule::resource(Method::PATCH, &source_path(&namespace, &format!("/ngsi-ld/v1/entities/{id}")));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let fragment = vehicle_speed_parked2_by_bob(&namespace, 1)?.into_fragment();
    let merged = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .merge(&id, &fragment)
        .send()
        .await;
    verify_that!(&merged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(stub_count(&source, &rule()).await?, gt(0));
    let body = json_body(&reader.entities().retrieve(&id).send().await?)?;
    expect_that!(body.get("brandName"), some(eq(old_brand_name)));
    expect_that!(body.get("isParked2"), some(not(eq(old_is_parked))));
    expect_that!(body.get("speed"), some(anything()));
    delete_entities(&broker, &[&id]).await;
    unregister(&broker, &namespace, 1).await
}
