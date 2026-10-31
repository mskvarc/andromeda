//! Entities are purged by their type, by a q query, by the local flag and by a type with an id (clauses 5.6.21 and
//! 6.4.3.3).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes, pisa_tower},
    entity_operations::{create_entity, delete_entities},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::EntityId,
    query::{Locality, q},
};
use serde_json::json;

/// The id suffix of the first entity the cases create.
const FIRST: &str = "060-04-1";

/// The id suffix of the second entity the cases create.
const SECOND: &str = "060-04-2";

/// The identifiers of the two entities the cases create, in that order.
fn ids(namespace: &Namespace) -> Result<[EntityId; 2]> {
    Ok([namespace.urn("Building", FIRST)?, namespace.urn("Building", SECOND)?])
}

/// Pins: Check that purge entities deletes matching a type
/// Clause: 5.6.21, 6.4.3.3
#[gtest]
#[tokio::test]
async fn e060_03_01_purge_entities_matching_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, FIRST)?).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, SECOND)?).await?;
    let client = broker.with_context(vocabulary.context())?;
    let purged = client
        .with_responses()
        .entities()
        .purge()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body.len(), eq(0));
    let remaining = client
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    let body = json_body(&remaining?)?;
    expect_that!(&body, eq(&json!([])));
    let [first, second] = ids(&namespace)?;
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}

/// Pins: Check that purge entities deletes entities matching a q query
/// Clause: 5.6.21, 6.4.3.3
/// Tenant: private (a purge selected by a q query alone names no entity type, id or idPattern, so clause 5.12 matches it
/// with every registration of the tenant and the broker forwards it to each; in the shared tenant another file's
/// registration then answers the purge, or fails it, and the 204 becomes a 207 naming that registration)
#[gtest]
#[tokio::test]
async fn e060_03_02_purge_entities_matching_q_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&tenant, &vocabulary, &building_simple_attributes(&namespace, FIRST)?).await?;
    create_entity(&tenant, &vocabulary, &pisa_tower(&namespace, SECOND)?).await?;
    let client = tenant.with_context(vocabulary.context())?;
    let purged = client.with_responses().entities().purge().q(q::attr("name").eq("Eiffel Tower")).send().await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body.len(), eq(0));
    let [first, second] = ids(&namespace)?;
    let removed = client.with_responses().entities().retrieve(&first).send().await;
    verify_that!(&removed, answered(StatusCode::NOT_FOUND))?;
    let kept = client.with_responses().entities().retrieve(&second).send().await;
    verify_that!(&kept, answered(StatusCode::OK))?;
    delete_entities(&tenant, &[&first, &second]).await;
    Ok(())
}

/// Pins: Check that purge entities deletes all entities in local mode
/// Clause: 5.6.21, 6.4.3.3
/// Tenant: private (local=true alone selects every entity of the tenant, so the purge would delete the shared tenant's
/// entities of every other file)
#[gtest]
#[tokio::test]
async fn e060_03_03_purge_all_entities_in_local_mode() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&tenant, &vocabulary, &building_simple_attributes(&namespace, FIRST)?).await?;
    create_entity(&tenant, &vocabulary, &building_simple_attributes(&namespace, SECOND)?).await?;
    let purged = tenant.with_responses().entities().purge().locality(Locality::Local).send().await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body.len(), eq(0));
    let remaining = tenant
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .send()
        .await;
    let body = json_body(&remaining?)?;
    expect_that!(&body, eq(&json!([])));
    delete_entities(&tenant, &[&namespace.urn("Building", FIRST)?, &namespace.urn("Building", SECOND)?]).await;
    Ok(())
}

/// Pins: Check that purge entities deletes matching a type and an id
/// Clause: 5.6.21, 6.4.3.3
#[gtest]
#[tokio::test]
async fn e060_03_04_purge_entities_matching_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, FIRST)?).await?;
    create_entity(&broker, &vocabulary, &building_simple_attributes(&namespace, SECOND)?).await?;
    let client = broker.with_context(vocabulary.context())?;
    let purged = client
        .with_responses()
        .entities()
        .purge()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .ids(vec![namespace.urn("Building", FIRST)?])
        .send()
        .await;
    verify_that!(&purged, answered(StatusCode::NO_CONTENT))?;
    expect_that!(purged?.body.len(), eq(0));
    let [first, second] = ids(&namespace)?;
    let removed = client.with_responses().entities().retrieve(&first).send().await;
    verify_that!(&removed, answered(StatusCode::NOT_FOUND))?;
    let kept = client.with_responses().entities().retrieve(&second).send().await;
    verify_that!(&kept, answered(StatusCode::OK))?;
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}
