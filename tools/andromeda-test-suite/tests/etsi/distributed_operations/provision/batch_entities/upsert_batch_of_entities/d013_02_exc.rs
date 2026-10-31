//! A batch upsert in the update mode of the `speed` an exclusive registration holds for two broker entities forwards
//! only that attribute, and the mode, to the context source (clauses 4.3.3, 4.3.6.3 and 5.6.8).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{create_entity_locally, register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::{delete_vehicles, exclusive_batch_registration},
            forwarded_documents::speed_forwarded,
            upsert_batch_of_entities::upsert_request::upsert,
        },
        redirected_request::{forwarded_body, forwarded_options, wait_for_redirected},
        vehicle_payloads::{vehicle_parked2, vehicle_speed_parked2},
    },
    entity_operations::ld_json_body_client,
};
use andromeda_test_suite::{broker, comparison::matches_document, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: Check that if one requests the Context Broker to replace a batch of entities that match an exclusive
/// registration, these are replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.3, 5.6.8
/// Expected: the exclusive batch registration names the entity by id and only the covered attribute is forwarded
/// (clauses 4.3.6.3, 5.6.8.4)
/// Expected: an exclusive registration names its entities by id (clause 4.3.6.3)
#[gtest]
#[tokio::test]
async fn d013_02_exc_batch_upsert_entities_with_exclusive_registration_with_update_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked2", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked2(&namespace, 1)?).await?;
    create_entity_locally(&broker, &vocabulary, &vehicle_parked2(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &exclusive_batch_registration(&namespace, &source_endpoint(&source, &namespace, "/broker1")?)?,
    )
    .await?;
    let source = serve(source).await?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/broker1/ngsi-ld/v1/entityoperations/upsert"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    upsert(
        &writer,
        &[vehicle_speed_parked2(&namespace, 1)?, vehicle_speed_parked2(&namespace, 2)?],
        "update",
    )
    .await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(
        forwarded_options(&forwarded),
        any![elements_are![contains_substring("update")], all![len(gt(1)), contains(eq("update"))]]
    );
    let body = forwarded_body(&forwarded)?;
    let entities = body.as_array().or_fail()?;
    expect_that!(entities, contains(matches_document(speed_forwarded(&namespace, 1))));
    expect_that!(entities, contains(matches_document(speed_forwarded(&namespace, 2))));
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
