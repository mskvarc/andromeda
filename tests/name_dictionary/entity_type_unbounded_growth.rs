//! Entity type dictionary growth: minting new type names is never refused.
//!
//! Entity type IRIs are open-world (clause 4.5.1) exactly as attribute IRIs
//! are, and the type dictionary's cache is the same implementation as the
//! attribute dictionary's, so a write that brings a new type is accepted at any
//! dictionary size and a write of an existing type keeps resolving it.

use crate::{SUITE, run_name_prefix};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// Types interned before the write under test.
///
/// Past 10,000 entries, so the check fails if a per-dictionary cap of that size
/// is ever enforced.
const TYPES_INTERNED_FIRST: i32 = 11_000;

/// An entity of one type under the core context's default vocabulary.
fn entity_of_type(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "@context": [CORE_CONTEXT]
    })
}

/// Delete an entity, tolerating one that was never created.
async fn discard(client: &TestClient, id: &str) {
    let _discarded = client.delete_entity(id).await;
}

#[tokio::test]
async fn a_type_dictionary_past_ten_thousand_types_still_mints_new_types_and_serves_existing_ones() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let client = environment.client();
    let pool = connect_test_database().await.expect("test database connection");
    let run = Uuid::new_v4();
    let prefix = run_name_prefix();
    let established_type = format!("TypeEstablished{}", run.simple());
    let novel_type = format!("TypeNovel{}", run.simple());
    let established_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:established:{run}");
    let novel_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:novel:{run}");
    let repeat_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:repeat:{run}");

    let established = client
        .post_entity(&entity_of_type(&established_entity, &established_type))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(established.status, StatusCode::CREATED, "create: {:?}", established.body);

    sqlx::query("INSERT INTO entity_type_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
        .bind(&prefix)
        .bind(TYPES_INTERNED_FIRST)
        .execute(&pool)
        .await
        .expect("interning distinct type names must not fail");
    let size: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entity_type_names")
        .fetch_one(&pool)
        .await
        .expect("the dictionary size must be observable");

    let novel = client
        .post_entity(&entity_of_type(&novel_entity, &novel_type))
        .send()
        .await
        .expect("create entity request failed");
    let novel_types: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entity_type_names WHERE name LIKE $1")
        .bind(format!("%{novel_type}"))
        .fetch_one(&pool)
        .await
        .expect("the dictionary must be readable");
    let repeated = client
        .post_entity(&entity_of_type(&repeat_entity, &established_type))
        .send()
        .await
        .expect("create entity request failed");

    // Every observation is taken before the entities are discarded and
    // asserted afterwards, so a failed assertion cannot leave them behind.
    discard(client, &established_entity).await;
    discard(client, &novel_entity).await;
    discard(client, &repeat_entity).await;
    pool.close().await;

    assert!(
        size > i64::from(TYPES_INTERNED_FIRST),
        "the dictionary must be past ten thousand types for this check to mean anything, size was {size}"
    );
    assert_eq!(novel.status, StatusCode::CREATED, "a new type is minted at any size: {:?}", novel.body);
    assert_eq!(novel_types, 1, "the new type is recorded exactly once");
    assert_eq!(repeated.status, StatusCode::CREATED, "an existing type stays usable: {:?}", repeated.body);

    environment.finish().await.expect("releasing exclusive ownership failed");
}
