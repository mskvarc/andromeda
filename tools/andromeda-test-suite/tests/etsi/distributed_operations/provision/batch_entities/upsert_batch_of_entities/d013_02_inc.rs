//! A batch upsert in the update mode of two entities an inclusive registration matches is sent to the context source
//! with the mode and updates the entities on the broker as well (clauses 4.3.3, 4.3.6.2 and 5.6.8).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::batch_entities::{
            batch_sources::{batch_registration, delete_vehicles},
            upsert_batch_of_entities::upsert_request::upsert,
        },
        redirected_request::{forwarded_options, wait_for_redirected},
        vehicle_payloads::vehicle_parked2,
    },
    entity_operations::{create_entity_with_context_string_any, ld_json_body_client},
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Check that if one requests the Context Broker to update a batch of entities that match an inclusive
/// registration, these are updated on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.8
/// Isolation: the registration's idPattern is `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that names no entity type
/// matches a registration on the id alone (clause 5.12), so a pattern without the namespace would draw every other
/// test's requests on a Vehicle id into this test's source
#[gtest]
#[tokio::test]
async fn d013_02_inc_batch_upsert_entities_with_inclusive_registration_with_update_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "isParked2"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string_any(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_with_context_string_any(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?).entity(&namespace.urn("Vehicle", 2)?);
    let _registration = register(
        &broker,
        &vocabulary,
        &batch_registration(&namespace, 1, &source_endpoint(&source, &namespace, "")?, None)?,
    )
    .await?;
    let source = serve(source).await?;
    let reader = broker.with_context(vocabulary.context())?;
    let first = namespace.urn("Vehicle", 1)?;
    let old = json_body(&reader.with_accept(Accept::any()).with_responses().entities().retrieve(&first).send().await?)?;
    let old_brand_name = old.get("brandName").or_fail()?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/upsert")),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    upsert(&writer, &[vehicle_parked2(&namespace, 1)?, vehicle_parked2(&namespace, 2)?], "update").await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(
        forwarded_options(&forwarded),
        any![elements_are![contains_substring("update")], all![len(gt(1)), contains(eq("update"))]]
    );
    reader
        .with_accept(NgsiLdMediaType::JsonLd)
        .entities()
        .query()
        .ids(vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .send()
        .await
        .any_answer();
    let updated = json_body(&reader.with_accept(Accept::any()).with_responses().entities().retrieve(&first).send().await?)?;
    expect_that!(updated.get("brandName"), some(not(eq(old_brand_name))));
    expect_that!(updated.get("isParked2"), some(anything()));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
